use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) struct ToolPaths {
    original: PathBuf,
    directory: Option<PathBuf>,
}

impl ToolPaths {
    pub fn for_output(output: &Path) -> Result<Self, String> {
        let original = env::current_dir()
            .map_err(|error| format!("could not resolve native tool directory: {error}"))?;
        // LLVM's temporary-output patterns substitute '%' in directory names.
        // Keeping the output argument relative avoids substituting its parent.
        let directory = if cfg!(windows) && output.to_string_lossy().contains('%') {
            let absolute = original.join(output);
            Some(
                absolute
                    .parent()
                    .ok_or("native output needs a directory")?
                    .to_owned(),
            )
        } else {
            None
        };
        return Ok(Self {
            original,
            directory,
        });
    }

    pub fn command(&self, tool: &OsStr) -> Command {
        let path = Path::new(tool);
        let relative_tool = path.is_relative()
            && path
                .parent()
                .is_some_and(|parent| !parent.as_os_str().is_empty());
        let mut command = if self.directory.is_some() && relative_tool {
            Command::new(self.original.join(path))
        } else {
            Command::new(tool)
        };
        if let Some(directory) = &self.directory {
            command.current_dir(directory);
        }
        return command;
    }

    pub fn input(&self, path: &Path) -> PathBuf {
        return if self.directory.is_some() {
            self.original.join(path)
        } else {
            path.to_owned()
        };
    }

    pub fn output(&self, path: &Path) -> PathBuf {
        return if self.directory.is_some() {
            PathBuf::from(path.file_name().expect("native output has a filename"))
        } else {
            path.to_owned()
        };
    }
}
