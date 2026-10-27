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
}

impl Project {
    pub fn metadata(&self) -> Result<ProjectMetadata, String> {
        let mut targets = Vec::new();
        for target in &self.targets {
            targets.push(TargetMetadata {
                name: target.name.clone(),
                kind: target.kind,
                entry: target.entry.clone(),
                output: target.kind.artifact_path(&self.output_base(target))?,
                link_targets: target.native.link_targets.clone(),
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
