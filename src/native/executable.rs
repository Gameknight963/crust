use super::staging::Staging;
use super::*;
use std::fs;
use std::io::Read;

pub(super) fn build(
    output: &Path,
    compile: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staging = Staging::create(parent)?;
    let previous = staging.file("previous");
    let had_previous = match fs::symlink_metadata(output) {
        Ok(metadata) => {
            if !metadata.is_file() && !metadata.file_type().is_symlink() {
                return Err(format!(
                    "executable output must be a file: {}",
                    output.display()
                ));
            }
            fs::rename(output, &previous)
                .map_err(|error| format!("could not prepare {}: {error}", output.display()))?;
            staging.preserve(&previous);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("could not inspect {}: {error}", output.display())),
    };
    let result = compile().and_then(|()| validate(output));
    if let Err(error) = result {
        let recovery = restore(output, had_previous.then_some(previous.as_path()));
        if let Err(recovery) = recovery {
            return Err(if had_previous {
                format!(
                    "{error}\n{recovery}. Previous executable kept at {}",
                    previous.display()
                )
            } else {
                format!("{error}\n{recovery}")
            });
        }
        return Err(error);
    }
    if had_previous {
        // Discard the backup only after validating the new executable.
        staging.file("previous");
    }
    return Ok(());
}

fn restore(output: &Path, previous: Option<&Path>) -> Result<(), String> {
    match fs::remove_file(output) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not remove failed output {}: {error}",
                output.display()
            ));
        }
    }
    if let Some(previous) = previous {
        fs::rename(previous, output)
            .map_err(|error| format!("could not restore {}: {error}", output.display()))?;
    }
    return Ok(());
}

fn validate(output: &Path) -> Result<(), String> {
    let invalid = |error| {
        format!(
            "C compiler did not produce an executable for {}: {error}",
            output.display()
        )
    };
    let mut signature = [0; 4];
    fs::File::open(output)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(invalid)?;
    let valid = if cfg!(windows) {
        signature.starts_with(b"MZ")
    } else if cfg!(target_os = "macos") {
        matches!(
            signature,
            [0xFE, 0xED, 0xFA, 0xCE | 0xCF]
                | [0xCE | 0xCF, 0xFA, 0xED, 0xFE]
                | [0xCA, 0xFE, 0xBA, 0xBE | 0xBF]
                | [0xBE | 0xBF, 0xBA, 0xFE, 0xCA]
        )
    } else {
        signature == *b"\x7FELF"
    };
    if !valid {
        return Err(format!(
            "C compiler did not produce an executable for {}: invalid native file signature",
            output.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(output).map_err(invalid)?.permissions();
        if permissions.mode() & 0o111 == 0 {
            return Err(format!(
                "C compiler output lacks execute permission: {}",
                output.display()
            ));
        }
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_restore_keeps_the_backup_and_reports_its_location() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/executable-recovery");
        fs::create_dir_all(&root).unwrap();
        let mut fixture = Staging::create(&root).unwrap();
        let output = fixture.file("program");
        fs::write(&output, "previous executable").unwrap();
        let error = build(&output, || {
            fs::create_dir(&output).unwrap();
            return Err("simulated linker failure".to_owned());
        })
        .unwrap_err();
        let backup = PathBuf::from(error.split("Previous executable kept at ").nth(1).unwrap());
        assert!(backup.starts_with(output.parent().unwrap()));
        assert_eq!(fs::read(&backup).unwrap(), b"previous executable");
        // Recover the file using the path reported to the caller.
        fs::remove_dir(&output).unwrap();
        fs::rename(&backup, &output).unwrap();
        fs::remove_dir(backup.parent().unwrap()).unwrap();
    }
}
