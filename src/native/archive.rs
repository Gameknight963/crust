use super::*;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_BUILD: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn validate_options(options: &Options) -> Result<(), String> {
    validate_defines(&options.defines)?;
    if !options.libraries.is_empty() || !options.library_dirs.is_empty() {
        return Err("static-library targets accept native sources, include directories, defines, and object link-inputs".to_owned());
    }
    for input in &options.link_inputs {
        if !input.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("o") || extension.eq_ignore_ascii_case("obj")
        }) {
            return Err(format!(
                "static-library link input must be an object file (.o or .obj): {}",
                input.display()
            ));
        }
    }
    return Ok(());
}

struct Staging {
    directory: PathBuf,
    files: Vec<PathBuf>,
}

impl Staging {
    fn create(parent: &Path) -> Result<Self, String> {
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
                    return Err(format!(
                        "could not create archive staging directory: {error}"
                    ));
                }
            }
        }
    }

    fn file(&mut self, name: &str) -> PathBuf {
        let path = self.directory.join(name);
        self.files.push(path.clone());
        return path;
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

pub fn compile_static_library(
    c_path: &Path,
    archive: &Path,
    options: &Options,
) -> Result<(), String> {
    validate_options(options)?;
    let parent = archive.parent().ok_or("archive path needs a directory")?;
    let mut staging = Staging::create(parent)?;
    let compiler = env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    let mut objects = Vec::new();
    for (index, source) in std::iter::once(c_path)
        .chain(options.sources.iter().map(PathBuf::as_path))
        .enumerate()
    {
        let object = staging.file(&format!("source_{index}.o"));
        let mut command = Command::new(&compiler);
        configure_windows_toolchain(&mut command);
        command.args(["-std=c11", "-c", "-x", "c"]);
        for define in &options.defines {
            command.arg("-D").arg(define);
        }
        for directory in &options.include_dirs {
            command.arg("-I").arg(directory);
        }
        command.arg("-o").arg(&object).arg("--").arg(source);
        let result = command.output().map_err(|error| {
            format!(
                "could not start C compiler '{}': {error}",
                compiler.to_string_lossy()
            )
        })?;
        if !result.status.success() {
            return Err(format!(
                "C compiler failed for {}:\n{}{}",
                source.display(),
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        objects.push(object);
    }
    for (index, input) in options.link_inputs.iter().enumerate() {
        let object = staging.file(&format!("input_{index}.o"));
        fs::copy(input, &object)
            .map_err(|error| format!("could not copy {}: {error}", input.display()))?;
        objects.push(object);
    }
    let temporary = staging.file("archive.a");
    let archiver = env::var_os("AR").unwrap_or_else(|| {
        if cfg!(windows) {
            "llvm-ar".into()
        } else {
            "ar".into()
        }
    });
    let result = Command::new(&archiver)
        .arg("rcs")
        .arg(&temporary)
        .args(objects)
        .output()
        .map_err(|error| {
            format!(
                "could not start archiver '{}': {error}. Set AR to an ar-compatible executable",
                archiver.to_string_lossy()
            )
        })?;
    if !result.status.success() {
        return Err(format!(
            "archiver failed for {}:\n{}{}",
            archive.display(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    if archive.exists() {
        fs::remove_file(archive)
            .map_err(|error| format!("could not replace {}: {error}", archive.display()))?;
    }
    fs::rename(&temporary, archive)
        .map_err(|error| format!("could not write {}: {error}", archive.display()))?;
    return Ok(());
}
