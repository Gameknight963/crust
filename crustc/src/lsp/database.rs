use super::*;
use crate::analysis::Analysis;
use crate::source::{SourceError, SourceSet};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

type Checked = Result<Rc<Analysis>, Rc<SourceError>>;

pub(super) struct Snapshot {
    pub sources: Sources,
    analyses: RefCell<HashMap<(usize, bool), Checked>>,
}

impl Snapshot {
    pub fn analyze(&self, index: usize, completion: bool) -> Checked {
        if let Some(result) = self.analyses.borrow().get(&(index, completion)) {
            return result.clone();
        }
        // A fully checked program also supplies completion and signature data.
        if completion && let Some(Ok(analysis)) = self.analyses.borrow().get(&(index, false)) {
            return Ok(analysis.clone());
        }
        let graph: &SourceSet = &self.sources.graphs[index];
        let kind = self
            .sources
            .targets
            .get(&graph.files()[0].path)
            .copied()
            .unwrap_or_default();
        let result = if completion {
            graph.analyze_completion_with_kind(kind)
        } else {
            graph.analyze_with_kind(kind)
        }
        .map(Rc::new)
        .map_err(Rc::new);
        self.analyses
            .borrow_mut()
            .insert((index, completion), result.clone());
        return result;
    }
}

/// Per-connection immutable source snapshots shared by diagnostics and features.
/// Watch notifications invalidate disk inputs. Text is part of the key because
/// completion recovery uses temporary documents with the same version number.
#[derive(Default)]
pub(super) struct Database {
    snapshots: RefCell<HashMap<u64, Rc<Snapshot>>>,
}

impl Database {
    pub fn clear(&self) {
        self.snapshots.borrow_mut().clear();
    }

    pub fn snapshot(
        &self,
        documents: &HashMap<String, Document>,
        roots: &[PathBuf],
    ) -> Rc<Snapshot> {
        let mut key = std::collections::hash_map::DefaultHasher::new();
        roots.hash(&mut key);
        let mut entries: Vec<_> = documents.iter().collect();
        entries.sort_by_key(|(uri, _)| *uri);
        for (uri, document) in entries {
            uri.hash(&mut key);
            document.native.hash(&mut key);
            document.version.hash(&mut key);
            document.text.hash(&mut key);
        }
        let key = key.finish();
        if let Some(snapshot) = self.snapshots.borrow().get(&key) {
            return snapshot.clone();
        }
        let snapshot = Rc::new(Snapshot {
            sources: load_sources(documents, roots),
            analyses: RefCell::new(HashMap::new()),
        });
        let mut cache = self.snapshots.borrow_mut();
        // Retain only a few repair snapshots, bounding memory during typing.
        if cache.len() >= 4 {
            cache.clear();
        }
        cache.insert(key, snapshot.clone());
        return snapshot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_and_features_share_analysis_until_text_or_disk_changes() {
        let uri = path_uri(&std::env::temp_dir().join("crust-analysis-cache/main.cr"));
        let mut documents = HashMap::from([(
            uri.clone(),
            Document {
                native: false,
                version: 1,
                text: "int answer(){return 42;} int main(){return answer();}".into(),
            },
        )]);
        let database = Database::default();
        let first = database.snapshot(&documents, &[]);
        let diagnostics = first.analyze(0, false).unwrap();
        let request = database.snapshot(&documents, &[]);
        assert!(Rc::ptr_eq(&first, &request));
        assert!(Rc::ptr_eq(
            &diagnostics,
            &request.analyze(0, false).unwrap()
        ));
        assert!(Rc::ptr_eq(&diagnostics, &request.analyze(0, true).unwrap()));

        // Completion repairs keep document versions but replace text.
        documents.get_mut(&uri).unwrap().text =
            "int other(){return 7;} int main(){return other();}".into();
        let edited = database.snapshot(&documents, &[]);
        assert!(!Rc::ptr_eq(&first, &edited));
        assert!(
            edited
                .analyze(0, false)
                .unwrap()
                .symbols
                .iter()
                .any(|symbol| symbol.name == "other")
        );

        // File watcher and workspace events discard all disk-dependent results.
        database.clear();
        assert!(!Rc::ptr_eq(&edited, &database.snapshot(&documents, &[])));
    }
}
