use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crust::project::Project;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Build,
    EmitC,
    Check,
    Tokens,
}

struct Options {
    mode: Mode,
    input: Option<PathBuf>,
    project: Option<PathBuf>,
    target: Option<String>,
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
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("Usage: crust [run|build|emit-c|check|tokens] [file.cr|crust.toml|directory]");
        println!("       crust [command] [--project <path>] [--target <name>]");
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
    };
    let mut arguments = arguments.into_iter().peekable();
    if let Some(mode) = arguments
        .peek()
        .and_then(|argument| argument.to_str())
        .and_then(|argument| match argument {
            "run" => Some(Mode::Run),
            "build" => Some(Mode::Build),
            "emit-c" => Some(Mode::EmitC),
            "check" => Some(Mode::Check),
            "tokens" => Some(Mode::Tokens),
            _ => None,
        })
    {
        result.mode = mode;
        arguments.next();
    }
    while let Some(argument) = arguments.next() {
        if argument == "--project" {
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
    let single = matches!(options.mode, Mode::Run | Mode::EmitC | Mode::Tokens);
    let targets = project.select(options.target.as_deref(), single)?;
    let protected: Vec<_> = project
        .targets
        .iter()
        .flat_map(|target| {
            std::iter::once(target.entry.clone())
                .chain(target.native.sources.clone())
                .chain(target.native.link_inputs.clone())
        })
        .chain(std::iter::once(project.manifest.clone()))
        .collect();
    for target in targets {
        let output = project
            .directory
            .join("target")
            .join("crust")
            .join(&target.name)
            .join(&target.name);
        let code = process_source(
            &target.entry,
            options.mode,
            &output,
            &protected,
            &target.native,
        )?;
        if options.mode == Mode::Run {
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
) -> Result<ExitCode, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("{}: could not read file: {error}", path.display()))?;
    match mode {
        Mode::Tokens => {
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
            crust::check(&source).map_err(|error| format!("{}:{error}", path.display()))?;
            println!("{}: OK", path.display());
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let generated =
        crust::compile(&source).map_err(|error| format!("{}:{error}", path.display()))?;
    if mode == Mode::EmitC {
        print!("{generated}");
        return Ok(ExitCode::SUCCESS);
    }
    let executable = build(&generated, output, protected, native)?;
    if mode == Mode::Build {
        println!("Built {}", executable.display());
        return Ok(ExitCode::SUCCESS);
    }
    let status = Command::new(&executable)
        .status()
        .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
}

fn build(
    generated: &str,
    output: &Path,
    protected: &[PathBuf],
    native: &crust::native::Options,
) -> Result<PathBuf, String> {
    let directory = output.parent().ok_or("output path needs a directory")?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let c_path = output.with_extension("c");
    let executable = output.with_extension(env::consts::EXE_EXTENSION);
    for destination in [&c_path, &executable] {
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
            if resolved == input {
                return Err(format!(
                    "output would overwrite an input file: {}",
                    destination.display()
                ));
            }
        }
    }
    fs::write(&c_path, generated)
        .map_err(|error| format!("could not write {}: {error}", c_path.display()))?;
    crust::native::compile_file_with_options(&c_path, &executable, native)?;
    return Ok(executable);
}
