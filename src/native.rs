use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

mod archive;
mod debug_bundle;
mod executable;
#[cfg(test)]
mod runtime_tests;
mod shared;
#[cfg(test)]
mod source_mapping_tests;
mod staging;
mod tool_paths;
#[cfg(test)]
mod volatile_tests;
pub use archive::compile_static_library;
pub(crate) use archive::validate_options as validate_archive_options;
pub use debug_bundle::debug_bundle;
pub use shared::{application_manifest, compile_shared_library, import_library};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Options {
    pub sanitizers: Vec<Sanitizer>,
    pub debug: bool,
    pub defines: Vec<String>,
    pub sources: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub link_inputs: Vec<PathBuf>,
    pub link_targets: Vec<String>,
    #[serde(skip)]
    pub runtime_dirs: Vec<PathBuf>,
    pub runtime_libraries: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sanitizer {
    Address,
    Undefined,
}

impl Sanitizer {
    fn name(self) -> &'static str {
        return match self {
            Self::Address => "address",
            Self::Undefined => "undefined",
        };
    }
}

fn instrumentation(command: &mut Command, options: &Options) {
    for sanitizer in &options.sanitizers {
        command.arg(format!("-fsanitize={}", sanitizer.name()));
    }
    if !options.sanitizers.is_empty() {
        command.args(["-fno-omit-frame-pointer", "-fno-sanitize-recover=all"]);
    }
}

// The caller controls artifact locations. This driver never invokes a shell.
pub fn compile_file(c_path: &Path, executable: &Path) -> Result<(), String> {
    return compile_file_with_options(c_path, executable, &Options::default());
}

pub fn compile_file_with_options(
    c_path: &Path,
    executable: &Path,
    options: &Options,
) -> Result<(), String> {
    resolved_targets(options)?;
    validate_defines(&options.defines)?;
    let copies = shared::runtime_copies(executable, options)?;
    let debug = debug_symbols_for(executable, options);
    let bundle = debug_bundle::output(executable, options);
    if debug.is_some()
        && executable
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdb"))
    {
        return Err("an executable and its debug symbols need distinct paths".to_owned());
    }
    let manifest = (cfg!(windows) && !options.runtime_libraries.is_empty())
        .then(|| application_manifest(executable));
    let build = || {
        executable::build(executable, || {
            if let Some(manifest) = &manifest {
                shared::write_application_manifest(manifest, options)?;
            }
            invoke_compiler(c_path, executable, options, false)?;
            if let Some(debug) = &debug {
                validate_debug_symbols(debug)?;
            }
            if let Some(bundle) = &bundle {
                debug_bundle::validate(bundle, executable)?;
            }
            shared::copy_runtime_libraries(&copies)?;
            return Ok(());
        })
    };
    let mut outputs: Vec<_> = copies
        .iter()
        .map(|(_, destination)| destination.as_path())
        .collect();
    if let Some(manifest) = &manifest {
        outputs.push(manifest);
    }
    if let Some(debug) = &debug {
        outputs.push(debug);
    }
    return debug_bundle::replace(bundle.as_deref(), || {
        if outputs.is_empty() {
            return build();
        }
        return shared::replace_outputs(&outputs, build);
    });
}

pub fn debug_symbols(output: &Path) -> PathBuf {
    return output.with_extension("pdb");
}

fn windows_gnu(compiler: &std::ffi::OsStr) -> bool {
    return Command::new(compiler)
        .arg("-dumpmachine")
        .output()
        .ok()
        .is_some_and(|result| {
            let target = String::from_utf8_lossy(&result.stdout).to_ascii_lowercase();
            result.status.success() && (target.contains("mingw") || target.contains("windows-gnu"))
        });
}

pub fn debug_symbols_for(output: &Path, options: &Options) -> Option<PathBuf> {
    if !cfg!(windows) || !options.debug {
        return None;
    }
    let compiler = env::var_os("CC").unwrap_or_else(|| "clang".into());
    return (!windows_gnu(&compiler)).then(|| debug_symbols(output));
}

fn validate_debug_symbols(path: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut signature = [0; 32];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| {
            format!(
                "C compiler did not produce debug symbols for {}: {error}",
                path.display()
            )
        })?;
    if &signature != b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0" {
        return Err(format!("invalid native debug symbols: {}", path.display()));
    }
    return Ok(());
}

