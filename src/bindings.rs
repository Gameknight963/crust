use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

mod enums;
mod render;

#[derive(Debug, Default)]
pub struct Options {
    pub binding_dir: Option<PathBuf>,
    pub clang: Option<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub functions: Vec<String>,
    pub variables: Vec<String>,
    pub enums: Vec<String>,
    pub typedefs: Vec<String>,
}

fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    return bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
}

fn crust_identifier(name: &str) -> bool {
    let Ok(tokens) = crate::lexer::tokenize(name) else {
        return false;
    };
    return tokens.len() == 2
        && matches!(&tokens[0].kind, crate::lexer::Token::Identifier(value) if value == name)
        && crate::ast::IntType::named(name).is_none()
        && !matches!(
            name,
            "Self" | "this" | "print" | "box_int" | "move" | "slice"
        );
}

fn children(node: &Value) -> &[Value] {
    return node["inner"].as_array().map(Vec::as_slice).unwrap_or(&[]);
}

fn kind(node: &Value) -> &str {
    return node["kind"].as_str().unwrap_or("");
}

fn type_children(node: &Value) -> impl DoubleEndedIterator<Item = &Value> {
    return children(node)
        .iter()
        .filter(|node| kind(node).ends_with("Type"));
}

fn clang(
    header: &Path,
    options: &Options,
    source: &str,
    ast: bool,
    filter: Option<&str>,
) -> Result<Vec<u8>, String> {
    return invoke_clang(header, options, source, ast, filter, true);
}

fn invoke_clang(
    header: &Path,
    options: &Options,
    source: &str,
    ast: bool,
    filter: Option<&str>,
    include: bool,
) -> Result<Vec<u8>, String> {
    let executable = options
        .clang
        .clone()
        .unwrap_or_else(|| PathBuf::from("clang"));
    let mut command = Command::new(&executable);
    crate::native::configure_windows_toolchain(&mut command);
    command.args([
        "-std=c11",
        "-fsyntax-only",
        "-Werror=incompatible-pointer-types",
        "-Werror=ignored-attributes",
        "-Werror=unknown-attributes",
    ]);
    for directory in &options.include_dirs {
        command.arg("-I").arg(directory);
    }
    for define in &options.defines {
        command.arg("-D").arg(define);
    }
    if include {
        command.arg("-include").arg(header);
    }
    if ast {
        command.args(["-Xclang", "-ast-dump=json"]);
    }
    if let Some(filter) = filter {
        command.args(["-Xclang", "-ast-dump-filter", "-Xclang", filter]);
    }
    command
        .args(["-x", "c", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start Clang '{}': {error}", executable.display()))?;
    let mut input = child.stdin.take().unwrap();
    let (output, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(source.as_bytes()));
        let output = child.wait_with_output();
        return (output, writer.join().unwrap());
    });
    let output = output.map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "C header analysis failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    written.map_err(|error| error.to_string())?;
    return Ok(output.stdout);
}

fn header_reference(header: &Path, directory: Option<&Path>) -> Result<String, String> {
    if let Some(directory) = directory {
        let base = crate::source::identity(directory).map_err(|error| error.to_string())?;
        let target: Vec<_> = header.components().collect();
        let base: Vec<_> = base.components().collect();
        let common = target.iter().zip(&base).take_while(|(a, b)| a == b).count();
        if common > 0
            && !target[common..].iter().chain(&base[common..]).any(|part| {
                matches!(
                    part,
                    std::path::Component::Prefix(_) | std::path::Component::RootDir
                )
            })
        {
            let mut relative = PathBuf::new();
            for _ in &base[common..] {
                relative.push("..");
            }
            for part in &target[common..] {
                relative.push(part.as_os_str());
            }
            return crate::source::header_name(&relative).map_err(|error| error.to_string());
        }
    }
    return crate::source::header_name(header).map_err(|error| error.to_string());
}

