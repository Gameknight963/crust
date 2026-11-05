use crate::invocation::Mode;
use crustc::project::TargetKind;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn process_source(
    path: &Path,
    mode: Mode,
    output: &Path,
    protected: &[PathBuf],
    native: &crustc::native::Options,
    kind: TargetKind,
    output_is_file: bool,
) -> Result<ExitCode, String> {
    match mode {
        Mode::Tokens => {
            let sources = crustc::source::SourceSet::load_for_target(path, native, kind, |path| {
                fs::read_to_string(path)
            })
            .map_err(|error| error.to_string())?;
            for (path, token) in sources.tokens() {
                println!(
                    "{}:{}:{}  {:?}",
                    path.display(),
                    token.span.line,
                    token.span.column,
                    token.kind
                );
            }
            return Ok(ExitCode::SUCCESS);
        }
        Mode::Check => {
            let sources = crustc::source::SourceSet::load_for_target(path, native, kind, |path| {
                fs::read_to_string(path)
            })
            .map_err(|error| error.to_string())?;
            sources
                .check_with_kind(kind.program_kind())
                .map_err(|error| error.to_string())?;
            println!("{}: OK", path.display());
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let sources = crustc::source::SourceSet::load_for_target(path, native, kind, |path| {
        fs::read_to_string(path)
    })
    .map_err(|error| error.to_string())?;
    if mode == Mode::EmitHeader {
        let header = sources
            .emit_header(kind.program_kind())
            .map_err(|error| error.to_string())?;
        print!("{header}");
        return Ok(ExitCode::SUCCESS);
    }
    let generated = sources
        .compile_with_kind(kind.program_kind())
        .map_err(|error| error.to_string())?;
    if mode == Mode::EmitC {
        print!("{generated}");
        return Ok(ExitCode::SUCCESS);
    }
    let protected: Vec<_> = protected
        .iter()
        .cloned()
        .chain(
            sources
                .files()
                .iter()
                .filter(|file| file.physical)
                .map(|file| file.path.clone()),
        )
        .chain(sources.native_headers().iter().cloned())
        .chain(native.sources.iter().cloned())
        .chain(native.link_inputs.iter().cloned())
        .chain(native.runtime_libraries.iter().cloned())
        .collect();
    let executable = build(&generated, output, &protected, native, kind, output_is_file)?;
    if matches!(mode, Mode::Build | Mode::BuildDependency) {
        if mode == Mode::Build {
            println!("Built {}", executable.display());
        }
        return Ok(ExitCode::SUCCESS);
    }
    return Ok(ExitCode::SUCCESS);
}

fn build(
    generated: &str,
    output: &Path,
    protected: &[PathBuf],
    native: &crustc::native::Options,
    kind: TargetKind,
    output_is_file: bool,
) -> Result<PathBuf, String> {
    let directory = output.parent().ok_or("output path needs a directory")?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let c_path = output.with_extension("c");
    let executable = if output_is_file {
        output.to_path_buf()
    } else {
        kind.artifact_path(output)?
    };
    if c_path == executable {
        return Err("generated C and native output need distinct paths".to_owned());
    }
    let import = crustc::native::import_library(&executable);
    let mut destinations = vec![c_path.clone(), executable.clone()];
    let debug_bundle =
        (cfg!(target_os = "macos") && native.debug && kind != TargetKind::StaticLibrary)
            .then(|| crustc::native::debug_bundle(&executable));
    if let Some(bundle) = &debug_bundle {
        destinations.push(bundle.clone());
    }
    if kind == TargetKind::SharedLibrary && cfg!(windows) {
        destinations.push(import);
    }
    if cfg!(windows) {
        if kind != TargetKind::StaticLibrary
            && let Some(debug) = crustc::native::debug_symbols_for(&executable, native)
        {
            destinations.push(debug);
        }
        if kind == TargetKind::Executable && !native.runtime_libraries.is_empty() {
            destinations.push(crustc::native::application_manifest(&executable));
        }
        for library in &native.runtime_libraries {
            destinations.push(
                directory.join(
                    library
                        .file_name()
                        .ok_or("runtime library needs a filename")?,
                ),
            );
        }
    }
    for destination in &destinations {
        let resolved = if destination.exists() {
            destination
                .canonicalize()
                .map_err(|error| error.to_string())?
        } else {
            directory
                .canonicalize()
                .map_err(|error| error.to_string())?
                .join(
                    destination
                        .file_name()
                        .ok_or("output path needs a filename")?,
                )
        };
        for input in protected {
            let input = input
                .canonicalize()
                .map_err(|error| format!("could not resolve input {}: {error}", input.display()))?;
            if resolved == input
                || (debug_bundle.as_ref() == Some(destination) && input.starts_with(&resolved))
            {
                return Err(format!(
                    "output would overwrite an input file: {}",
                    destination.display()
                ));
            }
        }
    }
    fs::write(&c_path, generated)
        .map_err(|error| format!("could not write {}: {error}", c_path.display()))?;
    match kind {
        TargetKind::StaticLibrary => {
            crustc::native::compile_static_library(&c_path, &executable, native)?
        }
        TargetKind::SharedLibrary => {
            crustc::native::compile_shared_library(&c_path, &executable, native)?
        }
        TargetKind::Executable => {
            crustc::native::compile_file_with_options(&c_path, &executable, native)?
        }
    }
    return Ok(executable);
}
