use super::staging::Staging;
use super::*;
use std::fs;
use std::io::Read;

pub fn import_library(output: &Path) -> PathBuf {
    return output.with_extension("lib");
}

pub fn application_manifest(output: &Path) -> PathBuf {
    let mut path = output.as_os_str().to_owned();
    path.push(".manifest");
    return PathBuf::from(path);
}

pub(super) fn write_application_manifest(path: &Path, options: &Options) -> Result<(), String> {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<assembly xmlns=\"urn:schemas-microsoft-com:asm.v1\" manifestVersion=\"1.0\">\n<assemblyIdentity name=\"Crust.application\" version=\"1.0.0.0\" type=\"win32\"/>\n",
    );
    let mut names = std::collections::HashSet::new();
    for library in &options.runtime_libraries {
        let name = library
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("runtime library filename requires UTF-8 for its Windows manifest")?;
        if !names.insert(name.to_ascii_lowercase()) {
            continue;
        }
        let escaped = name
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");
        text.push_str(&format!("<file name=\"{escaped}\"/>\n"));
    }
    text.push_str("</assembly>\n");
    fs::write(path, text).map_err(|error| {
        format!(
            "could not write application manifest {}: {error}",
            path.display()
        )
    })?;
    return Ok(());
}

pub fn compile_shared_library(
    c_path: &Path,
    output: &Path,
    options: &Options,
) -> Result<(), String> {
    resolved_targets(options)?;
    validate_defines(&options.defines)?;
    let import = import_library(output);
    let debug = debug_symbols_for(output, options);
    let bundle = debug_bundle::output(output, options);
    let copies = runtime_copies(output, options)?;
    let mut outputs = vec![output];
    if cfg!(windows) {
        if output
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lib"))
        {
            return Err("a shared library and its import library need distinct paths".to_owned());
        }
        outputs.push(&import);
    }
    outputs.extend(copies.iter().map(|(_, destination)| destination.as_path()));
    if let Some(debug) = &debug {
        outputs.push(debug);
    }
    return debug_bundle::replace(bundle.as_deref(), || {
        replace_outputs(&outputs, || {
            invoke_compiler(c_path, output, options, true)?;
            validate_native(output)?;
            if let Some(debug) = &debug {
                validate_debug_symbols(debug)?;
            }
            if let Some(bundle) = &bundle {
                debug_bundle::validate(bundle, output)?;
            }
            if cfg!(windows) {
                let mut signature = [0; 8];
                fs::File::open(&import)
                    .and_then(|mut file| file.read_exact(&mut signature))
                    .map_err(|error| {
                        format!(
                            "C compiler did not produce an import library for {}: {error}",
                            output.display()
                        )
                    })?;
                if &signature != b"!<arch>\n" {
                    return Err(format!("invalid import library: {}", import.display()));
                }
            }
            copy_runtime_libraries(&copies)?;
            return Ok(());
        })
    });
}

fn validate_native(output: &Path) -> Result<(), String> {
    let mut signature = [0; 4];
    fs::File::open(output)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| {
            format!(
                "C compiler did not produce a shared library for {}: {error}",
                output.display()
            )
        })?;
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
        return Err(format!("invalid shared library: {}", output.display()));
    }
    return Ok(());
}

// DLL and import-library backups live together. A compiler failure restores both
// before returning, and failed recovery keeps each surviving backup on disk.
pub(super) fn replace_outputs(
    outputs: &[&Path],
    build: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut identities = std::collections::HashSet::new();
    for output in outputs {
        let identity = crate::source::identity(output).map_err(|error| error.to_string())?;
        let identity = if cfg!(windows) {
            identity.to_string_lossy().to_lowercase()
        } else {
            identity.to_string_lossy().into_owned()
        };
        if !identities.insert(identity) {
            return Err("native outputs must have distinct paths".to_owned());
        }
    }
    let parent = outputs[0]
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staging = Staging::create(parent)?;
    let mut previous = Vec::new();
    let result = (|| {
        for (index, output) in outputs.iter().enumerate() {
            let backup = staging.file(&format!("previous_{index}"));
            let exists = match fs::symlink_metadata(output) {
                Ok(metadata) => {
                    if !metadata.is_file() && !metadata.file_type().is_symlink() {
                        return Err(format!(
                            "native output must be a file: {}",
                            output.display()
                        ));
                    }
                    fs::rename(output, &backup).map_err(|error| {
                        format!("could not prepare {}: {error}", output.display())
                    })?;
                    staging.preserve(&backup);
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(format!("could not inspect {}: {error}", output.display()));
                }
            };
            previous.push((output, backup, exists));
        }
        return build();
    })();
    if let Err(mut error) = result {
        for (output, backup, exists) in previous.iter().rev() {
            let recovered = (|| {
                match fs::remove_file(output) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                if *exists {
                    fs::rename(backup, output)?;
                }
                return Ok(());
            })();
            if let Err(recovery) = recovered {
                error.push_str(&format!(
                    "\ncould not recover {}: {recovery}",
                    output.display()
                ));
                if *exists {
                    error.push_str(&format!(". Previous output kept at {}", backup.display()));
                }
            }
        }
        return Err(error);
    }
    for (index, (_, _, exists)) in previous.iter().enumerate() {
        if *exists {
            staging.file(&format!("previous_{index}"));
        }
    }
    return Ok(());
}

