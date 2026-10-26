use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    project: Metadata,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    pub kind: TargetKind,
    pub entry: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Executable,
}

#[derive(Debug)]
pub struct Project {
    pub manifest: PathBuf,
    pub directory: PathBuf,
    pub name: String,
    pub targets: Vec<Target>,
}

impl Project {
    pub fn discover(start: &Path) -> Result<PathBuf, String> {
        let start = start
            .canonicalize()
            .map_err(|error| format!("{}: {error}", start.display()))?;
        for directory in start.ancestors() {
            let manifest = directory.join("crust.toml");
            if manifest.is_file() {
                return Ok(manifest);
            }
        }
        return Err(format!(
            "could not find crust.toml from {}",
            start.display()
        ));
    }

    pub fn load(path: &Path) -> Result<Project, String> {
        let path = if path.is_dir() {
            path.join("crust.toml")
        } else {
            path.to_owned()
        };
        let manifest = path
            .canonicalize()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let source = fs::read_to_string(&manifest)
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
        let parsed: Manifest =
            toml::from_str(&source).map_err(|error| format!("{}: {error}", manifest.display()))?;
        let directory = manifest
            .parent()
            .expect("absolute manifest has a parent")
            .to_owned();
        if parsed.project.name.trim().is_empty() {
            return Err(format!(
                "{}: project.name must have a value",
                manifest.display()
            ));
        }
        if parsed.targets.is_empty() {
            return Err(format!(
                "{}: define at least one [[targets]] entry",
                manifest.display()
            ));
        }
        let mut names = HashSet::new();
        let mut targets = parsed.targets;
        for target in &mut targets {
            if !target
                .name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !target
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(format!(
                    "{}: target names must start with a letter or digit and contain letters, digits, '-' or '_'",
                    manifest.display()
                ));
            }
            if !names.insert(target.name.to_ascii_lowercase()) {
                return Err(format!(
                    "{}: duplicate target name '{}'",
                    manifest.display(),
                    target.name
                ));
            }
            let entry = directory.join(&target.entry);
            target.entry = entry.canonicalize().map_err(|error| {
                format!(
                    "{}: target '{}' entry {}: {error}",
                    manifest.display(),
                    target.name,
                    entry.display()
                )
            })?;
            if !target.entry.is_file() {
                return Err(format!(
                    "{}: target '{}' entry must be a file",
                    manifest.display(),
                    target.name
                ));
            }
        }
        return Ok(Project {
            manifest,
            directory,
            name: parsed.project.name,
            targets,
        });
    }

    pub fn select(&self, name: Option<&str>, single: bool) -> Result<Vec<&Target>, String> {
        if let Some(name) = name {
            return self
                .targets
                .iter()
                .find(|target| target.name == name)
                .map(|target| vec![target])
                .ok_or_else(|| format!("{}: unknown target '{name}'", self.manifest.display()));
        }
        if single && self.targets.len() != 1 {
            return Err(format!(
                "{}: select a target with --target <name>: {}",
                self.manifest.display(),
                self.targets
                    .iter()
                    .map(|target| target.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        return Ok(self.targets.iter().collect());
    }
}
