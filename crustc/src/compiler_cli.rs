use crate::invocation::Mode;
use crustc::{native, project::TargetKind};
use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

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
    let mut arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if arguments == [OsString::from("--driver-request")] {
        let encoded = serde_json::from_reader(std::io::stdin().lock())
            .map_err(|error| format!("invalid compiler request: {error}"))?;
        arguments = crate::invocation::decode(encoded)?;
    }
    if arguments.is_empty()
        || arguments
            .iter()
            .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("Usage: crustc <file.cr> [-o <output>] [--release|--debug]");
        println!("       crustc [--check|--emit-c|--emit-header|--tokens] <file.cr>");
        println!("--kind executable|static-library|shared-library selects the output kind.");
        println!("Native options: --define, --include-dir, --native-source, --library-dir,");
        println!("--library, --link-input, --runtime-library, --runtime-dir, --sanitize.");
        println!("Use crust for project discovery, build plans, and running programs.");
        return Ok(ExitCode::SUCCESS);
    }
    let mut mode = Mode::Build;
    let mut input = None;
    let mut output = None;
    let mut output_is_file = false;
    let mut kind = TargetKind::Executable;
    let mut native = native::Options::default();
    let mut protected = Vec::new();
    let mut profile = false;
    let mut arguments = arguments.into_iter().peekable();
    if let Some(action) = arguments
        .peek()
        .and_then(|a| a.to_str())
        .and_then(|a| match a {
            "build" => Some(Mode::Build),
            "check" => Some(Mode::Check),
            "tokens" => Some(Mode::Tokens),
            "emit-c" => Some(Mode::EmitC),
            "emit-header" => Some(Mode::EmitHeader),
            _ => None,
        })
    {
        mode = action;
        arguments.next();
    }
    while let Some(argument) = arguments.next() {
        let flag = argument.to_str().unwrap_or("");
        match flag {
            "--build" => mode = Mode::Build,
            "--check" => mode = Mode::Check,
            "--tokens" => mode = Mode::Tokens,
            "--emit-c" => mode = Mode::EmitC,
            "--emit-header" => mode = Mode::EmitHeader,
            "--quiet" => {
                if mode == Mode::Build {
                    mode = Mode::BuildDependency;
                }
            }
            "--debug-symbols" => native.debug = true,
            "--release" | "--debug" | "--debug-profile" => {
                if profile {
                    return Err("specify --release or --debug once".into());
                }
                profile = true;
                native.profile = if flag == "--release" {
                    native::Profile::Release
                } else {
                    native::Profile::Debug
                };
                if flag == "--debug" {
                    native.debug = true;
                }
            }
            "-o" | "--output" | "--output-base" | "--kind" | "--protect-input"
            | "--native-source" | "--include-dir" | "--library-dir" | "--link-input"
            | "--runtime-dir" | "--runtime-library" | "--define" | "--library" | "--sanitize"
            | "--link-target" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                match flag {
                    "-o" | "--output" | "--output-base" => {
                        output_is_file = flag != "--output-base";
                        if output.replace(PathBuf::from(value)).is_some() {
                            return Err("output can be specified once".into());
                        }
                    }
                    "--protect-input" => protected.push(value.into()),
                    "--native-source" => native.sources.push(value.into()),
                    "--include-dir" => native.include_dirs.push(value.into()),
                    "--library-dir" => native.library_dirs.push(value.into()),
                    "--link-input" => native.link_inputs.push(value.into()),
                    "--runtime-dir" => native.runtime_dirs.push(value.into()),
                    "--runtime-library" => native.runtime_libraries.push(value.into()),
                    _ => {
                        let value = value
                            .into_string()
                            .map_err(|_| format!("{flag} requires Unicode text"))?;
                        match flag {
                            "--kind" => {
                                kind = match value.as_str() {
                                    "executable" => TargetKind::Executable,
                                    "static-library" => TargetKind::StaticLibrary,
                                    "shared-library" => TargetKind::SharedLibrary,
                                    _ => return Err(format!("unknown output kind '{value}'")),
                                }
                            }
                            "--define" => native.defines.push(value),
                            "--library" => native.libraries.push(value),
                            "--link-target" => native.link_targets.push(value),
                            "--sanitize" => native.sanitizers.push(match value.as_str() {
                                "address" => native::Sanitizer::Address,
                                "undefined" => native::Sanitizer::Undefined,
                                _ => return Err(format!("unknown sanitizer '{value}'")),
                            }),
                            _ => unreachable!(),
                        }
                    }
                }
            }
            "run" | "metadata" | "--project" | "--target" => {
                return Err(format!("'{flag}' is a project driver command. Use crust"));
            }
            _ if argument.to_string_lossy().starts_with('-') => {
                return Err(format!("unknown option '{}'", argument.to_string_lossy()));
            }
            _ => {
                if input.replace(PathBuf::from(argument)).is_some() {
                    return Err("specify one source file".into());
                }
            }
        }
    }
    let input = input.ok_or("crustc requires a source file. Use crust for projects")?;
    if input.is_dir()
        || input
            .extension()
            .is_some_and(|extension| extension == "toml")
    {
        return Err("crustc requires a source file. Use crust for projects".into());
    }
    let output = output.unwrap_or_else(|| input.file_stem().unwrap_or_default().into());
    let output = env::current_dir()
        .map_err(|error| error.to_string())?
        .join(output);
    return crate::compiler::process_source(
        &input,
        mode,
        &output,
        &protected,
        &native,
        kind,
        output_is_file,
    );
}
