use super::semantic::{Context, RequestError};
use super::*;
use crate::analysis::SymbolKind;

pub(super) fn roots(params: &Value) -> Vec<PathBuf> {
    let mut paths: Vec<_> = params["workspaceFolders"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|folder| folder["uri"].as_str().and_then(uri_path))
        .collect();
    if paths.is_empty()
        && let Some(path) = params["rootUri"].as_str().and_then(uri_path)
    {
        paths.push(path);
    }
    paths.sort();
    paths.dedup();
    return paths;
}

#[derive(Deserialize)]
struct Folder {
    uri: String,
}

#[derive(Deserialize)]
struct FolderEvent {
    added: Vec<Folder>,
    removed: Vec<Folder>,
}

#[derive(Deserialize)]
struct FolderParams {
    event: FolderEvent,
}

pub(super) fn change(params: Value, roots: &mut Vec<PathBuf>) -> ServerResult<()> {
    let params: FolderParams = serde_json::from_value(params)?;
    let paths = |folders: Vec<Folder>| -> ServerResult<Vec<PathBuf>> {
        return folders
            .into_iter()
            .map(|folder| {
                return uri_path(&folder.uri)
                    .ok_or_else(|| "workspace folders require local file URIs".into());
            })
            .collect();
    };
    let added = paths(params.event.added)?;
    let removed = paths(params.event.removed)?;
    roots.retain(|path| !removed.contains(path));
    roots.extend(added);
    roots.sort();
    roots.dedup();
    return Ok(());
}

#[derive(Deserialize)]
struct SymbolParams {
    query: String,
}

pub(super) fn symbols(
    params: Value,
    documents: &HashMap<String, Document>,
    roots: &[PathBuf],
) -> Result<Value, RequestError> {
    let params: SymbolParams = serde_json::from_value(params)?;
    let query = params.query.to_ascii_lowercase();
    let sources = source_graphs(documents, roots, false).map_err(RequestError::failed)?;
    let mut contexts = Vec::new();
    for graph in sources.graphs {
        let kind = sources
            .targets
            .get(&graph.files()[0].path)
            .copied()
            .unwrap_or_default();
        if let Ok(analysis) = graph.analyze_with_kind(kind) {
            contexts.push(Context {
                analysis,
                file: 0,
                uri: path_uri(&graph.files()[0].path),
                uris: sources.paths.clone(),
            });
        }
    }
    for (uri, document) in documents {
        if uri_path(uri).is_none()
            && let Ok(analysis) = crate::analyze(&document.text)
        {
            contexts.push(Context {
                analysis,
                file: 0,
                uri: uri.clone(),
                uris: HashMap::new(),
            });
        }
    }
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for context in contexts {
        for symbol in &context.analysis.symbols {
            if matches!(
                symbol.kind,
                SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::TypeParameter
            ) {
                continue;
            }
            let name = symbol
                .container
                .as_ref()
                .map(|container| format!("{container}::{}", symbol.name))
                .unwrap_or_else(|| symbol.name.clone());
            if !name.to_ascii_lowercase().contains(&query) {
                continue;
            }
            let information = context.symbol_information(symbol, symbol.declaration);
            let uri = information["location"]["uri"].as_str().unwrap().to_owned();
            if seen.insert((
                uri,
                symbol.declaration.span.start,
                symbol.declaration.span.end,
                symbol.kind,
            )) {
                results.push(information);
            }
        }
    }
    results.sort_by_cached_key(|value| {
        (
            value["name"].as_str().unwrap().to_owned(),
            value["location"]["uri"].as_str().unwrap().to_owned(),
            value["location"]["range"]["start"]["line"].as_u64(),
            value["location"]["range"]["start"]["character"].as_u64(),
        )
    });
    return Ok(Value::Array(results));
}

pub(super) fn source_graphs(
    documents: &HashMap<String, Document>,
    roots: &[PathBuf],
    strict: bool,
) -> Result<Sources, String> {
    let mut sources = load_sources(documents, roots);
    let mut entries: HashSet<_> = sources
        .graphs
        .iter()
        .map(|graph| graph.files()[0].path.clone())
        .collect();
    let mut manifests: HashSet<_> = sources
        .paths
        .keys()
        .filter_map(|path| {
            path.parent()?
                .ancestors()
                .map(|directory| directory.join("crust.toml"))
                .find(|manifest| manifest.is_file())
        })
        .collect();
    manifests.extend(
        roots
            .iter()
            .filter_map(|root| crate::project::Project::discover(root).ok()),
    );
    for manifest in manifests {
        let project = match crate::project::Project::load(&manifest) {
            Ok(project) => project,
            Err(error) if strict => return Err(error),
            Err(_) => continue,
        };
        for target in project.targets {
            sources
                .targets
                .insert(target.entry.clone(), target.kind.program_kind());
            if !entries.insert(target.entry.clone()) {
                continue;
            }
            match crate::source::SourceSet::load_with(&target.entry, |path| {
                source_text(path, &sources.paths, documents)
            }) {
                Ok(graph) => sources.graphs.push(graph),
                Err(error) => sources.errors.push(error),
            }
        }
    }
    if strict && let Some(error) = sources.errors.first() {
        return Err(format!(
            "global rename requires readable project sources: {error}"
        ));
    }
    sources.graphs.sort_by(|a, b| {
        b.files()
            .len()
            .cmp(&a.files().len())
            .then_with(|| a.files()[0].path.cmp(&b.files()[0].path))
    });
    return Ok(sources);
}
