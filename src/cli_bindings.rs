use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

pub(super) fn run(arguments: Vec<OsString>) -> Result<ExitCode, String> {
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "Usage: crust import-c <header.h> [--function <name>] [--variable <name>] [--enum <tag>] [--typedef <name>] [--macro <name>] [options]"
        );
        println!("  --function <name>     Select a function, repeat for additional functions");
        println!(
            "  --variable <name>     Select an external variable, repeat for additional variables"
        );
        println!("  --enum <tag>          Select a named enum, repeat for additional enums");
        println!("  --typedef <name>      Select a typedef, repeat for additional aliases");
        println!("  --macro <name>        Select an integer or boolean macro constant");
        println!("  --output <file.cr>    Write UTF-8 bindings to a file, otherwise use stdout");
        println!("  --force               Replace an existing output file");
        println!("  --include-dir <path>  Add a C include directory");
        println!("  --define <name=value> Define a C preprocessor macro");
        println!("  --clang <path>        Select the Clang executable");
        return Ok(ExitCode::SUCCESS);
    }
    let mut options = crust::bindings::Options::default();
    let mut header = None;
    let mut output = None;
    let mut force = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--function") => options.functions.push(
                arguments
                    .next()
                    .ok_or("--function needs a name")?
                    .into_string()
                    .map_err(|_| "function names must be ASCII")?,
            ),
            Some("--variable") => options.variables.push(
                arguments
                    .next()
                    .ok_or("--variable needs a name")?
                    .into_string()
                    .map_err(|_| "variable names must be ASCII")?,
            ),
            Some("--enum") => options.enums.push(
                arguments
                    .next()
                    .ok_or("--enum needs a tag name")?
                    .into_string()
                    .map_err(|_| "enum tags must be ASCII")?,
            ),
            Some("--typedef") => options.typedefs.push(
                arguments
                    .next()
                    .ok_or("--typedef needs a name")?
                    .into_string()
                    .map_err(|_| "typedef names must be ASCII")?,
            ),
            Some("--macro") => options.macros.push(
                arguments
                    .next()
                    .ok_or("--macro needs a name")?
                    .into_string()
                    .map_err(|_| "macro names must be ASCII")?,
            ),
            Some("--output") => {
                let path = PathBuf::from(arguments.next().ok_or("--output needs a path")?);
                if output.replace(path).is_some() {
                    return Err("--output can be specified once".to_owned());
                }
            }
            Some("--clang") => {
                let path = PathBuf::from(arguments.next().ok_or("--clang needs a path")?);
                if options.clang.replace(path).is_some() {
                    return Err("--clang can be specified once".to_owned());
                }
            }
            Some("--include-dir") => options.include_dirs.push(PathBuf::from(
                arguments.next().ok_or("--include-dir needs a path")?,
            )),
            Some("--define") => options.defines.push(
                arguments
                    .next()
                    .ok_or("--define needs a macro definition")?
                    .into_string()
                    .map_err(|_| "macro definitions must be UTF-8")?,
            ),
            Some("--force") => force = true,
            Some(flag) if flag.starts_with('-') => {
                return Err(format!("unknown import-c option '{flag}'"));
            }
            _ => {
                if header.replace(PathBuf::from(argument)).is_some() {
                    return Err("import-c accepts one header path".to_owned());
                }
            }
        }
    }
    let header = header.ok_or("import-c needs a header path")?;
    if let Some(output) = &output {
        if crust::source::identity(&header).map_err(|error| error.to_string())?
            == crust::source::identity(output).map_err(|error| error.to_string())?
        {
            return Err("the binding output would overwrite its C header".to_owned());
        }
        if output.exists() && !force {
            return Err(format!(
                "{} already exists. Use --force to replace it",
                output.display()
            ));
        }
    }
    options.binding_dir = output.as_ref().map(|path| {
        return path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."))
            .to_owned();
    });
    let bindings = crust::bindings::generate(&header, &options)?;
    if let Some(output) = output {
        std::fs::write(&output, bindings)
            .map_err(|error| format!("{}: {error}", output.display()))?;
    } else {
        std::io::stdout()
            .write_all(bindings.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    return Ok(ExitCode::SUCCESS);
}