pub(super) fn runtime_copies(
    output: &Path,
    options: &Options,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if !cfg!(windows) || options.runtime_libraries.is_empty() {
        return Ok(Vec::new());
    }
    let directory = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut sources = std::collections::HashMap::new();
    let mut copies = Vec::new();
    for source in &options.runtime_libraries {
        let name = source
            .file_name()
            .ok_or("runtime library needs a filename")?;
        let destination = directory.join(name);
        let source = source.canonicalize().map_err(|error| {
            format!(
                "could not read runtime library {}: {error}",
                source.display()
            )
        })?;
        if destination.canonicalize().ok().as_ref() == Some(&source) {
            continue;
        }
        let key = name.to_string_lossy().to_ascii_lowercase();
        if let Some(previous) = sources.insert(key, source.clone()) {
            if previous == source {
                continue;
            }
            return Err(format!(
                "runtime libraries have the same filename: {} and {}",
                previous.display(),
                source.display()
            ));
        }
        copies.push((source, destination));
    }
    return Ok(copies);
}

pub(super) fn copy_runtime_libraries(copies: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (source, destination) in copies {
        fs::copy(source, destination).map_err(|error| {
            format!(
                "could not deploy runtime library {}: {error}",
                destination.display()
            )
        })?;
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Staging {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/shared-recovery");
        fs::create_dir_all(&root).unwrap();
        return Staging::create(&root).unwrap();
    }

    #[test]
    fn failure_preparing_an_import_library_restores_the_shared_library() {
        let mut fixture = fixture();
        let library = fixture.file("program.dll");
        let import = fixture.file("program.lib");
        fs::write(&library, "last working DLL").unwrap();
        fs::create_dir(&import).unwrap();
        let error =
            replace_outputs(&[&library, &import], || panic!("preparation must fail")).unwrap_err();
        assert!(error.contains("must be a file"));
        assert_eq!(fs::read(&library).unwrap(), b"last working DLL");
        assert!(import.is_dir());
        fs::remove_dir(&import).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn runtime_library_names_cannot_replace_the_primary_shared_output() {
        let mut output_stage = fixture();
        let mut dependency = fixture();
        let library = output_stage.file("program.dll");
        let source = dependency.file("program.dll");
        fs::write(&library, "last working DLL").unwrap();
        fs::write(&source, "dependency DLL").unwrap();
        let error = compile_shared_library(
            Path::new("missing.c"),
            &library,
            &Options {
                runtime_libraries: vec![source],
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("distinct paths"));
        assert_eq!(fs::read(library).unwrap(), b"last working DLL");
    }

    #[test]
    fn recovery_failure_preserves_the_backup_while_recovering_other_outputs() {
        let mut fixture = fixture();
        let library = fixture.file("program.dll");
        let import = fixture.file("program.lib");
        fs::write(&library, "last working DLL").unwrap();
        fs::write(&import, "last working import library").unwrap();
        let error = replace_outputs(&[&library, &import], || {
            fs::create_dir(&library).unwrap();
            fs::write(&import, "partial import").unwrap();
            return Err("simulated linker failure".to_owned());
        })
        .unwrap_err();
        assert_eq!(fs::read(&import).unwrap(), b"last working import library");
        let backup = PathBuf::from(error.split("Previous output kept at ").nth(1).unwrap());
        assert!(backup.starts_with(library.parent().unwrap()));
        assert_eq!(fs::read(&backup).unwrap(), b"last working DLL");
        fs::remove_dir(&library).unwrap();
        fs::rename(&backup, &library).unwrap();
        fs::remove_dir(backup.parent().unwrap()).unwrap();
    }
}