pub fn generate(header: &Path, options: &Options) -> Result<String, String> {
    crate::native::validate_defines(&options.defines)?;
    let header = header
        .canonicalize()
        .map_err(|error| format!("{}: {error}", header.display()))?;
    let names: Vec<_> = options
        .functions
        .iter()
        .chain(&options.variables)
        .chain(&options.typedefs)
        .cloned()
        .collect();
    if names.is_empty() && options.enums.is_empty() {
        return Err("select at least one C function, variable, enum, or typedef".to_owned());
    }
    let mut selected = HashSet::new();
    for name in &names {
        if !identifier(name) || name == "main" || name.starts_with("__builtin_") {
            return Err(format!("invalid C declaration selection '{name}'"));
        }
        if !selected.insert(name) {
            return Err(format!("C declaration '{name}' was selected twice"));
        }
    }
    let mut selected_enums = HashSet::new();
    for name in &options.enums {
        if !identifier(name) {
            return Err(format!("invalid C enum selection '{name}'"));
        }
        if !selected_enums.insert(name) {
            return Err(format!("C enum '{name}' was selected twice"));
        }
    }
    let mut prefix = names.first().cloned().unwrap_or_default();
    for name in names.iter().skip(1) {
        while !name.starts_with(&prefix) {
            prefix.pop();
        }
    }
    let filters = if prefix.len() >= 3 {
        vec![prefix]
    } else {
        names.clone()
    };
    let mut declarations = Vec::new();
    for filter in filters {
        let bytes = clang(&header, options, "", true, Some(&filter))?;
        declarations.extend(
            serde_json::Deserializer::from_slice(&bytes)
                .into_iter::<Value>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?,
        );
    }
    let mut imported = HashSet::new();
    for (index, name) in names.iter().enumerate() {
        let function = index < options.functions.len();
        let alias = index >= options.functions.len() + options.variables.len();
        let expected = if function {
            "FunctionDecl"
        } else if alias {
            "TypedefDecl"
        } else {
            "VarDecl"
        };
        let label = if function {
            "function"
        } else if alias {
            "typedef"
        } else {
            "variable"
        };
        let matches: Vec<_> = declarations
            .iter()
            .filter(|node| kind(node) == expected && node["name"] == *name)
            .collect();
        if matches.is_empty() {
            return Err(format!(
                "C header has no {label} declaration named '{name}'"
            ));
        }
        for declaration in matches {
            if children(declaration)
                .iter()
                .any(|node| kind(node) == "DLLImportAttr")
            {
                imported.insert(name.clone());
            }
            if !function
                && children(declaration)
                    .iter()
                    .any(|node| kind(node) == "DLLExportAttr")
            {
                return Err(format!(
                    "C variable '{name}' is declared for DLL export. Use the header's consumer configuration"
                ));
            }
            if declaration["storageClass"] == "static" {
                return Err(format!(
                    "C {label} '{name}' has internal linkage. Expose it through a native C wrapper"
                ));
            }
            if children(declaration)
                .iter()
                .any(|node| kind(node) == "AsmLabelAttr")
            {
                return Err(format!("C {label} '{name}' uses an assembler symbol label"));
            }
            if !function && !declaration["tls"].is_null() {
                return Err(format!(
                    "C variable '{name}' requires storage attributes. Expose it through a native C wrapper"
                ));
            }
        }
    }
    let undefines = names
        .iter()
        .map(|name| format!("#undef {name}\n"))
        .collect::<String>();
    let mut probes = undefines.clone();
    for (index, name) in names.iter().enumerate() {
        probes.push_str(&format!(
            "typedef __typeof__({name}) __crust_import_probe_{index};\n"
        ));
    }
    let bytes = if names.is_empty() {
        Vec::new()
    } else {
        clang(
            &header,
            options,
            &probes,
            true,
            Some("__crust_import_probe_"),
        )?
    };
    let probes: Vec<Value> = serde_json::Deserializer::from_slice(&bytes)
        .into_iter()
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let mut renderer = render::Renderer::new(&declarations, &probes, &names);
    let enums = enums::load(&header, options, &probes)?;
    let has_native_types = !enums.is_empty();
    renderer.enums(enums)?;
    let mut declarations = Vec::new();
    let mut checks = Vec::new();
    let mut variable_checks = Vec::new();
    let mut typedef_checks = Vec::new();
    let mut typedef_globals = Vec::new();
    let mut locals = String::new();
    for (index, native) in names.iter().enumerate() {
        let probe = probes
            .iter()
            .find(|node| node["name"] == format!("__crust_import_probe_{index}"))
            .ok_or_else(|| format!("Clang omitted the type of '{native}'"))?;
        let node = type_children(probe)
            .next()
            .ok_or("Clang omitted a declaration type")?;
        if index >= options.functions.len() + options.variables.len() {
            let name = renderer.typedef_name(native);
            let (ty, sized) = renderer
                .alias_type(node)
                .map_err(|error| format!("C typedef '{native}': {error}"))?;
            declarations.push(format!("typedef {ty} {name};"));
            let check_name = renderer.check_name(index);
            typedef_globals.push(format!("extern {name}* {check_name};"));
            typedef_checks.push((check_name, native, sized));
            continue;
        }
        let name = renderer.binding_name(native);
        let link = if name == *native {
            String::new()
        } else {
            format!(" = \"{native}\"")
        };
        let attribute = if imported.contains(native) {
            "__declspec(dllimport) "
        } else {
            ""
        };
        if index >= options.functions.len() {
            let ty = renderer
                .object_type(node)
                .map_err(|error| format!("C variable '{native}': {error}"))?;
            declarations.push(format!("extern {attribute}{ty} {name}{link};"));
            variable_checks.push((name, native));
            continue;
        }
        let signature = renderer
            .signature(node)
            .map_err(|error| format!("C function '{native}': {error}"))?;
        declarations.push(format!(
            "extern {attribute}{} {}{}({}){link};",
            signature.result, signature.convention, name, signature.parameters
        ));
        let check_name = renderer.check_name(index);
        locals.push_str(&format!("{}* {check_name} = {name};\n", signature.alias));
        checks.push((name, native));
    }
    let mut output = if has_native_types {
        format!(
            "extern header {};\n",
            serde_json::to_string(&header_reference(&header, options.binding_dir.as_deref())?)
                .map_err(|error| error.to_string())?
        )
    } else {
        String::new()
    };
    output.push_str(&renderer.declarations(true));
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(&declarations.join("\n"));
    output.push('\n');
    let header_declaration = if has_native_types {
        format!(
            "extern header {};\n",
            serde_json::to_string(
                &crate::source::header_name(&header).map_err(|error| error.to_string())?
            )
            .map_err(|error| error.to_string())?
        )
    } else {
        String::new()
    };
    let checked_source = format!(
        "{header_declaration}{}\n{}\n{}\nint main() {{ {locals} return 0; }}\n",
        renderer.declarations(false),
        declarations.join("\n"),
        typedef_globals.join("\n")
    );
    let program = crate::parser::parse(
        crate::lexer::tokenize(&checked_source).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let checked =
        crate::check::check(program).map_err(|error| format!("generated C bindings: {error}"))?;
    let mut verification = format!("{undefines}{}", crate::emit_c::generate(&checked));
    // Native enum emission includes the header again. Its macros must not
    // replace selected typedef names in the checks appended below.
    verification.push_str(&undefines);
    let analysis = &checked.analyses["main"];
    for (name, native) in checks {
        let (span, _) = analysis
            .function_values
            .iter()
            .find(|(_, binding)| **binding == name)
            .ok_or("missing binding validation type")?;
        let crate::ast::Type::Pointer(pointer) = analysis.expression_types[span] else {
            return Err("binding validation requires a function pointer".to_owned());
        };
        let crate::ast::Type::Function(function) = checked.types.pointers[pointer].pointee else {
            return Err("binding validation requires a function type".to_owned());
        };
        verification.push_str(&format!("_Static_assert(__builtin_types_compatible_p(__typeof__({native}), crust_function_{function}), \"unsupported C ABI for {native}\");\n"));
    }
    for (name, native) in variable_checks {
        let global = checked
            .program
            .globals
            .iter()
            .find(|global| global.name == name)
            .ok_or("missing variable validation type")?;
        let ty = crate::emit_c::c_type(global.ty);
        verification.push_str(&format!("_Static_assert(__builtin_types_compatible_p(__typeof__({native}), {ty}), \"unsupported C ABI for {native}\");\n"));
    }
    for (name, native, sized) in typedef_checks {
        let global = checked
            .program
            .globals
            .iter()
            .find(|global| global.name == name)
            .ok_or("missing typedef validation type")?;
        let ty = crate::emit_c::c_type(global.ty);
        verification.push_str(&format!("_Static_assert(__builtin_types_compatible_p({native}*, {ty}), \"unsupported C ABI for typedef {native}\");\n"));
        if sized {
            let crate::ast::Type::Pointer(pointer) = global.ty else {
                unreachable!();
            };
            let inner = crate::emit_c::c_type(checked.types.pointers[pointer].pointee);
            verification.push_str(&format!("_Static_assert(sizeof({native}) == sizeof({inner}) && _Alignof({native}) == _Alignof({inner}), \"unsupported C typedef layout for {native}\");\n"));
        }
    }
    invoke_clang(
        &header,
        options,
        &verification,
        false,
        None,
        !has_native_types,
    )?;
    return Ok(output);
}
