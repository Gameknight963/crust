use super::*;

pub(super) struct Names {
    reserved: HashSet<String>,
    used: HashSet<String>,
}

impl Names {
    pub fn new(program: &Program) -> Self {
        let mut reserved: HashSet<String> = [
            "auto",
            "break",
            "case",
            "char",
            "const",
            "continue",
            "default",
            "do",
            "double",
            "else",
            "enum",
            "extern",
            "float",
            "for",
            "goto",
            "if",
            "inline",
            "int",
            "long",
            "register",
            "restrict",
            "return",
            "short",
            "signed",
            "sizeof",
            "static",
            "struct",
            "switch",
            "typedef",
            "union",
            "unsigned",
            "void",
            "volatile",
            "while",
            "_Alignas",
            "_Alignof",
            "_Atomic",
            "_Bool",
            "_Complex",
            "_Generic",
            "_Imaginary",
            "_Noreturn",
            "_Static_assert",
            "_Thread_local",
            "bool",
            "int8_t",
            "uint8_t",
            "int16_t",
            "uint16_t",
            "int32_t",
            "uint32_t",
            "int64_t",
            "uint64_t",
            "intptr_t",
            "uintptr_t",
            "size_t",
            "ptrdiff_t",
            "intmax_t",
            "uintmax_t",
            "FILE",
            "main",
            "stdin",
            "stdout",
            "stderr",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        reserved.extend(
            program
                .functions
                .iter()
                .filter_map(|function| function.link_name.clone()),
        );
        reserved.extend(
            program
                .globals
                .iter()
                .map(|global| global.link_name.clone()),
        );
        // Runtime calls and C library calls share the ordinary C namespace
        // with locals. A source binding must not hide one of these calls.
        for runtime in [RUNTIME, NUMERIC_RUNTIME] {
            let bytes = runtime.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
                    let start = index;
                    while index < bytes.len()
                        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                    {
                        index += 1;
                    }
                    let end = index;
                    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                        index += 1;
                    }
                    if bytes.get(index) == Some(&b'(') {
                        reserved.insert(runtime[start..end].to_owned());
                    }
                } else {
                    index += 1;
                }
            }
        }
        return Self {
            reserved,
            used: HashSet::new(),
        };
    }

    pub fn begin_function(&mut self) {
        self.used.clear();
    }

    fn available(&self, name: &str) -> bool {
        return !name.is_empty()
            && name
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && !name.starts_with("crust_")
            && !name.starts_with('_')
            && !self.reserved.contains(name);
    }
}

impl Emitter<'_> {
    pub(super) fn binding_name(&mut self, generated: &str, source: &str) {
        if !self.debug_names.available(source) {
            return;
        }
        if self.debug_names.used.insert(source.to_owned()) {
            self.line(&format!(
                "#if !defined({source})\n#define {generated} {source}\n#endif"
            ));
        } else {
            // Cleanup for an outer binding can run inside an inner scope.
            // Keep its C spelling distinct from the shadowing declaration.
            self.line(&format!(
                "#define {generated} crust_shadow_{source}_{generated}"
            ));
        }
    }

    pub(super) fn type_names(&mut self, program: &Program) {
        let native_headers = !program.native_headers.is_empty();
        let mut fields = HashSet::new();
        for (id, info) in self.types.structs.iter().enumerate() {
            if info.complete {
                let source = &info.name;
                let readable: String = source
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() || character == '_' {
                            character
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let name = if !native_headers && self.debug_names.available(source) {
                    source.clone()
                } else {
                    format!("crust_{readable}_{id}")
                };
                let kind = if info.is_union { "union" } else { "struct" };
                self.line(&format!("#if !defined({name})\n#define crust_struct_{id} {name}\n#endif\n#define crust_type_{id} {kind} crust_struct_{id}"));
            }
            for field in &info.fields {
                if fields.insert(field.name.clone()) && self.debug_names.available(&field.name) {
                    self.line(&format!(
                        "#if !defined({})\n#define crust_field_{} {}\n#endif",
                        field.name, field.name, field.name
                    ));
                }
            }
        }
        for definition in &program.enums {
            let name = &program.enum_names[definition.id].0;
            let readable: String = name
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || character == '_' {
                        character
                    } else {
                        '_'
                    }
                })
                .collect();
            self.line(&format!(
                "#define crust_enum_{} crust_{}_{}",
                definition.id, readable, definition.id
            ));
        }
    }
}
