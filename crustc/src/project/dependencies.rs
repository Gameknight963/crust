use super::*;
use std::collections::BTreeMap;

type Exports = HashMap<String, String>;

#[derive(Default)]
struct Graph {
    targets: Vec<Target>,
    manifests: Vec<PathBuf>,
    exports: HashMap<PathBuf, Exports>,
    active: Vec<PathBuf>,
}

pub(super) fn load(path: &Path) -> Result<Project, LoadError> {
    let (mut project, dependencies) = Project::load_one(path)?;
    let mut graph = Graph::default();
    project.aliases = graph.expand(&mut project, dependencies, "")?;
    project.targets = graph.targets;
    project.manifests = graph.manifests;
    project.ordered_targets(&project.targets.iter().collect::<Vec<_>>())?;
    return Ok(project);
}

impl Graph {
    fn expand(
        &mut self,
        project: &mut Project,
        dependencies: BTreeMap<String, Dependency>,
        prefix: &str,
    ) -> Result<Exports, LoadError> {
        if let Some(start) = self
            .active
            .iter()
            .position(|path| path == &project.manifest)
        {
            let cycle = self.active[start..]
                .iter()
                .chain(std::iter::once(&project.manifest))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(format!("project dependency cycle: {cycle}").into());
        }
        if let Some(exports) = self.exports.get(&project.manifest) {
            return Ok(exports.clone());
        }
        if self.active.len() >= 128 {
            return Err(format!(
                "{}: project dependency nesting exceeds 128 manifests",
                project.manifest.display()
            )
            .into());
        }
        self.active.push(project.manifest.clone());
        self.manifests.push(project.manifest.clone());
        let start = self.targets.len();
        let mut exports = Exports::new();
        for mut target in std::mem::take(&mut project.targets) {
            let qualified = format!("{prefix}{}", target.name);
            exports.insert(target.name.clone(), qualified.clone());
            target.name = qualified;
            self.targets.push(target);
        }
        let end = self.targets.len();
        let mut names = HashSet::new();
        for (name, dependency) in dependencies {
            if !name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err(format!(
                    "{}: dependency names must be unique ignoring ASCII case, start with a letter or digit, and contain letters, digits, '-' or '_'",
                    project.manifest.display()
                ).into());
            }
            let path = project.directory.join(&dependency.path);
            let (mut child, nested) = Project::load_one(&path).map_err(|error| LoadError {
                message: format!(
                    "{}: dependency '{name}': {}",
                    project.manifest.display(),
                    error.message
                ),
                // Child TOML offsets do not describe the root manifest.
                range: None,
            })?;
            let child_exports = self.expand(&mut child, nested, &format!("{prefix}{name}:"))?;
            for (route, target) in child_exports {
                exports.insert(format!("{name}:{route}"), target);
            }
        }
        for target in &mut self.targets[start..end] {
            for link in &mut target.native.link_targets {
                *link = exports.get(link).cloned().ok_or_else(|| {
                    format!(
                        "{}: target '{}' links unknown target '{link}'",
                        project.manifest.display(),
                        target.name
                    )
                })?;
            }
        }
        self.active.pop();
        self.exports
            .insert(project.manifest.clone(), exports.clone());
        return Ok(exports);
    }
}
