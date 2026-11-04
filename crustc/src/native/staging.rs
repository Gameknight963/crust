use super::*;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_BUILD: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Staging {
    directory: PathBuf,
    files: Vec<PathBuf>,
}

impl Staging {
    pub(super) fn create(parent: &Path) -> Result<Self, String> {
        let parent = parent.canonicalize().map_err(|error| error.to_string())?;
        loop {
            let directory = parent.join(format!(
                "crust-build-{}-{}",
                std::process::id(),
                NEXT_BUILD.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        directory,
                        files: Vec::new(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("could not create build staging directory: {error}"));
                }
            }
        }
    }

    pub(super) fn file(&mut self, name: &str) -> PathBuf {
        let path = self.directory.join(name);
        self.files.push(path.clone());
        return path;
    }

    pub(super) fn preserve(&mut self, path: &Path) {
        self.files.retain(|file| file != path);
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
