use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub format_version: u32,
    pub manifest: PathBuf,
    pub directory: PathBuf,
    pub name: String,
    pub targets: Vec<TargetMetadata>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMetadata {
    pub name: String,
    pub kind: TargetKind,
    pub entry: PathBuf,
    pub output: PathBuf,
    pub link_targets: Vec<String>,
    pub debug: bool,
    pub runtime_paths: Vec<PathBuf>,
    pub runtime_libraries: Vec<PathBuf>,
}

impl Project {
    pub fn metadata(&self) -> Result<ProjectMetadata, String> {
        let mut targets = Vec::new();
        for target in &self.targets {
            let runtime_libraries = self.runtime_libraries(target)?;
            targets.push(TargetMetadata {
                name: target.name.clone(),
                kind: target.kind,
                entry: target.entry.clone(),
                output: target.kind.artifact_path(&self.output_base(target))?,
                link_targets: target.native.link_targets.clone(),
                debug: target.native.debug,
                runtime_paths: super::links::runtime_directories(&runtime_libraries),
                runtime_libraries,
            });
        }
        return Ok(ProjectMetadata {
            format_version: 1,
            manifest: self.manifest.clone(),
            directory: self.directory.clone(),
            name: self.name.clone(),
            targets,
        });
    }
}
