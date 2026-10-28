use super::staging::Staging;
use super::*;
use std::fs;
use std::io::Read;

pub(crate) fn validate_options(options: &Options) -> Result<(), String> {
    validate_defines(&options.defines)?;
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

pub fn compile_static_library(
    c_path: &Path,
    archive: &Path,
    options: &Options,
) -> Result<(), String> {
    resolved_targets(options)?;
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
        let paths = tool_paths::ToolPaths::for_output(&object)?;
        let mut command = paths.command(&compiler);
        configure_windows_toolchain(&mut command);
        command.args(["-std=c11", "-c", "-x", "c"]);
        super::instrumentation(&mut command, options);
        if options.debug {
            command.arg("-g");
        }
        if !cfg!(windows) {
            command.arg("-fPIC");
        }
        for define in &options.defines {
            command.arg("-D").arg(define);
        }
        for directory in &options.include_dirs {
            command.arg("-I").arg(paths.input(directory));
        }
        // Absolute source paths cannot be interpreted as compiler options.
        // GCC does not accept a standalone '--' delimiter here.
        let source_path = env::current_dir()
            .map_err(|error| error.to_string())?
            .join(source);
        command
            .arg("-o")
            .arg(paths.output(&object))
            .arg(paths.input(&source_path));
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
    let paths = tool_paths::ToolPaths::for_output(&temporary)?;
    let result = paths
        .command(&archiver)
        .arg("rcs")
        .arg(paths.output(&temporary))
        .args(objects.iter().map(|object| paths.input(object)))
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
    let mut signature = [0; 8];
    fs::File::open(&temporary)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| {
            format!(
                "archiver did not produce a regular archive for {}: {error}",
                archive.display()
            )
        })?;
    if &signature != b"!<arch>\n" {
        return Err(format!(
            "archiver did not produce a regular archive for {}",
            archive.display()
        ));
    }
    fs::rename(&temporary, archive)
        .map_err(|error| format!("could not write {}: {error}", archive.display()))?;
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_and_links_a_source_with_an_option_like_filename() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/archive-source-tests")
            .join(std::process::id().to_string());
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("--source.c");
        fs::write(&source, "int answer(void) { return 42; }").unwrap();
        let archive = directory.join("answer.a");
        compile_static_library(&source, &archive, &Options::default()).unwrap();
        let consumer = directory.join("consumer.c");
        fs::write(
            &consumer,
            "int answer(void); int main(void) { return answer() == 42 ? 0 : 1; }",
        )
        .unwrap();
        let executable = directory.join(format!("consumer{}", std::env::consts::EXE_SUFFIX));
        compile_file_with_options(
            &consumer,
            &executable,
            &Options {
                link_inputs: vec![archive],
                ..Default::default()
            },
        )
        .unwrap();
        let status = Command::new(executable).status().unwrap();
        assert!(status.success(), "{status:?}");
    }
}
