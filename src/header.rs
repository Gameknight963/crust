use crate::ast::{Function, Type};
use crate::check::CheckedProgram;
use crate::diagnostic::{Diagnostic, Span};
use crate::types::Types;
use std::collections::{HashMap, HashSet};

struct Renderer<'a> {
    types: &'a Types,
    prefix: String,
    aliases: HashMap<Type, String>,
    reserved: &'a mut HashSet<String>,
    declarations: String,
    native_enums: bool,
    next_name: usize,
}

impl Renderer<'_> {
    fn alias(&mut self) -> String {
        loop {
            let name = format!("crust_header_{}_{}", self.prefix, self.next_name);
            self.next_name += 1;
            if self.reserved.insert(name.clone()) {
                return name;
            }
        }
    }

    fn ty(&mut self, ty: Type, span: Span) -> Result<String, Diagnostic> {
        if let Some(alias) = self.aliases.get(&ty) {
            return Ok(alias.clone());
        }
        match ty {
            Type::Integer(kind) => return Ok(kind.c_name().to_owned()),
            Type::Bool => return Ok("bool".to_owned()),
            Type::Void => return Ok("void".to_owned()),
            Type::Pointer(id) => {
                let pointer = self.types.pointers[id];
                let pointee = self.ty(pointer.pointee, span)?;
                return Ok(format!(
                    "{pointee}{} *",
                    if pointer.mutable { "" } else { " const" }
                ));
            }
            Type::Struct(id) => {
                let info = &self.types.structs[id];
                if info.complete {
                    return Err(Diagnostic::new(
                        span,
                        "C header generation for structs and unions requires a native opaque tag",
                    ));
                }
                let name = info.name.clone();
                let kind = if info.is_union { "union" } else { "struct" };
                let alias = self.alias();
                self.declarations.push_str(&format!("#pragma push_macro(\"{name}\")\n#undef {name}\ntypedef {kind} {name} {alias};\n#pragma pop_macro(\"{name}\")\n"));
                self.aliases.insert(ty, alias.clone());
                return Ok(alias);
            }
            Type::Enum(id) => {
                let definition = &self.types.enums[id];
                if let Some(name) = definition.native_name.clone() {
                    self.native_enums = true;
                    let alias = self.alias();
                    self.declarations.push_str(&format!("#pragma push_macro(\"{name}\")\n#undef {name}\ntypedef enum {name} {alias};\n#pragma pop_macro(\"{name}\")\n"));
                    self.aliases.insert(ty, alias.clone());
                    return Ok(alias);
                }
                return Ok(definition.underlying.c_name().to_owned());
            }
            Type::Function(id) => {
                let signature = self.types.function_types[id].clone();
                let result = self.ty(signature.result, signature.span)?;
                let parameters =
                    self.parameters(&signature.parameters, signature.variadic, signature.span)?;
                let alias = self.alias();
                self.declarations.push_str(&format!(
                    "typedef {result} {}{alias}({parameters});\n",
                    signature.calling_convention.c_prefix()
                ));
                self.aliases.insert(ty, alias.clone());
                return Ok(alias);
            }
            Type::Array(_) => {
                return Err(Diagnostic::new(
                    span,
                    "C header generation for arrays requires a native wrapper",
                ));
            }
            Type::BoxInt | Type::Borrow { .. } => {
                return Err(Diagnostic::new(
                    span,
                    "C headers require copyable C values and raw pointers",
                ));
            }
        }
    }

    fn parameters(
        &mut self,
        types: &[Type],
        variadic: bool,
        span: Span,
    ) -> Result<String, Diagnostic> {
        if types.is_empty() {
            return Ok("void".to_owned());
        }
        let mut parameters = types
            .iter()
            .map(|ty| self.ty(*ty, span))
            .collect::<Result<Vec<_>, _>>()?;
        if variadic {
            parameters.push("...".to_owned());
        }
        return Ok(parameters.join(", "));
    }

    fn function(&mut self, function: &Function) -> Result<String, Diagnostic> {
        let result = self.ty(function.return_type, function.name_span)?;
        let parameters = self.parameters(
            &function
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect::<Vec<_>>(),
            false,
            function.name_span,
        )?;
        return Ok(format!(
            "{result} {}{}({parameters});\n",
            function.calling_convention.c_prefix(),
            function.link_name.as_ref().unwrap()
        ));
    }
}

pub(crate) fn generate(checked: &CheckedProgram) -> Result<String, Diagnostic> {
    let mut exports: Vec<_> = checked
        .program
        .functions
        .iter()
        .filter(|function| !function.external && function.link_name.is_some())
        .collect();
    exports.sort_by_key(|function| function.link_name.as_ref().unwrap());
    let mut reserved: HashSet<_> = checked
        .program
        .functions
        .iter()
        .filter_map(|function| function.link_name.clone())
        .chain(
            checked
                .program
                .globals
                .iter()
                .map(|global| global.link_name.clone()),
        )
        .chain(checked.program.enums.iter().flat_map(|definition| {
            definition
                .variants
                .iter()
                .map(|variant| variant.name.clone())
        }))
        .collect();
    let mut declarations = String::new();
    let mut native_enums = false;
    for function in exports {
        let symbol = function.link_name.as_ref().unwrap();
        let mut renderer = Renderer {
            types: &checked.types,
            prefix: symbol.clone(),
            aliases: HashMap::new(),
            reserved: &mut reserved,
            declarations: String::new(),
            native_enums: false,
            next_name: 0,
        };
        let prototype = renderer.function(function)?;
        native_enums |= renderer.native_enums;
        declarations.push_str(&format!("#pragma push_macro(\"{symbol}\")\n#undef {symbol}\n{}{prototype}#pragma pop_macro(\"{symbol}\")\n\n", renderer.declarations));
    }
    let mut output =
        "#pragma once\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n".to_owned();
    if native_enums {
        for header in &checked.program.native_headers {
            output.push_str(&format!("#include \"{header}\"\n"));
        }
    }
    output.push_str("\n#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");
    output.push_str(&declarations);
    output.push_str("#ifdef __cplusplus\n}\n#endif\n");
    return Ok(output);
}
