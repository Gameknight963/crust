use std::env;
use std::path::Path;
use std::process::Command;

// The caller controls artifact locations. This driver never invokes a shell.
pub fn compile_file(c_path: &Path, executable: &Path) -> Result<(), String> {
    let compiler = env::var_os("CC").unwrap_or_else(|| {
        if cfg!(windows) {
            return "clang".into();
        }
        return "cc".into();
    });
    let mut command = Command::new(&compiler);
    configure_windows_toolchain(&mut command);
    let result = command.arg("-std=c11").arg(c_path).arg("-o").arg(executable)
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

fn configure_windows_toolchain(command: &mut Command) {
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
