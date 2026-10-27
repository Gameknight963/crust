use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crust::project::{Project, TargetKind};
mod cli_bindings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Build,
    BuildDependency,
    EmitC,
    EmitHeader,
    Check,
    Tokens,
    Metadata,
}

struct Options {
    mode: Mode,
    input: Option<PathBuf>,
    project: Option<PathBuf>,
    target: Option<String>,
    program_arguments: Vec<OsString>,
}

fn main() -> ExitCode {
    return match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    };
}

fn run() -> Result<ExitCode, String> {
    // args_os preserves source and project paths that aren't valid UTF-8.
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "import-c")
    {
        return cli_bindings::run(arguments.into_iter().skip(1).collect());
    }
    if arguments
        .iter()
        .take_while(|argument| *argument != "--")
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "Usage: crust [run|build|emit-c|emit-header|check|tokens|metadata] [file.cr|crust.toml|directory]"
        );
        println!("       crust [command] [--project <path>] [--target <name>]");
        println!(
            "       crust import-c <header.h> [--function <name>] [--variable <name>] [--enum <tag>] [--typedef <name>]"
        );
        println!("Project commands discover crust.toml in the current directory or its parents.");
        println!("A source filename compiles and runs that file.");
        println!("build/run use CC, or clang on Windows and cc elsewhere.");
        return Ok(ExitCode::SUCCESS);
    }
    let options = options(arguments)?;
    if let Some(path) = &options.project {
        return run_project(Project::load(path)?, &options);
    }
    if let Some(path) = &options.input {
        if path.is_dir()
            || path
                .extension()
                .is_some_and(|extension| extension == "toml")
        {
            return run_project(Project::load(path)?, &options);
        }
        if options.mode == Mode::Metadata {
            return Err("metadata requires a crust.toml file or project directory".to_owned());
        }
        if options.target.is_some() {
            return Err(String::from("--target requires a project"));
        }
        let output = env::current_dir()
            .map_err(|error| error.to_string())?
            .join("target")
            .join("crust")
            .join(path.file_name().ok_or("source path needs a filename")?);
        return process_source(
            path,
            options.mode,
            &output,
            &[path.to_owned()],
            &crust::native::Options::default(),
            TargetKind::Executable,
            &options.program_arguments,
        );
    }
    let directory = env::current_dir().map_err(|error| error.to_string())?;
    let path = Project::discover(&directory)?;
    return run_project(Project::load(&path)?, &options);
}

fn options(arguments: Vec<OsString>) -> Result<Options, String> {
    let mut result = Options {
        mode: Mode::Run,
        input: None,
        project: None,
        target: None,
        program_arguments: Vec::new(),
    };
    let mut arguments = arguments.into_iter().peekable();
    if let Some(mode) = arguments
        .peek()
        .and_then(|argument| argument.to_str())
        .and_then(|argument| match argument {
            "run" => Some(Mode::Run),
            "build" => Some(Mode::Build),
            "emit-c" => Some(Mode::EmitC),
            "emit-header" => Some(Mode::EmitHeader),
            "check" => Some(Mode::Check),
            "tokens" => Some(Mode::Tokens),
            "metadata" => Some(Mode::Metadata),
            _ => None,
        })
    {
        result.mode = mode;
        arguments.next();
    }
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            if result.mode != Mode::Run {
                return Err("program arguments require run".to_owned());
            }
            result.program_arguments.extend(arguments);
            break;
        } else if argument == "--project" {
            let path = arguments.next().ok_or("--project needs a path")?;
            if result.project.replace(PathBuf::from(path)).is_some() {
                return Err(String::from("--project can be specified once"));
            }
        } else if argument == "--target" {
            let name = arguments
                .next()
                .ok_or("--target needs a name")?
                .into_string()
                .map_err(|_| "target names must be UTF-8")?;
            if result.target.replace(name).is_some() {
                return Err(String::from("--target can be specified once"));
            }
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!("unknown option '{}'", argument.to_string_lossy()));
        } else if result.input.replace(PathBuf::from(&argument)).is_some() {
            return Err(format!(
                "unexpected argument '{}'",
                argument.to_string_lossy()
            ));
        }
    }
    if result.project.is_some() && result.input.is_some() {
        return Err(String::from("specify a source/project path or --project"));
    }
    return Ok(result);
}

