use super::semantic::{Context, RequestError};
use super::*;
use crate::analysis::{Location, TextEdit};
use crate::diagnostic::Span;
use std::collections::BTreeMap;

type Edits = BTreeMap<(String, usize, usize), (Span, String)>;

fn append(context: &Context, edits: Vec<TextEdit>, output: &mut Edits) -> Result<(), RequestError> {
    for edit in edits {
        let span = edit.location.span;
        let key = (context.uri(edit.location.file), span.start, span.end);
        if let Some((_, previous)) = output.get(&key) {
            if previous != &edit.new_text {
                return Err(RequestError::failed(
                    "rename has conflicting edits in shared source",
                ));
            }
        } else {
            output.insert(key, (span, edit.new_text));
        }
    }
    return Ok(());
}

pub(super) fn global_edits(
    initial: &Context,
    offset: usize,
    name: &str,
    documents: &HashMap<String, Document>,
    roots: &[PathBuf],
) -> Result<BTreeMap<String, Vec<Value>>, RequestError> {
    let (selected, _) = initial
        .analysis
        .rename_target(initial.file, offset)
        .ok_or_else(|| {
            RequestError::failed(
                "select a function, method, typedef, interface, type parameter, or external variable to rename",
            )
        })?;
    let first = initial
        .analysis
        .rename(initial.file, offset, name)
        .map_err(RequestError::failed)?;
    if first.is_empty() {
        return Ok(BTreeMap::new());
    }
    let declarations: HashSet<_> = initial
        .analysis
        .symbols
        .iter()
        .filter(|symbol| symbol.declaration == selected.declaration)
        .map(|symbol| {
            (
                initial.uri(symbol.location.file),
                symbol.location.span.start,
                symbol.location.span.end,
                symbol.kind,
            )
        })
        .collect();
    let sources =
        super::workspace::source_graphs(documents, roots, true).map_err(RequestError::failed)?;
    let mut covered: HashSet<_> = initial
        .analysis
        .files
        .iter()
        .filter_map(|file| file.path.clone())
        .collect();
    let mut contexts = Vec::new();
    let mut failures = Vec::new();
    for graph in sources.graphs {
        let entry = &graph.files()[0].path;
        if initial.analysis.files[0].path.as_ref() == Some(entry)
            || (covered.contains(entry) && !sources.targets.contains_key(entry))
        {
            continue;
        }
        let kind = sources.targets.get(entry).copied().unwrap_or_default();
        match graph.analyze_with_kind(kind) {
            Ok(analysis) => {
                covered.extend(graph.files().iter().map(|file| file.path.clone()));
                contexts.push(Context {
                    analysis,
                    file: 0,
                    uri: path_uri(&graph.files()[0].path),
                    uris: sources.paths.clone(),
                });
            }
            Err(error) => failures.push((
                graph
                    .files()
                    .iter()
                    .map(|file| {
                        sources
                            .paths
                            .get(&file.path)
                            .cloned()
                            .unwrap_or_else(|| path_uri(&file.path))
                    })
                    .collect::<Vec<_>>(),
                error.to_string(),
            )),
        }
    }
    for (uri, document) in documents {
        if uri_path(uri).is_none()
            && uri != &initial.uri
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
    let mut snapshots = HashMap::new();
    for context in std::iter::once(initial).chain(&contexts) {
        for (file, source) in context.analysis.files.iter().enumerate() {
            let uri = context.uri(file);
            if let Some(previous) = snapshots.insert(uri, &source.text)
                && previous != &source.text
            {
                return Err(RequestError::failed(
                    "source changed while preparing rename",
                ));
            }
        }
    }
    let mut edits = Edits::new();
    append(initial, first, &mut edits)?;
    for context in &contexts {
        let symbol = context.analysis.symbols.iter().find(|symbol| {
            declarations.contains(&(
                context.uri(symbol.location.file),
                symbol.location.span.start,
                symbol.location.span.end,
                symbol.kind,
            ))
        });
        if let Some(symbol) = symbol {
            let local = context
                .analysis
                .rename(symbol.location.file, symbol.location.span.start, name)
                .map_err(RequestError::failed)?;
            append(context, local, &mut edits)?;
        }
    }
    let changed: HashSet<_> = edits.keys().map(|(uri, _, _)| uri).collect();
    for (files, error) in failures {
        if files.iter().any(|uri| changed.contains(uri)) {
            return Err(RequestError::failed(format!(
                "rename requires affected targets to check successfully: {error}"
            )));
        }
    }
    for context in std::iter::once(initial).chain(&contexts) {
        let files: HashMap<_, _> = (0..context.analysis.files.len())
            .map(|file| (context.uri(file), file))
            .collect();
        let mut local: Vec<_> = edits
            .iter()
            .filter_map(|((uri, _, _), (span, text))| {
                files.get(uri).map(|file| TextEdit {
                    location: Location {
                        file: *file,
                        span: *span,
                    },
                    new_text: text.clone(),
                })
            })
            .collect();
        if local.is_empty() {
            continue;
        }
        local.sort_by_key(|edit| {
            (
                edit.location.file,
                edit.location.span.start,
                edit.location.span.end,
            )
        });
        context
            .analysis
            .validate_rename_edits(&local)
            .map_err(RequestError::failed)?;
    }
    let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for ((uri, _, _), (span, text)) in edits {
        let source = snapshots[&uri];
        changes.entry(uri).or_default().push(json!({ "range": { "start": position(source, span.start), "end": position(source, span.end) }, "newText": text }));
    }
    return Ok(changes);
}
