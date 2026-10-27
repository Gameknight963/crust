use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

mod links;

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
    #[serde(default)]
    pub native: crate::native::Options,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Executable,
    StaticLibrary,
}

impl TargetKind {
    pub fn artifact_path(self, output: &Path) -> Result<PathBuf, String> {
        return Ok(match self {
            Self::Executable => output.with_extension(std::env::consts::EXE_EXTENSION),
            Self::StaticLibrary if cfg!(windows) => output.with_extension("lib"),
            Self::StaticLibrary => {
                let mut name = std::ffi::OsString::from("lib");
                name.push(output.file_name().ok_or("output needs a filename")?);
                name.push(".a");
                output.with_file_name(name)
            }
        });
    }

    pub fn program_kind(self) -> crate::ProgramKind {
        return match self {
            Self::Executable => crate::ProgramKind::Executable,
            Self::StaticLibrary => crate::ProgramKind::Library,
        };
    }
}

#[derive(Debug)]
pub struct Project {
    pub manifest: PathBuf,
    pub directory: PathBuf,
    pub name: String,
    pub targets: Vec<Target>,
}

#[derive(Debug)]
pub(crate) struct LoadError {
    pub message: String,
    pub range: Option<std::ops::Range<usize>>,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        return Self {
            message,
            range: None,
        };
    }
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
        return Self::load_diagnostic(path).map_err(|error| error.message);
    }

    pub(crate) fn load_diagnostic(path: &Path) -> Result<Project, LoadError> {
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
            toml::from_str(&source).map_err(|error: toml::de::Error| LoadError {
                range: error.span(),
                message: format!("{}: {error}", manifest.display()),
            })?;
        let directory = manifest
            .parent()
            .expect("absolute manifest has a parent")
            .to_owned();
        if parsed.project.name.trim().is_empty() {
            return Err(format!("{}: project.name must have a value", manifest.display()).into());
        }
        if parsed.targets.is_empty() {
            return Err(format!(
                "{}: define at least one [[targets]] entry",
                manifest.display()
            )
            .into());
        }
        let mut names = HashSet::new();
        let mut entry_kinds = HashMap::new();
        let mut targets = parsed.targets;
        for target in &mut targets {
            if target.kind == TargetKind::StaticLibrary {
                crate::native::validate_archive_options(&target.native).map_err(|error| {
                    format!("{}: target '{}': {error}", manifest.display(), target.name)
                })?;
            }
            crate::native::validate_defines(&target.native.defines).map_err(|error| {
                format!("{}: target '{}': {error}", manifest.display(), target.name)
            })?;
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
                ).into());
            }
            if !names.insert(target.name.to_ascii_lowercase()) {
                return Err(format!(
                    "{}: duplicate target name '{}'",
                    manifest.display(),
                    target.name
                )
                .into());
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
                )
                .into());
            }
            if let Some(previous) = entry_kinds.insert(target.entry.clone(), target.kind)
                && previous != target.kind
            {
                return Err(format!(
                    "{}: entry {} is used by incompatible target kinds",
                    manifest.display(),
                    target.entry.display()
                )
                .into());
            }
            let mut inputs = HashSet::new();
            for (kind, paths, is_directory) in [
                ("source", &mut target.native.sources, false),
                ("link input", &mut target.native.link_inputs, false),
                ("include directory", &mut target.native.include_dirs, true),
                ("library directory", &mut target.native.library_dirs, true),
            ] {
                for path in paths {
                    let original = directory.join(&*path);
                    *path = original.canonicalize().map_err(|error| {
                        format!(
                            "{}: target '{}' native {kind} {}: {error}",
                            manifest.display(),
                            target.name,
                            original.display()
                        )
                    })?;
                    if (is_directory && !path.is_dir()) || (!is_directory && !path.is_file()) {
                        return Err(format!(
                            "{}: target '{}' native {kind} {} must be a {}",
                            manifest.display(),
                            target.name,
                            path.display(),
                            if is_directory { "directory" } else { "file" }
                        )
                        .into());
                    }
                    if !is_directory && !inputs.insert(path.clone()) {
                        return Err(format!(
                            "{}: target '{}' repeats native input {}",
                            manifest.display(),
                            target.name,
                            path.display()
                        )
                        .into());
                    }
                }
            }
            for library in &target.native.libraries {
                if library.trim().is_empty() || library.contains(['\0', '/', '\\']) {
                    return Err(format!(
                        "{}: target '{}' native library names must be nonempty names. Use link-inputs for paths",
                        manifest.display(),
                        target.name
                    ).into());
                }
            }
        }
        let project = Project {
            manifest,
            directory,
            name: parsed.project.name,
            targets,
        };
        project.ordered_targets(&project.targets.iter().collect::<Vec<_>>())?;
        return Ok(project);
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