fn invoke_compiler(
    c_path: &Path,
    executable: &Path,
    options: &Options,
    shared: bool,
) -> Result<(), String> {
    let compiler = env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            return "clang".into();
        }
        return "cc".into();
    });
    let paths = tool_paths::ToolPaths::for_output(executable)?;
    let mut command = paths.command(&compiler);
    configure_windows_toolchain(&mut command);
    command.arg("-std=c11");
    instrumentation(&mut command, options);
    if options.debug {
        command.arg("-g");
    }
    if shared {
        command.arg("-DCRUST_BUILD_SHARED");
        if cfg!(target_os = "macos") {
            command.arg("-dynamiclib");
        } else {
            command.arg("-shared");
        }
        if cfg!(windows) {
            let gnu = windows_gnu(&compiler);
            let mut flag = std::ffi::OsString::from(if gnu { "--out-implib=" } else { "/implib:" });
            flag.push(paths.output(&import_library(executable)));
            command.arg("-Xlinker").arg(flag);
        } else {
            command.arg("-fPIC");
            let name = executable
                .file_name()
                .ok_or("shared library output needs a filename")?;
            if cfg!(target_os = "macos") {
                let mut install_name = std::ffi::OsString::from("@rpath/");
                install_name.push(name);
                command
                    .args(["-Xlinker", "-install_name", "-Xlinker"])
                    .arg(install_name);
            } else {
                command.args(["-Xlinker", "-soname", "-Xlinker"]).arg(name);
            }
        }
    }
    if !cfg!(windows) {
        for directory in &options.runtime_dirs {
            // Xlinker keeps commas and other path characters within one argv.
            command
                .args(["-Xlinker", "-rpath", "-Xlinker"])
                .arg(directory);
        }
    }
    for define in &options.defines {
        command.arg("-D").arg(define);
    }
    for directory in &options.include_dirs {
        command.arg("-I").arg(paths.input(directory));
    }
    command
        .args(["-x", "c"])
        .arg(paths.input(c_path))
        .args(options.sources.iter().map(|source| paths.input(source)))
        .args(["-x", "none"]);
    command.args(options.link_inputs.iter().map(|input| paths.input(input)));
    for directory in &options.library_dirs {
        command.arg("-L").arg(paths.input(directory));
    }
    let windows_msvc = cfg!(windows) && !options.libraries.is_empty() && !windows_gnu(&compiler);
    for library in &options.libraries {
        // Resolve configured MSVC library directories before invoking the linker.
        // Explicit inputs use the same Unicode paths as linked target archives.
        let resolved = if windows_msvc {
            options
                .library_dirs
                .iter()
                .map(|directory| directory.join(format!("{library}.lib")))
                .find(|path| path.is_file())
        } else {
            None
        };
        if let Some(path) = resolved {
            command.arg(paths.input(&path));
        } else {
            command.arg(format!("-l{library}"));
        }
    }
    let result = command.arg("-o").arg(paths.output(executable))
        .output()
        .map_err(|error| format!("could not start C compiler '{}': {error}; set CC to a Clang/GCC-compatible compiler executable", compiler.to_string_lossy()))?;
    if !result.status.success() {
        return Err(format!(
            "C compiler failed for {}:\n{}{}",
            c_path.display(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    return Ok(());
}

fn resolved_targets(options: &Options) -> Result<(), String> {
    if !options.link_targets.is_empty() {
        return Err("native link-targets must be resolved by a project build".to_owned());
    }
    return Ok(());
}

pub(crate) fn validate_defines(defines: &[String]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for definition in defines {
        let head = definition.split('=').next().unwrap();
        let name = head.split('(').next().unwrap();
        let mut bytes = name.bytes();
        if definition.contains(['\0', '\n', '\r'])
            || !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || (head.contains('(') && !head.ends_with(')'))
        {
            return Err(format!("invalid C preprocessor definition '{definition}'"));
        }
        if !names.insert(name) {
            return Err(format!("C macro '{name}' is defined more than once"));
        }
    }
    return Ok(());
}

pub(crate) fn configure_windows_toolchain(command: &mut Command) {
    #[cfg(windows)]
    {
        // Some Clang installations select an obsolete Visual Studio from the
        // registry. Prefer the active cl.exe on PATH, without changing the
        // user's environment or requiring a Developer Command Prompt.
        if env::var_os("VCToolsInstallDir").is_none() {
            let path = env::var_os("PATH").unwrap_or_default();
            for directory in env::split_paths(&path) {
                let compiler = directory.join("cl.exe");
                if !compiler.is_file() {
                    continue;
                }
                let Some(root) = compiler.ancestors().nth(4) else {
                    continue;
                };
                if root.join("include").join("vcruntime.h").is_file() {
                    command.env("VCToolsInstallDir", root);
                    break;
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = command;
}