fn run_project(project: Project, options: &Options) -> Result<ExitCode, String> {
    let single = matches!(
        options.mode,
        Mode::Run | Mode::EmitC | Mode::EmitHeader | Mode::Tokens
    );
    let targets = project.select(options.target.as_deref(), single)?;
    if options.mode == Mode::Metadata {
        let mut metadata = project.metadata()?;
        if let Some(name) = &options.target {
            metadata.targets.retain(|target| &target.name == name);
        }
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|error| format!("could not serialize project metadata: {error}"))?;
        println!("{json}");
        return Ok(ExitCode::SUCCESS);
    }
    if options.mode == Mode::Run && targets[0].kind != TargetKind::Executable {
        return Err(format!(
            "{} libraries are built with 'crust build' and linked by a consumer",
            if targets[0].kind == TargetKind::StaticLibrary {
                "static"
            } else {
                "shared"
            }
        ));
    }
    let run_target = targets.first().map(|target| target.name.clone());
    let targets = if matches!(options.mode, Mode::Build | Mode::Run | Mode::Check) {
        project.ordered_targets(&targets)?
    } else {
        targets
    };
    let mut protected: Vec<_> = project
        .targets
        .iter()
        .flat_map(|target| {
            std::iter::once(target.entry.clone())
                .chain(target.native.sources.clone())
                .chain(target.native.link_inputs.clone())
                .chain(target.native.runtime_libraries.clone())
        })
        .chain(std::iter::once(project.manifest.clone()))
        .collect();
    if matches!(options.mode, Mode::Build | Mode::Run) {
        for target in &project.targets {
            let sources =
                crust::source::SourceSet::load(&target.entry).map_err(|error| error.to_string())?;
            protected.extend(sources.files().iter().map(|file| file.path.clone()));
            protected.extend_from_slice(sources.native_headers());
        }
    }
    for target in targets {
        let output = project.output_base(target);
        let mode = if options.mode == Mode::Run && run_target.as_ref() != Some(&target.name) {
            Mode::BuildDependency
        } else {
            options.mode
        };
        let native = project.resolved_native(target)?;
        let code = process_source(
            &target.entry,
            mode,
            &output,
            &protected,
            &native,
            target.kind,
            &options.program_arguments,
        )?;
        if mode == Mode::Run {
            return Ok(code);
        }
    }
    return Ok(ExitCode::SUCCESS);
}

fn process_source(
    path: &Path,
    mode: Mode,
    output: &Path,
    protected: &[PathBuf],
    native: &crust::native::Options,
    kind: TargetKind,
    program_arguments: &[OsString],
) -> Result<ExitCode, String> {
    if mode == Mode::Run && kind != TargetKind::Executable {
        return Err(format!(
            "{} libraries are built with 'crust build' and linked by a consumer",
            if kind == TargetKind::StaticLibrary {
                "static"
            } else {
                "shared"
            }
        ));
    }
    match mode {
        Mode::Tokens => {
            let source = fs::read_to_string(path)
                .map_err(|error| format!("{}: could not read file: {error}", path.display()))?;
            let tokens = crust::lexer::tokenize(&source)
                .map_err(|error| format!("{}:{error}", path.display()))?;
            for token in tokens {
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
            let sources =
                crust::source::SourceSet::load(path).map_err(|error| error.to_string())?;
            sources
                .check_with_kind(kind.program_kind())
                .map_err(|error| error.to_string())?;
            println!("{}: OK", path.display());
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let sources = crust::source::SourceSet::load(path).map_err(|error| error.to_string())?;
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
        .chain(sources.files().iter().map(|file| file.path.clone()))
        .chain(sources.native_headers().iter().cloned())
        .chain(native.runtime_libraries.iter().cloned())
        .collect();
    let executable = build(&generated, output, &protected, native, kind)?;
    if matches!(mode, Mode::Build | Mode::BuildDependency) {
        if mode == Mode::Build {
            println!("Built {}", executable.display());
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut command = Command::new(&executable);
    command.args(program_arguments);
    if !cfg!(windows) && !native.runtime_dirs.is_empty() {
        let variable = if cfg!(windows) {
            "PATH"
        } else if cfg!(target_os = "macos") {
            "DYLD_LIBRARY_PATH"
        } else {
            "LD_LIBRARY_PATH"
        };
        let mut paths = native.runtime_dirs.clone();
        paths.extend(env::split_paths(&env::var_os(variable).unwrap_or_default()));
        command.env(
            variable,
            env::join_paths(paths).map_err(|error| error.to_string())?,
        );
    }
    let status = command
        .status()
        .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
}

fn build(
    generated: &str,
    output: &Path,
    protected: &[PathBuf],
    native: &crust::native::Options,
    kind: TargetKind,
) -> Result<PathBuf, String> {
    let directory = output.parent().ok_or("output path needs a directory")?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let c_path = output.with_extension("c");
    let executable = kind.artifact_path(output)?;
    let import = crust::native::import_library(&executable);
    let mut destinations = vec![c_path.clone(), executable.clone()];
    let debug_bundle =
        (cfg!(target_os = "macos") && native.debug && kind != TargetKind::StaticLibrary)
            .then(|| crust::native::debug_bundle(&executable));
    if let Some(bundle) = &debug_bundle {
        destinations.push(bundle.clone());
    }
    if kind == TargetKind::SharedLibrary && cfg!(windows) {
        destinations.push(import);
    }
    if cfg!(windows) {
        if kind != TargetKind::StaticLibrary
            && let Some(debug) = crust::native::debug_symbols_for(&executable, native)
        {
            destinations.push(debug);
        }
        if kind == TargetKind::Executable && !native.runtime_libraries.is_empty() {
            destinations.push(crust::native::application_manifest(&executable));
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
            let input = input.canonicalize().map_err(|error| error.to_string())?;
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
            crust::native::compile_static_library(&c_path, &executable, native)?
        }
        TargetKind::SharedLibrary => {
            crust::native::compile_shared_library(&c_path, &executable, native)?
        }
        TargetKind::Executable => {
            crust::native::compile_file_with_options(&c_path, &executable, native)?
        }
    }
    return Ok(executable);
}
