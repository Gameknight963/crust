use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

mod render;

#[derive(Debug, Default)]
pub struct Options {
    pub clang: Option<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub functions: Vec<String>,
    pub variables: Vec<String>,
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
        && !matches!(name, "Self" | "this" | "print" | "box_int" | "move");
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
    command.arg("-include").arg(header);
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

pub fn generate(header: &Path, options: &Options) -> Result<String, String> {
    crate::native::validate_defines(&options.defines)?;
    let header = header
        .canonicalize()
        .map_err(|error| format!("{}: {error}", header.display()))?;
    let names: Vec<_> = options
        .functions
        .iter()
        .chain(&options.variables)
        .cloned()
        .collect();
    if names.is_empty() {
        return Err("select at least one C function or variable".to_owned());
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
    let mut prefix = names[0].clone();
    for name in &names[1..] {
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
        let expected = if function { "FunctionDecl" } else { "VarDecl" };
        let label = if function { "function" } else { "variable" };
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
    let bytes = clang(
        &header,
        options,
        &probes,
        true,
        Some("__crust_import_probe_"),
    )?;
    let probes: Vec<Value> = serde_json::Deserializer::from_slice(&bytes)
        .into_iter()
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let mut renderer = render::Renderer::new(&declarations, &probes, &names);
    let mut declarations = Vec::new();
    let mut checks = Vec::new();
    let mut variable_checks = Vec::new();
    let mut locals = String::new();
    for (index, native) in names.iter().enumerate() {
        let probe = probes
            .iter()
            .find(|node| node["name"] == format!("__crust_import_probe_{index}"))
            .ok_or_else(|| format!("Clang omitted the type of '{native}'"))?;
        let node = type_children(probe)
            .next()
            .ok_or("Clang omitted a declaration type")?;
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
    let mut output = renderer.declarations(true);
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(&declarations.join("\n"));
    output.push('\n');
    let checked_source = format!(
        "{}\n{}\nint main() {{ {locals} return 0; }}\n",
        renderer.declarations(false),
        declarations.join("\n")
    );
    let program = crate::parser::parse(
        crate::lexer::tokenize(&checked_source).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let checked =
        crate::check::check(program).map_err(|error| format!("generated C bindings: {error}"))?;
    let mut verification = format!("{undefines}{}", crate::emit_c::generate(&checked));
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
    clang(&header, options, &verification, false, None)?;
    return Ok(output);
}
