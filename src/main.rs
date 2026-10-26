use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Clone, Copy)]
enum Mode {
    Run,
    Build,
    EmitC,
    Check,
    Tokens,
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
    // args_os preserves Windows file paths that aren't representable as UTF-8.
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "-h") {
        println!("Usage: crust [run|build|emit-c|check|tokens] <file.cr>");
        println!("Without a command, compile and run the source file.");
        println!("build/run use CC, or clang on Windows and cc elsewhere.");
        return Ok(ExitCode::SUCCESS);
    }

    let (mode, input) = match arguments.as_slice() {
        [input] => (Mode::Run, input),
        [mode, input] => {
            let mode = match mode.to_str() {
                Some("run") => Mode::Run,
                Some("build") => Mode::Build,
                Some("emit-c") => Mode::EmitC,
                Some("check") => Mode::Check,
                Some("tokens") => Mode::Tokens,
                _ => {
                    return Err(String::from(
                        "unknown command; use 'crust --help' for usage",
                    ));
                }
            };
            (mode, input)
        }
        _ => {
            return Err(String::from(
                "Usage: crust [run|build|emit-c|check|tokens] <file.cr>",
            ));
        }
    };

    let path = Path::new(input);
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
    if matches!(mode, Mode::EmitC) {
        print!("{generated}");
        return Ok(ExitCode::SUCCESS);
    }

    let executable = build(path, &generated)?;
    if matches!(mode, Mode::Build) {
        println!("Built {}", executable.display());
        return Ok(ExitCode::SUCCESS);
    }

    let status = Command::new(&executable)
        .status()
        .map_err(|error| format!("could not run {}: {error}", executable.display()))?;
    return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
}

fn build(source: &Path, generated: &str) -> Result<PathBuf, String> {
    let directory = env::current_dir()
        .map_err(|error| error.to_string())?
        .join("target")
        .join("crust");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    let filename = source
        .file_name()
        .ok_or_else(|| String::from("source path needs a filename"))?;
    let c_path = directory.join(filename).with_extension("c");
    let executable = directory
        .join(filename)
        .with_extension(env::consts::EXE_EXTENSION);
    fs::write(&c_path, generated)
        .map_err(|error| format!("could not write {}: {error}", c_path.display()))?;

    crust::native::compile_file(&c_path, &executable)?;
    return Ok(executable);
}
