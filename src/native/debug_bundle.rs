use super::staging::Staging;
use super::*;
use std::fs;
use std::io::Read;

pub fn debug_bundle(output: &Path) -> PathBuf {
    let mut path = output.as_os_str().to_owned();
    path.push(".dSYM");
    return PathBuf::from(path);
}

pub(super) fn output(output: &Path, options: &Options) -> Option<PathBuf> {
    return (cfg!(target_os = "macos") && options.debug).then(|| debug_bundle(output));
}

pub(super) fn validate(bundle: &Path, binary: &Path) -> Result<(), String> {
    let file = bundle
        .join("Contents/Resources/DWARF")
        .join(binary.file_name().ok_or("debug output needs a filename")?);
    let mut signature = [0; 4];
    fs::File::open(&file)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| {
            format!(
                "C compiler did not produce a debug bundle for {}: {error}",
                binary.display()
            )
        })?;
    if !matches!(
        signature,
        [0xFE, 0xED, 0xFA, 0xCE | 0xCF]
            | [0xCE | 0xCF, 0xFA, 0xED, 0xFE]
            | [0xCA, 0xFE, 0xBA, 0xBE | 0xBF]
            | [0xBE | 0xBF, 0xBA, 0xFE, 0xCA]
    ) {
        return Err(format!("invalid native debug bundle: {}", bundle.display()));
    }
    return Ok(());
}

pub(super) fn replace(
    bundle: Option<&Path>,
    build: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let Some(bundle) = bundle else {
        return build();
    };
    let parent = bundle
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staging = Staging::create(parent)?;
    let previous = staging.file("previous.dSYM");
    let had_previous = match fs::symlink_metadata(bundle) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(format!(
                    "debug bundle output must be a directory: {}",
                    bundle.display()
                ));
            }
            fs::rename(bundle, &previous)
                .map_err(|error| format!("could not prepare {}: {error}", bundle.display()))?;
            staging.preserve(&previous);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("could not inspect {}: {error}", bundle.display())),
    };
    if let Err(error) = build() {
        let recovery = remove(bundle).and_then(|()| {
            if had_previous {
                fs::rename(&previous, bundle)
                    .map_err(|error| format!("could not restore {}: {error}", bundle.display()))?;
            }
            return Ok(());
        });
        if let Err(recovery) = recovery {
            return Err(if had_previous {
                format!(
                    "{error}\n{recovery}. Previous debug bundle kept at {}",
                    previous.display()
                )
            } else {
                format!("{error}\n{recovery}")
            });
        }
        return Err(error);
    }
    if had_previous {
        let _ = fs::remove_dir_all(&previous);
    }
    return Ok(());
}

fn remove(path: &Path) -> Result<(), String> {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "could not inspect failed debug bundle {}: {error}",
                path.display()
            ));
        }
    };
    return result.map_err(|error| {
        format!(
            "could not remove failed debug bundle {}: {error}",
            path.display()
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_debug_bundle_replacement_restores_previous_contents() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/debug-bundle-tests")
            .join(std::process::id().to_string());
        fs::create_dir_all(&root).unwrap();
        let binary = root.join("app");
        let bundle = debug_bundle(&binary);
        let contents = bundle.join("Contents/Resources/DWARF/app");
        fs::create_dir_all(contents.parent().unwrap()).unwrap();
        fs::write(&contents, b"old debug data").unwrap();
        let error = replace(Some(&bundle), || {
            fs::create_dir_all(contents.parent().unwrap()).unwrap();
            fs::write(&contents, b"invalid debug data").unwrap();
            return validate(&bundle, &binary);
        })
        .unwrap_err();
        assert!(error.contains("invalid native debug bundle"));
        assert_eq!(fs::read(&contents).unwrap(), b"old debug data");
        replace(Some(&bundle), || {
            fs::create_dir_all(contents.parent().unwrap()).unwrap();
            fs::write(&contents, b"\xcf\xfa\xed\xfe new debug data").unwrap();
            return validate(&bundle, &binary);
        })
        .unwrap();
        assert_eq!(
            fs::read(&contents).unwrap(),
            b"\xcf\xfa\xed\xfe new debug data"
        );
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("crust-build-")
        }));
    }
}
