use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crustc::project::{Project, TargetKind};

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
    profile: Option<crustc::native::Profile>,
}

pub fn main() -> ExitCode {
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
    if arguments.is_empty()
        || arguments
            .iter()
            .take_while(|argument| *argument != "--")
            .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "Usage: {} [run|build|emit-c|emit-header|check|tokens|metadata] [file.cr|crust.toml|directory]",
            env!("CARGO_BIN_NAME")
        );
        println!(
            "       {} [command] [--project <path>] [--target <name>]",
            env!("CARGO_BIN_NAME")
        );
        println!("Project commands discover crust.toml in the current directory or its parents.");
        println!("A source filename compiles and runs that file.");
        println!("build/run use CC, or clang on Windows and cc elsewhere.");
        println!(
            "--release selects native -O2 optimization; --debug selects -O0 and debug symbols."
        );
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
            std::slice::from_ref(path),
            &native_options(options.profile),
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
        profile: None,
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
        } else if argument == "--release" || argument == "--debug" {
            let profile = if argument == "--release" {
                crustc::native::Profile::Release
            } else {
                crustc::native::Profile::Debug
            };
            if result.profile.replace(profile).is_some() {
                return Err("specify --release or --debug once".to_owned());
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

fn native_options(profile: Option<crustc::native::Profile>) -> crustc::native::Options {
    return crustc::native::Options {
        profile: profile.unwrap_or_default(),
        debug: profile == Some(crustc::native::Profile::Debug),
        ..Default::default()
    };
}

fn run_project(mut project: Project, options: &Options) -> Result<ExitCode, String> {
    if let Some(profile) = options.profile {
        for target in &mut project.targets {
            target.native.profile = profile;
            if profile == crustc::native::Profile::Debug {
                target.native.debug = true;
            }
        }
    }
    let single = matches!(
        options.mode,
        Mode::Run | Mode::EmitC | Mode::EmitHeader | Mode::Tokens
    );
    let targets = project.select(options.target.as_deref(), single)?;
    if options.mode == Mode::Metadata {
        let metadata = project.metadata_for(&targets)?;
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
        .chain(project.manifests.iter().cloned())
        .collect();
    if matches!(options.mode, Mode::Build | Mode::Run) {
        for target in &project.targets {
            let sources = crustc::source::SourceSet::load_for_target(
                &target.entry,
                &target.native,
                target.kind,
                |path| fs::read_to_string(path),
            )
            .map_err(|error| error.to_string())?;
            protected.extend(
                sources
                    .files()
                    .iter()
                    .filter(|file| file.physical)
                    .map(|file| file.path.clone()),
            );
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
    native: &crustc::native::Options,
    kind: TargetKind,
    program_arguments: &[OsString],
) -> Result<ExitCode, String> {
    use std::io::Write;
    use std::process::Stdio;
    let compiler = if let Some(path) = env::var_os("CRUSTC") {
        PathBuf::from(path)
    } else {
        let current = env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| error.to_string())?;
        current
            .parent()
            .ok_or("driver executable needs a directory")?
            .join(if cfg!(windows) {
                "crustc.exe"
            } else {
                "crustc"
            })
    };
    let mut arguments = vec![
        OsString::from(match mode {
            Mode::Check => "--check",
            Mode::Tokens => "--tokens",
            Mode::EmitC => "--emit-c",
            Mode::EmitHeader => "--emit-header",
            _ => "--build",
        }),
        path.as_os_str().to_owned(),
        OsString::from("--output-base"),
        output.as_os_str().to_owned(),
        OsString::from("--kind"),
        OsString::from(match kind {
            TargetKind::Executable => "executable",
            TargetKind::StaticLibrary => "static-library",
            TargetKind::SharedLibrary => "shared-library",
        }),
    ];
    arguments.push(OsString::from(
        if native.profile == crustc::native::Profile::Release {
            "--release"
        } else {
            "--debug-profile"
        },
    ));
    if native.debug {
        arguments.push(OsString::from("--debug-symbols"));
    }
    if mode == Mode::Run || mode == Mode::BuildDependency {
        arguments.push(OsString::from("--quiet"));
    }
    for (flag, paths) in [
        ("--protect-input", protected),
        ("--native-source", native.sources.as_slice()),
        ("--include-dir", native.include_dirs.as_slice()),
        ("--library-dir", native.library_dirs.as_slice()),
        ("--link-input", native.link_inputs.as_slice()),
        ("--runtime-dir", native.runtime_dirs.as_slice()),
        ("--runtime-library", native.runtime_libraries.as_slice()),
    ] {
        for path in paths {
            arguments.push(flag.into());
            arguments.push(path.as_os_str().to_owned());
        }
    }
    for (flag, values) in [
        ("--define", &native.defines),
        ("--library", &native.libraries),
        ("--link-target", &native.link_targets),
    ] {
        for value in values {
            arguments.push(flag.into());
            arguments.push(value.into());
        }
    }
    for sanitizer in &native.sanitizers {
        arguments.push("--sanitize".into());
        arguments.push(
            match sanitizer {
                crustc::native::Sanitizer::Address => "address",
                crustc::native::Sanitizer::Undefined => "undefined",
            }
            .into(),
        );
    }
    let request = serde_json::to_vec(&crate::invocation::encode(&arguments))
        .map_err(|error| error.to_string())?;
    let mut child = Command::new(&compiler)
        .arg("--driver-request")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start compiler {}: {error}. Set CRUSTC to the compiler executable",
                compiler.display()
            )
        })?;
    let written = child
        .stdin
        .take()
        .ok_or("compiler stdin was not piped")?
        .write_all(&request);
    let status = child
        .wait()
        .map_err(|error| format!("could not wait for compiler: {error}"))?;
    if !status.success() {
        return Err(format!("compiler exited with {status}"));
    }
    written.map_err(|error| format!("could not send compiler arguments: {error}"))?;
    if mode != Mode::Run {
        return Ok(ExitCode::SUCCESS);
    }
    let executable = kind.artifact_path(output)?;
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
