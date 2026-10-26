use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Options {
    pub defines: Vec<String>,
    pub sources: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub link_inputs: Vec<PathBuf>,
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
    validate_defines(&options.defines)?;
    let compiler = env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            return "clang".into();
        }
        return "cc".into();
    });
    let mut command = Command::new(&compiler);
    configure_windows_toolchain(&mut command);
    command.arg("-std=c11");
    for define in &options.defines {
        command.arg("-D").arg(define);
    }
    for directory in &options.include_dirs {
        command.arg("-I").arg(directory);
    }
    command
        .args(["-x", "c"])
        .arg(c_path)
        .args(&options.sources)
        .args(["-x", "none"]);
    command.args(&options.link_inputs);
    for directory in &options.library_dirs {
        command.arg("-L").arg(directory);
    }
    for library in &options.libraries {
        command.arg(format!("-l{library}"));
    }
    let result = command.arg("-o").arg(executable)
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
