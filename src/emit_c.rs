use std::collections::{HashMap, HashSet};
mod globals;

use crate::ast::*;
use crate::check::{CheckedProgram, FunctionAnalysis};
use crate::diagnostic::Span;
use crate::types::Types;

pub(crate) struct Source {
    start: usize,
    name: String,
    line_starts: Vec<usize>,
}

impl Source {
    pub fn new(start: usize, name: String, text: &str) -> Self {
        let mut line_starts = vec![start];
        let bytes = text.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\n' || (*byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')) {
                line_starts.push(start + index + 1);
            }
        }
        return Self {
            start,
            name,
            line_starts,
        };
    }
}

pub(crate) fn generate(checked: &CheckedProgram) -> String {
    return generate_with_sources(checked, &[]);
}

pub(crate) fn generate_with_sources(checked: &CheckedProgram, sources: &[Source]) -> String {
    let program = &checked.program;
    let mut native_headers = program
        .native_headers
        .iter()
        .map(|name| format!("#include \"{name}\"\n"))
        .collect::<String>();
    if !native_headers.is_empty() {
        for name in program
            .functions
            .iter()
            .filter_map(|function| function.link_name.as_ref())
            .chain(program.globals.iter().map(|global| &global.link_name))
        {
            native_headers.push_str(&format!("#undef {name}\n"));
        }
    }
    let empty_analysis = FunctionAnalysis::default();
    let mut emitter = Emitter {
        source_files: sources,
        source_offset: None,
        mapped_location: None,
        runtime_files_used: false,
        output: format!("{native_headers}{RUNTIME}{NUMERIC_RUNTIME}"),
        indent: 0,
        next_name: 0,
        scopes: vec![Scope::default()],
        return_types: program
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.return_type))
            .collect(),
        types: &checked.types,
        expression_types: &empty_analysis.expression_types,
        reference_reads: &empty_analysis.reference_reads,
        place_volatile: &empty_analysis.place_volatile,
        method_calls: &empty_analysis.method_calls,
        conversions: &empty_analysis.conversions,
        constants: &empty_analysis.constants,
        loops: Vec::new(),
        external_names: program
            .functions
            .iter()
            .filter_map(|function| {
                function
                    .link_name
                    .clone()
                    .map(|name| (function.name.clone(), name))
            })
            .collect(),
        function_values: &empty_analysis.function_values,
    };
    let source_declaration_position = emitter.output.len();
    emitter
        .return_types
        .insert(String::from("print"), Type::Void);
    emitter
        .return_types
        .insert(String::from("box_int"), Type::BoxInt);
    emitter
        .return_types
        .insert(String::from("move"), Type::BoxInt);

    emitter.line(&format!("_Static_assert(sizeof(void *) * CHAR_BIT == {}, \"C compiler target must match the Crust host\");", usize::BITS));
    emitter.line(&format!(
        "_Static_assert(CHAR_MIN == {}, \"C char signedness must match the Crust host\");",
        IntType::Char.minimum()
    ));
    emitter.unsigned_runtime();
    emitter.line(&format!("_Static_assert(sizeof(long) * CHAR_BIT == {}, \"C long width must match the Crust host\");", std::mem::size_of::<std::ffi::c_long>() * 8));
    emitter.line("_Static_assert(sizeof(int) * CHAR_BIT == 32 && sizeof(short) * CHAR_BIT == 16 && sizeof(long long) * CHAR_BIT == 64, \"C integer widths must match the Crust host\");");
    let native_i64 = if IntType::native_long() == IntType::I64 {
        "long"
    } else {
        "long long"
    };
    emitter.line(&format!("_Static_assert(_Generic((int64_t)0, {native_i64}: 1, default: 0), \"C int64_t must match the Crust host ABI\");"));
    for name in ["size_t", "ptrdiff_t", "uintptr_t", "intptr_t"] {
        emitter.line(&format!("_Static_assert(_Generic(({name})0, {}: 1, default: 0), \"C {name} must match the Crust host ABI\");", IntType::named(name).unwrap().c_name()));
    }
    for definition in &checked.types.enums {
        if let Some(name) = &definition.native_name {
            emitter.line(&format!("#pragma push_macro(\"{name}\")\n#undef {name}"));
            emitter.line(&format!(
                "typedef enum {name} crust_enum_{};",
                definition.id
            ));
            let underlying = definition.underlying.c_name();
            // MSVC treats native enums as distinct types in _Generic. Its
            // representation is checked through signedness, size, and alignment.
            emitter.line("#if !defined(_MSC_VER) || defined(__clang__)");
            emitter.line(&format!("_Static_assert(_Generic((enum {name})0, {underlying}: 1, default: 0), \"native enum integer representation must match its declaration\");"));
            emitter.line("#endif");
            emitter.line(&format!("_Static_assert(((enum {name})-1 < (enum {name})0) == {}, \"native enum signedness must match its declaration\");", i32::from(definition.underlying.signed())));
            emitter.line(&format!("_Static_assert(sizeof(enum {name}) == sizeof({underlying}) && _Alignof(enum {name}) == _Alignof({underlying}), \"native enum layout must match its integer representation\");"));
            emitter.line(&format!("#pragma pop_macro(\"{name}\")"));
            continue;
        }
        emitter.line(&format!(
            "typedef {} crust_enum_{};",
            definition.underlying.c_name(),
            definition.id
        ));
    }
    for id in 0..checked.types.structs.len() {
        let info = &checked.types.structs[id];
        let tag = if info.complete {
            format!("crust_struct_{id}")
        } else {
            info.native_name
                .clone()
                .unwrap_or_else(|| info.name.clone())
        };
        let kind = if info.is_union { "union" } else { "struct" };
        if !info.complete {
            emitter.line(&format!("#pragma push_macro(\"{tag}\")\n#undef {tag}"));
        }
        emitter.line(&format!("typedef {kind} {tag} crust_type_{id};"));
        if !info.complete {
            emitter.line(&format!("#pragma pop_macro(\"{tag}\")"));
        }
    }
    for id in 0..checked.types.arrays.len() {
        emitter.line(&format!("struct crust_array_{id};"));
    }
    let mut declared = HashSet::new();
    for id in 0..checked.types.pointers.len() {
        emitter.type_declaration(Type::Pointer(id), &mut declared);
    }
    for id in 0..checked.types.function_types.len() {
        emitter.type_declaration(Type::Function(id), &mut declared);
    }
    emitter.struct_definitions();
    for variable in &program.globals {
        emitter.line(&format!(
            "{}{}{}{}{} {};",
            if variable.external {
                "extern "
            } else {
                "static "
            },
            if variable.dll_import {
                "__declspec(dllimport) "
            } else {
                ""
            },
            c_type(variable.ty),
            if variable.readonly { " const" } else { "" },
            if variable.volatile { " volatile" } else { "" },
            variable.link_name
        ));
        emitter.scopes[0].insert(
            variable.name.clone(),
            Local {
                name: variable.link_name.clone(),
                ty: variable.ty,
                live: None,
            },
        );
    }
    for function in &program.functions {
        if function.name.starts_with('?') {
            continue;
        }
        emitter.line(&format!("{};", header(function, checked.kind)));
    }
    emitter.global_definitions(checked);
    emitter.drop_functions();
    for function in &program.functions {
        if function.external || function.name.starts_with('?') {
            continue;
        }
        let analysis = &checked.analyses[&function.name];
        emitter.expression_types = &analysis.expression_types;
        emitter.reference_reads = &analysis.reference_reads;
        emitter.place_volatile = &analysis.place_volatile;
        emitter.method_calls = &analysis.method_calls;
        emitter.conversions = &analysis.conversions;
        emitter.constants = &analysis.constants;
        emitter.function_values = &analysis.function_values;
        emitter.line("");
        emitter.source_offset = Some(function.span.start);
        emitter.line(&header(function, checked.kind));
        let mut scope = Scope::default();
        for (index, parameter) in function.parameters.iter().enumerate() {
            scope.insert(
                parameter.name.clone(),
                Local {
                    name: format!("crust_arg_{index}"),
                    ty: parameter.ty,
                    live: if matches!(parameter.ty, Type::Struct(_) | Type::Array(_))
                        && checked.types.owns(parameter.ty)
                    {
                        Some(format!("crust_live_arg_{index}"))
                    } else {
                        None
                    },
                },
            );
        }
        emitter.block_with_scope(&function.body, scope);
        emitter.source_offset = None;
    }
    if emitter.runtime_files_used {
        let names = sources
            .iter()
            .map(|source| format!("    {},\n", c_string(&source.name)))
            .collect::<String>();
        emitter.output.insert_str(
            source_declaration_position,
            &format!("static const char *const crust_rt_files[] = {{\n{names}}};\n"),
        );
    }
    return emitter.output;
}

fn function_name(name: &str) -> String {
    if let Some(local) = name.strip_prefix('!') {
        return format!("crust_local_{local}");
    }
    if let Some(generic) = name.strip_prefix('$') {
        return format!("crust_generic_{}", generic.replace('!', "local_"));
    }
    if let Some(method) = name.strip_prefix('#') {
        return format!("crust_static_{method}");
    }
    if let Some(id) = name.strip_prefix('~') {
        return format!("crust_destructor_{id}");
    }
    if let Some(method) = name.strip_prefix('@') {
        return format!("crust_method_{method}");
    }
    if name == "main" {
        return String::from("main");
    }
    if name == "print" || name == "box_int" {
        return format!("crust_rt_{name}");
    }
    return format!("crust_fn_{name}");
}

pub(crate) fn c_type(ty: Type) -> String {
    return match ty {
        Type::Function(id) => format!("crust_function_{id}"),
        Type::Integer(kind) => kind.c_name().to_owned(),
        Type::Float(kind) => kind.c_name().to_owned(),
        Type::Bool => String::from("bool"),
        Type::Void => String::from("void"),
        Type::BoxInt => String::from("int32_t *"),
        Type::Struct(id) => format!("crust_type_{id}"),
        Type::Array(id) => format!("struct crust_array_{id}"),
        Type::Enum(id) => format!("crust_enum_{id}"),
        Type::Pointer(id) => format!("crust_ptr_{id}"),
        Type::Borrow {
            pointee: Pointee::Slice(id),
            ..
        } => format!("struct crust_array_{id}"),
        Type::Borrow {
            pointee,
            mutable,
            volatile,
        } => format!(
            "{}{}{} *",
            c_type(pointee.ty()),
            if mutable { "" } else { " const" },
            if volatile { " volatile" } else { "" }
        ),
    };
}

fn header(function: &Function, kind: crate::ProgramKind) -> String {
    let result = if function.name == "main" {
        String::from("int")
    } else {
        c_type(function.return_type)
    };
    let mut parameters = if function.parameters.is_empty() {
        String::from("void")
    } else {
        function
            .parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| format!("{} crust_arg_{index}", c_type(parameter.ty)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if function.variadic {
        parameters.push_str(", ...");
    }
    return format!(
        "{}{result} {}{}({parameters})",
        if (kind == crate::ProgramKind::Library && function.link_name.is_none())
            || function.name.contains('!')
        {
            "static "
        } else if function.dll_import {
            "__declspec(dllimport) "
        } else if !function.external && function.link_name.is_some() {
            "CRUST_EXPORT "
        } else {
            ""
        },
        if function.name == "main" {
            ""
        } else {
            function.calling_convention.c_prefix()
        },
        function
            .link_name
            .clone()
            .unwrap_or_else(|| function_name(&function.name))
    );
}

#[derive(Clone)]
struct Local {
    name: String,
    ty: Type,
    // Named owning structs can be moved without changing their public layout.
    // Fields and unnamed results are always initialized when cleanup reaches them.
    live: Option<String>,
}

#[derive(Default)]
struct Scope {
    variables: HashMap<String, Local>,
    declaration_order: Vec<String>,
}

impl Scope {
    fn insert(&mut self, name: String, local: Local) {
        self.declaration_order.push(name.clone());
        self.variables.insert(name, local);
    }
}

struct Emitter<'checked> {
    source_files: &'checked [Source],
    source_offset: Option<usize>,
    mapped_location: Option<(usize, usize)>,
    runtime_files_used: bool,
    function_values: &'checked HashMap<Span, String>,
    output: String,
    indent: usize,
    next_name: usize,
    scopes: Vec<Scope>,
    return_types: HashMap<String, Type>,
    types: &'checked Types,
    expression_types: &'checked HashMap<Span, Type>,
    reference_reads: &'checked HashMap<Span, Type>,
    place_volatile: &'checked HashMap<Span, bool>,
    method_calls: &'checked HashMap<Span, Expression>,
    conversions: &'checked HashMap<Span, Type>,
    constants: &'checked HashMap<Span, i128>,
    loops: Vec<LoopContext>,
    external_names: HashMap<String, String>,
}

struct LoopContext {
    is_switch: bool,
    scope_count: usize,
    step: Option<Box<Statement>>,
    continue_label: Option<String>,
}

impl Emitter<'_> {
    fn runtime_location(&mut self, span: Span) -> String {
        let file = self
            .source_files
            .partition_point(|source| source.start <= span.start);
        let name = if file == 0 {
            "NULL".to_owned()
        } else {
            self.runtime_files_used = true;
            format!("crust_rt_files[{}]", file - 1)
        };
        return format!("{name}, {}, {}", span.line, span.column);
    }

    fn address_type(&self, expression: &Expression, mutable: bool) -> Type {
        return self
            .place_type(expression)
            .borrowed_qualified(
                mutable,
                self.place_volatile
                    .get(&expression.span)
                    .copied()
                    .unwrap_or(false),
            )
            .expect("checked borrow target");
    }
    fn type_declaration(&mut self, ty: Type, declared: &mut HashSet<Type>) {
        if !declared.insert(ty) {
            return;
        }
        match ty {
            Type::Pointer(id) => {
                let pointer = self.types.pointers[id];
                self.type_declaration(pointer.pointee, declared);
                self.line(&format!(
                    "typedef {}{}{} *crust_ptr_{id};",
                    c_type(pointer.pointee),
                    if pointer.mutable { "" } else { " const" },
                    if pointer.volatile { " volatile" } else { "" }
                ));
            }
            Type::Function(id) => {
                let function = self.types.function_types[id].clone();
                self.type_declaration(function.result, declared);
                for parameter in &function.parameters {
                    self.type_declaration(*parameter, declared);
                }
                let mut parameters = if function.parameters.is_empty() {
                    String::from("void")
                } else {
                    function
                        .parameters
                        .iter()
                        .map(|ty| c_type(*ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                if function.variadic {
                    parameters.push_str(", ...");
                }
                self.line(&format!(
                    "typedef {} {}crust_function_{id}({parameters});",
                    c_type(function.result),
                    function.calling_convention.c_prefix()
                ));
            }
            _ => {}
        }
    }
    fn unsigned_runtime(&mut self) {
        for kind in [IntType::U32, IntType::U64] {
            let ty = kind.c_name();
            let bits = kind.bits();
            for (name, symbol) in [
                ("add", "+"),
                ("sub", "-"),
                ("mul", "*"),
                ("div", "/"),
                ("rem", "%"),
            ] {
                self.line(&format!("static inline {ty} crust_rt_u{bits}_{name}({ty} a, {ty} b, const char *crust_file, size_t line, size_t column) {{"));
                self.indent += 1;
                self.line("(void)crust_file; (void)line; (void)column;");
                if name == "div" || name == "rem" {
                    let reason = if name == "div" {
                        "division by zero"
                    } else {
                        "remainder by zero"
                    };
                    self.line(&format!(
                        "if (b == 0) {{ crust_rt_fail(\"{reason}\", crust_file, line, column); }}"
                    ));
                }
                self.line(&format!("return ({ty})(a {symbol} b);"));
                self.indent -= 1;
                self.line("}");
            }
            self.line(&format!("static inline {ty} crust_rt_u{bits}_neg({ty} value, const char *crust_file, size_t line, size_t column) {{ (void)crust_file; (void)line; (void)column; return ({ty})(0 - value); }}"));
        }
    }

    fn struct_definitions(&mut self) {
        for ty in &self.types.order {
            if let Type::Array(id) = ty {
                let array = self.types.arrays[*id];
                if array.length == 0 {
                    self.line(&format!("struct crust_array_{id} {{ {} const *crust_elements; size_t crust_field_length; }};", c_type(array.element)));
                    continue;
                }
                self.line(&format!(
                    "struct crust_array_{id} {{ {} crust_elements[{}]; }};",
                    c_type(array.element),
                    array.length
                ));
                continue;
            }
            let Type::Struct(id) = ty else { unreachable!() };
            let info = &self.types.structs[*id];
            if !info.complete {
                continue;
            }
            let kind = if info.is_union { "union" } else { "struct" };
            self.line(&format!("{kind} crust_struct_{id} {{"));
            self.indent += 1;
            if info.tagged {
                self.line("size_t crust_tag; union {");
                self.indent += 1;
            }
            for field in &info.fields {
                self.line(&format!("{} crust_field_{};", c_type(field.ty), field.name));
            }
            if info.tagged {
                self.indent -= 1;
                self.line("};");
            }
            self.indent -= 1;
            self.line("};");
            self.line("");
        }
    }

    fn drop_functions(&mut self) {
        for ty in &self.types.order {
            if let Type::Array(id) = ty {
                if self.types.owns(*ty) {
                    let array = self.types.arrays[*id];
                    self.line(&format!("static inline void crust_drop_array_{id}(struct crust_array_{id} *value) {{"));
                    self.indent += 1;
                    self.line(&format!(
                        "for (size_t crust_index = {}; crust_index > 0; --crust_index) {{",
                        array.length
                    ));
                    self.indent += 1;
                    self.drop_value(array.element, "value->crust_elements[crust_index - 1]");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                }
                continue;
            }
            let Type::Struct(id) = ty else { unreachable!() };
            let info = &self.types.structs[*id];
            if info.owns {
                self.line(&format!(
                    "static inline void crust_drop_struct_{id}(crust_type_{id} *value) {{"
                ));
                self.indent += 1;
                if info.has_destructor {
                    self.line(&format!("{}(value);", function_name(&destructor_name(*id))));
                }
                for (index, field) in info.fields.iter().enumerate().rev() {
                    if info.tagged {
                        self.line(&format!("if (value->crust_tag == {}) {{", index));
                        self.indent += 1;
                    }
                    self.drop_value(field.ty, &format!("value->crust_field_{}", field.name));
                    if info.tagged {
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                self.indent -= 1;
                self.line("}");
            }
            self.line("");
        }
    }

    fn drop_value(&mut self, ty: Type, place: &str) {
        match ty {
            Type::BoxInt => self.line(&format!("crust_rt_drop(&({place}));")),
            Type::Struct(id) if self.types.owns(ty) => {
                self.line(&format!("crust_drop_struct_{id}(&({place}));"))
            }
            Type::Array(id) if self.types.owns(ty) => {
                self.line(&format!("crust_drop_array_{id}(&({place}));"))
            }
            _ => {}
        }
    }

    fn drop_local(&mut self, local: &Local) {
        if let Some(live) = &local.live {
            self.line(&format!("if ({live}) {{"));
            self.indent += 1;
            self.drop_value(local.ty, &local.name);
            self.line(&format!("{live} = 0;"));
            self.indent -= 1;
            self.line("}");
        } else {
            self.drop_value(local.ty, &local.name);
        }
    }

    fn block(&mut self, block: &Block) {
        self.block_with_scope(block, Scope::default());
    }

    fn block_with_scope(&mut self, block: &Block, scope: Scope) {
        let previous = self.source_offset.replace(block.span.start);
        self.line("{");
        self.indent += 1;
        for name in &scope.declaration_order {
            if let Some(live) = &scope.variables[name].live {
                self.line(&format!("int {live} = 1;"));
            }
        }
        self.scopes.push(scope);
        for statement in &block.statements {
            self.statement(statement);
        }
        self.source_offset = Some(block.span.end.saturating_sub(1));
        self.cleanup_scope(self.scopes.len() - 1);
        self.scopes.pop();
        self.indent -= 1;
        self.line("}");
        self.source_offset = previous;
    }

    fn statement(&mut self, statement: &Statement) {
        let previous = self.source_offset.replace(statement.span.start);
        self.statement_inner(statement);
        self.source_offset = previous;
    }

    fn statement_inner(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Unsafe(block) => self.block(block),
            StatementKind::Local {
                name,
                ty,
                initializer,
                ..
            } => {
                let value = self.value_expression(initializer);
                let local = self.fresh_name("local");
                self.line(&format!("{} {local} = {value};", c_type(*ty)));
                let live = if matches!(ty, Type::Struct(_) | Type::Array(_)) && self.types.owns(*ty)
                {
                    let name = self.fresh_name("live");
                    self.line(&format!("int {name} = 1;"));
                    Some(name)
                } else {
                    None
                };
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        name: local,
                        ty: *ty,
                        live,
                    },
                );
            }
            StatementKind::StaticLocal {
                name, global, ty, ..
            } => {
                let storage = self.scopes[0].variables[global].name.clone();
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        name: storage,
                        ty: *ty,
                        live: None,
                    },
                );
            }
            StatementKind::Assign { name, value, .. } => {
                // Finish the right side first, including a possible move of the
                // old value, before dropping the destination's previous owner.
                let value = self.value_expression(value);
                let local = self.local(name);
                if let Type::Borrow { pointee, .. } = local.ty {
                    let place = format!("*({})", local.name);
                    self.drop_value(pointee.ty(), &place);
                    self.line(&format!("{place} = {value};"));
                    return;
                }
                self.drop_local(&local);
                self.line(&format!("{} = {value};", local.name));
                if let Some(live) = local.live {
                    self.line(&format!("{live} = 1;"));
                }
            }
            StatementKind::Store { target, value } => {
                let value = self.value_expression(value);
                let place = self.place(target);
                self.drop_value(self.place_type(target), &place);
                self.line(&format!("{place} = {value};"));
            }
            StatementKind::CompoundAssign {
                target,
                operator,
                value,
            } => {
                let target_type = self.place_type(target);
                if matches!(target_type, Type::Pointer(_)) {
                    let place = self.place(target);
                    let address =
                        self.temporary(self.address_type(target, true), &format!("&({place})"));
                    let old = self.temporary(target_type, &format!("*{address}"));
                    let right = self.value_expression(value);
                    let symbol = if *operator == BinaryOperator::Add {
                        "+"
                    } else {
                        "-"
                    };
                    let result = self.temporary(target_type, &format!("{old} {symbol} {right}"));
                    self.line(&format!("*{address} = {result};"));
                    return;
                }
                if matches!(target_type, Type::Float(_))
                    || matches!(self.expression_type(value), Type::Float(_))
                {
                    let common = target_type
                        .common_numeric(self.expression_type(value))
                        .unwrap();
                    let place = self.place(target);
                    let address =
                        self.temporary(self.address_type(target, true), &format!("&({place})"));
                    let old = self.temporary(target_type, &format!("*{address}"));
                    let left = self.convert_value(old, target_type, common, target.span);
                    let right = self.value_expression(value);
                    let result = self.temporary(
                        common,
                        &format!("({left} {} {right})", operator.numeric_symbol()),
                    );
                    let result = self.convert_value(result, common, target_type, statement.span);
                    self.line(&format!("*{address} = {result};"));
                    return;
                }
                let left_kind = target_type.integer().unwrap();
                let common = if operator.is_shift() {
                    left_kind.promoted()
                } else {
                    left_kind.common(self.expression_type(value).integer().unwrap())
                };
                let place = self.place(target);
                let address =
                    self.temporary(self.address_type(target, true), &format!("&({place})"));
                let old = self.temporary(target_type, &format!("*{address}"));
                let left = self.convert_value(old, target_type, Type::Integer(common), target.span);
                let right = self.value_expression(value);
                let operation = self.operation(*operator, common, &left, &right, statement.span);
                let result = self.temporary(Type::Integer(common), &operation);
                let result =
                    self.convert_value(result, Type::Integer(common), target_type, statement.span);
                self.line(&format!("*{address} = {result};"));
            }
            StatementKind::Expression(expression) => {
                if let Some(value) = self.expression(expression) {
                    if self.types.owns(self.expression_type(expression)) {
                        // Discarded owning results still need a destructor.
                        self.drop_value(self.expression_type(expression), &value);
                    } else {
                        self.line(&format!("(void){value};"));
                    }
                }
            }
            StatementKind::Return(value) => {
                let value = value
                    .as_ref()
                    .map(|expression| self.value_expression(expression));
                for index in (0..self.scopes.len()).rev() {
                    self.cleanup_scope(index);
                }
                match value {
                    Some(value) => self.line(&format!("return {value};")),
                    None => self.line("return;"),
                }
            }
            StatementKind::Block(block) => self.block(block),
            StatementKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.value_expression(condition);
                self.line(&format!("if ({condition})"));
                self.block(then_block);
                if let Some(block) = else_block {
                    self.line("else");
                    self.block(block);
                }
            }
            StatementKind::Break | StatementKind::Continue => {
                let context = self
                    .loops
                    .iter()
                    .rev()
                    .find(|context| {
                        matches!(statement.kind, StatementKind::Break) || !context.is_switch
                    })
                    .expect("checked loop control");
                let scope_count = context.scope_count;
                let step = context.step.clone();
                let continue_label = context.continue_label.clone();
                for index in (scope_count..self.scopes.len()).rev() {
                    self.cleanup_scope(index);
                }
                if matches!(statement.kind, StatementKind::Continue) {
                    if let Some(step) = step {
                        let inner_scopes = self.scopes.split_off(scope_count);
                        self.statement(&step);
                        self.scopes.extend(inner_scopes);
                    }
                    if let Some(label) = continue_label {
                        self.line(&format!("goto {label};"));
                    } else {
                        self.line("continue;");
                    }
                } else {
                    self.line("break;");
                }
            }
            StatementKind::Switch { value, cases } => {
                let tagged = match self.place_type(value) {
                    Type::Struct(id) if self.types.structs[id].tagged => Some(id),
                    _ => None,
                };
                let numeric = if tagged.is_none() {
                    Some(
                        self.types
                            .integer_representation(self.expression_type(value))
                            .unwrap()
                            .promoted(),
                    )
                } else {
                    None
                };
                let tag = if tagged.is_some() {
                    let place = self.place(value);
                    self.temporary(
                        Type::Integer(IntType::named("size_t").unwrap()),
                        &format!("({place}).crust_tag"),
                    )
                } else {
                    self.value_expression(value)
                };
                self.line(&format!("switch ({tag}) {{"));
                self.loops.push(LoopContext {
                    is_switch: true,
                    scope_count: self.scopes.len(),
                    step: None,
                    continue_label: None,
                });
                for case in cases {
                    if let Some(label) = &case.label {
                        if let Some(id) = tagged {
                            let CaseLabel::Named { name, .. } = label else {
                                unreachable!()
                            };
                            let index = self.types.structs[id]
                                .fields
                                .iter()
                                .position(|field| field.name == *name)
                                .unwrap();
                            self.line(&format!("case {}:", index));
                        } else {
                            let constant = match label {
                                CaseLabel::Constant { value, .. } => *value,
                                CaseLabel::Named { name, .. } => {
                                    self.types.enumerator(name).unwrap().1.value
                                }
                            };
                            let kind = numeric.unwrap();
                            let bits = kind.wrap_constant(constant) as u64;
                            self.line(&format!("case (({})UINT64_C({bits})):", kind.c_name()));
                        }
                    } else {
                        self.line("default:");
                    }
                    self.block(&case.body);
                }
                if tagged.is_some_and(|id| cases.len() == self.types.structs[id].fields.len())
                    && cases.iter().all(|case| case.label.is_some())
                {
                    self.line("break;");
                    let location = self.runtime_location(value.span);
                    self.line(&format!("default: crust_rt_fail(\"invalid tagged-union discriminator\", {location});"));
                }
                self.loops.pop();
                self.line("}");
            }
            StatementKind::While {
                condition,
                body,
                step,
            } => {
                self.line("while (1) {");
                self.indent += 1;
                let condition = self.value_expression(condition);
                self.line(&format!("if (!{condition}) {{ break; }}"));
                self.loops.push(LoopContext {
                    is_switch: false,
                    scope_count: self.scopes.len(),
                    step: step.clone(),
                    continue_label: None,
                });
                self.block(body);
                if let Some(step) = step {
                    self.statement(step);
                }
                self.loops.pop();
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::DoWhile { body, condition } => {
                let label = format!("crust_continue_{}", statement.span.start);
                self.line("while (1) {");
                self.indent += 1;
                self.loops.push(LoopContext {
                    is_switch: false,
                    scope_count: self.scopes.len(),
                    step: None,
                    continue_label: Some(label.clone()),
                });
                self.block(body);
                self.loops.pop();
                self.line(&format!("{label}: ;"));
                let condition = self.value_expression(condition);
                self.line(&format!("if (!{condition}) {{ break; }}"));
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::Empty => self.line(";"),
        }
    }

    fn value_expression(&mut self, expression: &Expression) -> String {
        let previous = self.source_offset.replace(expression.span.start);
        let value = self.value_expression_inner(expression);
        self.source_offset = previous;
        return value;
    }

    fn value_expression_inner(&mut self, expression: &Expression) -> String {
        let value = self
            .expression(expression)
            .expect("checker requires a non-void value here");
        if let Some(target) = self.conversions.get(&expression.span) {
            return self.convert_value(
                value,
                self.expression_type(expression),
                *target,
                expression.span,
            );
        }
        return value;
    }

    fn convert_value(&mut self, value: String, from: Type, to: Type, span: Span) -> String {
        if from == to {
            return value;
        }
        if let Type::Borrow {
            pointee: Pointee::Slice(_),
            ..
        } = to
        {
            if let Type::Borrow {
                pointee: Pointee::Array(source),
                ..
            } = from
            {
                return self.temporary(to, &format!("({}){{ .crust_elements = ({value})->crust_elements, .crust_field_length = {} }}", c_type(to), self.types.arrays[source].length));
            }
            return value;
        }
        let cast = format!("({})({value})", c_type(to));
        if matches!(from, Type::Pointer(_) | Type::Borrow { .. })
            || matches!(to, Type::Pointer(_) | Type::Borrow { .. })
        {
            return self.temporary(to, &cast);
        }
        if from == Type::Bool || to == Type::Bool {
            return self.temporary(to, &cast);
        }
        if matches!(from, Type::Float(_)) || matches!(to, Type::Float(_)) {
            let location = self.runtime_location(span);
            if matches!(from, Type::Float(_))
                && let Some(target) = self.types.integer_representation(to)
            {
                let wide = self.temporary(
                    Type::Float(FloatType::LongDouble),
                    &format!("(long double)({value})"),
                );
                let upper = if target.signed() {
                    target.bits() - 1
                } else {
                    target.bits()
                };
                let lower = if target.signed() {
                    format!("({wide} >= -0x1p{upper}L || {wide} > (-0x1p{upper}L - 1.0L))")
                } else {
                    format!("{wide} > -1.0L")
                };
                self.line(&format!("if (!({lower} && {wide} < 0x1p{upper}L)) {{ crust_rt_fail(\"floating-to-integer conversion out of range\", {location}); }}"));
            }
            if let (Type::Float(source), Type::Float(target)) = (from, to)
                && source > target
            {
                let maximum = if target == FloatType::Float {
                    "FLT_MAX"
                } else {
                    "DBL_MAX"
                };
                self.line(&format!("if (isfinite({value}) && ((long double)({value}) > (long double){maximum} || (long double)({value}) < -(long double){maximum})) {{ crust_rt_fail(\"floating conversion out of range\", {location}); }}"));
            }
            return self.temporary(to, &cast);
        }
        let source = self
            .types
            .integer_representation(from)
            .expect("numeric conversion source");
        let target = self
            .types
            .integer_representation(to)
            .expect("numeric conversion target");
        if target.minimum() <= source.minimum() && target.maximum() >= source.maximum() {
            return self.temporary(to, &cast);
        }
        let bits = target.bits();
        let location = self.runtime_location(span);
        let call = match (source.signed(), target.signed()) {
            (true, true) => {
                format!("crust_rt_cast_signed({value}, INT{bits}_MIN, INT{bits}_MAX, {location})")
            }
            (true, false) => {
                format!("crust_rt_cast_unsigned_from_signed({value}, UINT{bits}_MAX, {location})")
            }
            (false, true) => {
                format!("crust_rt_cast_signed_from_unsigned({value}, INT{bits}_MAX, {location})")
            }
            (false, false) => {
                format!("crust_rt_cast_unsigned({value}, UINT{bits}_MAX, {location})")
            }
        };
        return self.temporary(to, &format!("({})({call})", c_type(to)));
    }

    fn operation(
        &mut self,
        operator: BinaryOperator,
        kind: IntType,
        left: &str,
        right: &str,
        span: Span,
    ) -> String {
        if operator.is_shift() {
            let location = self.runtime_location(span);
            let direction = if operator == BinaryOperator::ShiftLeft {
                "left"
            } else {
                "right"
            };
            let sign = if kind.signed() { "signed" } else { "unsigned" };
            return format!(
                "crust_rt_shift_{direction}_{sign}({left}, (uint64_t)({right}), {}, {location})",
                kind.bits()
            );
        }
        let helper = match operator {
            BinaryOperator::Add => Some("add"),
            BinaryOperator::Subtract => Some("sub"),
            BinaryOperator::Multiply => Some("mul"),
            BinaryOperator::Divide => Some("div"),
            BinaryOperator::Remainder => Some("rem"),
            _ => None,
        };
        if let Some(helper) = helper {
            let location = self.runtime_location(span);
            let family = kind.runtime_prefix();
            return format!("crust_rt_{family}{helper}({left}, {right}, {location})");
        }
        let symbol = match operator {
            BinaryOperator::Equal => "==",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::Less => "<",
            BinaryOperator::LessEqual => "<=",
            BinaryOperator::Greater => ">",
            BinaryOperator::GreaterEqual => ">=",
            BinaryOperator::BitAnd => "&",
            BinaryOperator::BitOr => "|",
            BinaryOperator::BitXor => "^",
            _ => unreachable!("non-arithmetic binary operator"),
        };
        return format!("({left} {symbol} {right})");
    }

    fn integer_constant(&self, expression: &Expression, value: i128) -> String {
        let kind = self
            .types
            .integer_representation(self.expression_type(expression))
            .unwrap();
        let bits = kind.bits();
        let literal = if value < 0 {
            if value == kind.minimum() {
                format!("INT{bits}_MIN")
            } else {
                format!("(-INT{bits}_C({}))", -value)
            }
        } else {
            format!(
                "{}INT{bits}_C({value})",
                if kind.signed() { "" } else { "U" }
            )
        };
        // Fixed-width macros can have a different native rank. For example,
        // INT32_C produces int on Windows, while a long literal must stay long.
        return if matches!(
            kind,
            IntType::Long | IntType::ULong | IntType::LongLong | IntType::ULongLong
        ) {
            format!("(({})({literal}))", kind.c_name())
        } else {
            literal
        };
    }

    // An owning expression result has exactly one consumer: an initializer,
    // assignment, argument, return, or discard. Its temporary is transferred to
    // that consumer; only named owning locals are registered for scope cleanup.
    fn expression(&mut self, expression: &Expression) -> Option<String> {
        let previous = self.source_offset.replace(expression.span.start);
        let mut value = self.expression_inner(expression);
        if let Some(ty) = self.reference_reads.get(&expression.span) {
            value = value.map(|value| self.temporary(ty.referent(), &format!("*({value})")));
        }
        self.source_offset = previous;
        return value;
    }

    fn expression_inner(&mut self, expression: &Expression) -> Option<String> {
        if let Some(name) = self.function_values.get(&expression.span) {
            let function = if let Some(symbol) = self.external_names.get(name) {
                symbol.clone()
            } else {
                function_name(name)
            };
            return Some(format!("(&{function})"));
        }
        if let Some(value) = self.constants.get(&expression.span) {
            return Some(self.integer_constant(expression, *value));
        }
        match &expression.kind {
            ExpressionKind::Update {
                target,
                increment,
                prefix,
            } => {
                let ty = self.expression_type(expression);
                let place = self.place(target);
                let address =
                    self.temporary(self.address_type(target, true), &format!("&({place})"));
                let old = self.temporary(ty, &format!("*{address}"));
                let value = if matches!(ty, Type::Pointer(_) | Type::Float(_)) {
                    let operator = if *increment { "+" } else { "-" };
                    self.temporary(ty, &format!("{old} {operator} 1"))
                } else {
                    let kind = ty.integer().unwrap().promoted();
                    let promoted =
                        self.convert_value(old.clone(), ty, Type::Integer(kind), expression.span);
                    let operator = if *increment {
                        BinaryOperator::Add
                    } else {
                        BinaryOperator::Subtract
                    };
                    let operation = self.operation(operator, kind, &promoted, "1", expression.span);
                    let value = self.temporary(Type::Integer(kind), &operation);
                    self.convert_value(value, Type::Integer(kind), ty, expression.span)
                };
                self.line(&format!("*{address} = {value};"));
                return Some(if *prefix { value } else { old });
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                let condition = self.value_expression(condition);
                let ty = self.expression_type(expression);
                let result = if ty == Type::Void {
                    None
                } else {
                    let result = self.fresh_name("conditional");
                    self.line(&format!("{} {result};", c_type(ty)));
                    Some(result)
                };
                self.line(&format!("if ({condition}) {{"));
                self.indent += 1;
                if let Some(result) = &result {
                    let value = self.value_expression(then_value);
                    self.line(&format!("{result} = {value};"));
                } else {
                    self.expression(then_value);
                }
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                if let Some(result) = &result {
                    let value = self.value_expression(else_value);
                    self.line(&format!("{result} = {value};"));
                } else {
                    self.expression(else_value);
                }
                self.indent -= 1;
                self.line("}");
                return result;
            }
            ExpressionKind::Storage { pointer, value } => {
                let Type::Pointer(id) = self.expression_type(pointer) else {
                    unreachable!()
                };
                let ty = self.types.pointers[id].pointee;
                let pointer = self.value_expression(pointer);
                if let Some(value) = value {
                    let value = self.value_expression(value);
                    self.line(&format!("*({pointer}) = {value};"));
                } else {
                    self.drop_value(ty, &format!("*({pointer})"));
                }
                return None;
            }
            ExpressionKind::StaticReference { .. } => {
                unreachable!("checked static reference has a function address")
            }
            ExpressionKind::IndirectCall { callee, arguments } => {
                let callee = self.value_expression(callee);
                let values = arguments
                    .iter()
                    .map(|argument| self.value_expression(argument))
                    .collect::<Vec<_>>();
                let call = format!("({callee})({})", values.join(", "));
                let ty = self.expression_type(expression);
                if ty == Type::Void {
                    self.line(&format!("{call};"));
                    return None;
                }
                return Some(self.temporary(ty, &call));
            }
            ExpressionKind::EnumValue { .. } => unreachable!("enum constants have checked values"),
            ExpressionKind::StringLiteral { bytes, .. } => {
                let ty = self.expression_type(expression);
                let signed_char = matches!(ty, Type::Array(id) if self.types.arrays[id].element == Type::Integer(IntType::Char))
                    && IntType::Char.signed();
                let mut values = bytes
                    .iter()
                    .map(|byte| {
                        if signed_char {
                            (*byte as i8).to_string()
                        } else {
                            byte.to_string()
                        }
                    })
                    .collect::<Vec<_>>();
                values.push(String::from("0"));
                return Some(self.temporary(
                    ty,
                    &format!(
                        "({}){{ .crust_elements = {{ {} }} }}",
                        c_type(ty),
                        values.join(", ")
                    ),
                ));
            }
            ExpressionKind::Cast { ty, operand } => {
                if *ty == Type::Void {
                    if let Some(value) = self.expression(operand) {
                        let operand_type = self.expression_type(operand);
                        if self.types.owns(operand_type) {
                            self.drop_value(operand_type, &value);
                        } else {
                            self.line(&format!("(void){value};"));
                        }
                    }
                    return None;
                }
                let value = self.value_expression(operand);
                return Some(self.temporary(*ty, &value));
            }
            ExpressionKind::SizeofType(ty) | ExpressionKind::Alignof(ty) => {
                let operator = if matches!(expression.kind, ExpressionKind::Alignof(_)) {
                    "_Alignof"
                } else {
                    "sizeof"
                };
                return Some(self.temporary(
                    self.expression_type(expression),
                    &format!("{operator}({})", c_type(*ty)),
                ));
            }
            ExpressionKind::SizeofValue(operand) => {
                return Some(self.temporary(
                    self.expression_type(expression),
                    &format!("sizeof({})", c_type(self.expression_type(operand))),
                ));
            }
            ExpressionKind::Boolean(value) => return Some(value.to_string()),
            ExpressionKind::Floating { spelling, .. } => {
                return Some(self.temporary(self.expression_type(expression), spelling));
            }
            ExpressionKind::MethodCall { .. } | ExpressionKind::StaticCall { .. } => {
                let call = self.method_calls[&expression.span].clone();
                return self.expression_inner(&call);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                let ty = self.expression_type(expression);
                let mut values = Vec::new();
                if let Type::Struct(id) = ty
                    && self.types.structs[id].tagged
                {
                    let index = self.types.structs[id]
                        .fields
                        .iter()
                        .position(|field| field.name == fields[0].name)
                        .unwrap();
                    values.push(format!(".crust_tag = {}", index));
                }
                for field in fields {
                    let value = self.value_expression(&field.value);
                    values.push(format!(".crust_field_{} = {value}", field.name));
                }
                let literal = format!("({}){{ {} }}", c_type(ty), values.join(", "));
                return Some(self.temporary(ty, &literal));
            }
            ExpressionKind::ArrayLiteral { elements, .. } => {
                let ty = self.expression_type(expression);
                let values = elements
                    .iter()
                    .map(|element| self.value_expression(element))
                    .collect::<Vec<_>>();
                return Some(self.temporary(
                    ty,
                    &format!(
                        "({}){{ .crust_elements = {{ {} }} }}",
                        c_type(ty),
                        values.join(", ")
                    ),
                ));
            }
            ExpressionKind::Field { .. } | ExpressionKind::Index { .. } => {
                let place = self.place(expression);
                return Some(self.temporary(self.expression_type(expression), &place));
            }
            ExpressionKind::Integer(_) | ExpressionKind::TypedInteger { .. } => {
                unreachable!("checked literal has a constant value")
            }
            ExpressionKind::Variable { name, .. } => {
                let local = self.local(name);
                let temporary = self.temporary(local.ty, &local.name);
                return Some(temporary);
            }
            ExpressionKind::Call {
                name, arguments, ..
            } => {
                if let Some(call) = self.method_calls.get(&expression.span)
                    && matches!(call.kind, ExpressionKind::IndirectCall { .. })
                {
                    return self.expression_inner(&call.clone());
                }
                if name == "move" {
                    let ExpressionKind::Variable { name: owner, .. } = &arguments[0].kind else {
                        let place = self.place(&arguments[0]);
                        let value = self.temporary(self.expression_type(expression), &place);
                        if let ExpressionKind::Field { base, .. } = &arguments[0].kind
                            && let ExpressionKind::Variable { name, .. } = &base.kind
                            && let Type::Struct(id) = self.local(name).ty
                            && self.types.structs[id].tagged
                            && let Some(live) = self.local(name).live
                        {
                            self.line(&format!("{live} = 0;"));
                        }
                        return Some(value);
                    };
                    let local = self.local(owner);
                    let temporary = self.temporary(local.ty, &local.name);
                    if !self.types.owns(local.ty) {
                        return Some(temporary);
                    }
                    // NULL is an internal drop flag, not a usable language value.
                    if local.ty == Type::BoxInt {
                        self.line(&format!("{} = NULL;", local.name));
                    } else {
                        self.line(&format!(
                            "{} = 0;",
                            local.live.expect("owning struct has a drop flag")
                        ));
                    }
                    return Some(temporary);
                }
                let mut values = Vec::new();
                for argument in arguments {
                    values.push(self.value_expression(argument));
                }
                if name == "slice" {
                    let ty = self.expression_type(expression);
                    let Type::Borrow {
                        pointee: Pointee::Slice(id),
                        ..
                    } = ty
                    else {
                        unreachable!()
                    };
                    let location = self.runtime_location(expression.span);
                    self.line(&format!("if (({} == NULL && {} != 0) || {} > SIZE_MAX / sizeof({})) {{ crust_rt_fail(\"invalid slice storage\", {}); }}", values[0], values[1], values[1], c_type(self.types.arrays[id].element), location));
                    return Some(self.temporary(
                        ty,
                        &format!(
                            "({}){{ .crust_elements = {}, .crust_field_length = {} }}",
                            c_type(ty),
                            values[0],
                            values[1]
                        ),
                    ));
                }
                if name == "box_int" {
                    values.push(self.runtime_location(expression.span));
                }
                if name == "print"
                    && let Type::Float(kind) = self.expression_type(&arguments[0])
                {
                    match kind {
                        FloatType::Float => {
                            self.line(&format!("printf(\"%.9g\\n\", (double){});", values[0]))
                        }
                        FloatType::Double => {
                            self.line(&format!("printf(\"%.17g\\n\", (double){});", values[0]))
                        }
                        FloatType::LongDouble => self.line(&format!(
                            "printf(\"%.*Lg\\n\", LDBL_DECIMAL_DIG, (long double){});",
                            values[0]
                        )),
                    }
                    return None;
                }
                let function = if name == "print" {
                    let argument_type = self.expression_type(&arguments[0]);
                    if let Some(length) = self.types.byte_array_length(argument_type) {
                        let value = &values[0];
                        let member = if argument_type.is_borrow() { "->" } else { "." };
                        let function =
                            if matches!(argument_type, Type::Borrow { volatile: true, .. }) {
                                "crust_rt_print_volatile_bytes"
                            } else {
                                "crust_rt_print_bytes"
                            };
                        self.line(&format!(
                            "{function}(({value}){member}crust_elements, {length});"
                        ));
                        return None;
                    }
                    let kind = self.expression_type(&arguments[0]).integer().unwrap();
                    match (kind.signed(), kind.bits()) {
                        (true, 64) => String::from("crust_rt_print_i64"),
                        (false, 32 | 64) => String::from("crust_rt_print_u64"),
                        _ => function_name(name),
                    }
                } else if let Some(symbol) = self.external_names.get(name) {
                    symbol.clone()
                } else {
                    function_name(name)
                };
                let call = format!("{}({})", function, values.join(", "));
                let ty = self.return_types[name];
                if ty == Type::Void {
                    self.line(&format!("{call};"));
                    return None;
                }
                let value = self.temporary(ty, &call);
                return Some(value);
            }
            ExpressionKind::Unary { operator, operand } => {
                if *operator == UnaryOperator::Address {
                    let place = self.place(operand);
                    if matches!(
                        self.expression_type(expression),
                        Type::Borrow {
                            pointee: Pointee::Slice(_),
                            ..
                        }
                    ) {
                        return Some(self.temporary(self.expression_type(expression), &place));
                    }
                    return Some(
                        self.temporary(self.expression_type(expression), &format!("&({place})")),
                    );
                }
                if let UnaryOperator::Borrow { .. } = operator {
                    let ty = self.expression_type(expression);
                    let place = self.place(operand);
                    if matches!(
                        ty,
                        Type::Borrow {
                            pointee: Pointee::Slice(_),
                            ..
                        }
                    ) {
                        return Some(self.temporary(ty, &place));
                    }
                    let value = self.temporary(ty, &format!("&({place})"));
                    return Some(value);
                }
                if *operator == UnaryOperator::Dereference {
                    let place = self.place(expression);
                    if matches!(self.place_type(expression), Type::Function(_)) {
                        return Some(
                            self.temporary(
                                self.expression_type(expression),
                                &format!("&({place})"),
                            ),
                        );
                    }
                    let value = self.temporary(self.place_type(expression), &place);
                    return Some(value);
                }
                let value = self.value_expression(operand);
                let operation = match operator {
                    UnaryOperator::Plus => {
                        return Some(value);
                    }
                    UnaryOperator::Not => format!("!{value}"),
                    UnaryOperator::BitNot => format!("~({value})"),
                    UnaryOperator::Negate => {
                        if matches!(self.expression_type(expression), Type::Float(_)) {
                            return Some(self.temporary(
                                self.expression_type(expression),
                                &format!("-({value})"),
                            ));
                        }
                        let family = self
                            .expression_type(expression)
                            .integer()
                            .unwrap()
                            .runtime_prefix();
                        format!(
                            "crust_rt_{family}neg({value}, {})",
                            self.runtime_location(expression.span)
                        )
                    }
                    UnaryOperator::Dereference => unreachable!(),
                    UnaryOperator::Address => unreachable!(),
                    UnaryOperator::Borrow { .. } => unreachable!(),
                };
                let temporary = self.temporary(self.expression_type(expression), &operation);
                return Some(temporary);
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left_value = self.value_expression(left);
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    let result = self
                        .temporary(Type::Integer(IntType::I32), &format!("({left_value} != 0)"));
                    let condition = if *operator == BinaryOperator::And {
                        result.clone()
                    } else {
                        format!("!{result}")
                    };
                    self.line(&format!("if ({condition}) {{"));
                    self.indent += 1;
                    let right = self.value_expression(right);
                    self.line(&format!("{result} = ({right} != 0);"));
                    self.indent -= 1;
                    self.line("}");
                    return Some(result);
                }
                if (matches!(self.expression_type(left), Type::Pointer(_))
                    || matches!(self.expression_type(right), Type::Pointer(_)))
                    && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                {
                    let right = self.value_expression(right);
                    let symbol = if *operator == BinaryOperator::Add {
                        "+"
                    } else {
                        "-"
                    };
                    return Some(self.temporary(
                        self.expression_type(expression),
                        &format!("{left_value} {symbol} {right}"),
                    ));
                }
                if matches!(self.expression_type(left), Type::Float(_))
                    || matches!(self.expression_type(right), Type::Float(_))
                {
                    let right = self.value_expression(right);
                    return Some(self.temporary(
                        self.expression_type(expression),
                        &format!("({left_value} {} {right})", operator.numeric_symbol()),
                    ));
                }
                let kind = if operator.is_shift() {
                    self.expression_type(left).integer().unwrap().promoted()
                } else if matches!(self.expression_type(left), Type::Pointer(_))
                    || matches!(self.expression_type(right), Type::Pointer(_))
                {
                    IntType::I32
                } else {
                    self.types
                        .integer_representation(self.expression_type(left))
                        .unwrap()
                        .common(
                            self.types
                                .integer_representation(self.expression_type(right))
                                .unwrap(),
                        )
                };
                let right = self.value_expression(right);
                let operation =
                    self.operation(*operator, kind, &left_value, &right, expression.span);
                let temporary = self.temporary(self.expression_type(expression), &operation);
                return Some(temporary);
            }
        }
    }

    fn expression_type(&self, expression: &Expression) -> Type {
        return self.expression_types[&expression.span];
    }

    fn place(&mut self, expression: &Expression) -> String {
        match &expression.kind {
            ExpressionKind::Index { base, index } => {
                let base_type = self.place_type(base);
                if matches!(base_type, Type::Pointer(_)) {
                    let value = if base.is_place() {
                        self.place(base)
                    } else {
                        self.value_expression(base)
                    };
                    let address = self.temporary(base_type, &value);
                    let index = self.value_expression(index);
                    return format!("({address})[{index}]");
                }
                let Type::Array(id) = base_type else {
                    unreachable!()
                };
                let length = self.types.arrays[id].length;
                let place = self.place(base);
                let address = self.temporary(
                    self.address_type(base, self.place_mutable(base)),
                    &format!("&({place})"),
                );
                let value = self.value_expression(index);
                let dynamic = length == 0;
                let bound = if dynamic {
                    format!("({address})->crust_field_length")
                } else {
                    length.to_string()
                };
                let signed = self.expression_type(index).integer().unwrap().signed();
                let check = format!(
                    "crust_rt_index_{}({value}, {bound}, {})",
                    if signed { "signed" } else { "unsigned" },
                    self.runtime_location(index.span)
                );
                let checked_index =
                    self.temporary(Type::Integer(IntType::named("size_t").unwrap()), &check);
                if dynamic && self.place_mutable(base) {
                    return format!(
                        "(({} *)({address})->crust_elements)[{checked_index}]",
                        c_type(self.types.arrays[id].element)
                    );
                }
                return format!("({address})->crust_elements[{checked_index}]");
            }
            ExpressionKind::Field { base, name, .. } => {
                let place = self.place(base);
                if let Type::Struct(id) = self.place_type(base)
                    && self.types.structs[id].tagged
                {
                    let index = self.types.structs[id]
                        .fields
                        .iter()
                        .position(|field| field.name == *name)
                        .unwrap();
                    let address = self.temporary(
                        self.address_type(base, self.place_mutable(base)),
                        &format!("&({place})"),
                    );
                    let location = self.runtime_location(expression.span);
                    self.line(&format!("if (({address})->crust_tag != {}) {{ crust_rt_fail(\"inactive tagged-union payload\", {location}); }}", index));
                    return format!("({address})->crust_field_{name}");
                }
                return format!("({place}).crust_field_{name}");
            }
            ExpressionKind::Variable { name, .. } => {
                let local = self.local(name);
                return if local.ty.is_borrow()
                    && !matches!(
                        local.ty,
                        Type::Borrow {
                            pointee: Pointee::Slice(_),
                            ..
                        }
                    ) {
                    format!("*({})", local.name)
                } else {
                    local.name
                };
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                let pointer = if operand.is_place() || self.expression_type(operand).is_borrow() {
                    self.place(operand)
                } else {
                    self.value_expression(operand)
                };
                return format!("*({pointer})");
            }
            _ => {
                let value = self.value_expression(expression);
                if self.expression_type(expression).is_borrow() {
                    if matches!(
                        self.expression_type(expression),
                        Type::Borrow {
                            pointee: Pointee::Slice(_),
                            ..
                        }
                    ) {
                        return value;
                    }
                    return format!("*({value})");
                }
                unreachable!("checker requires an assignable or borrowable place");
            }
        }
    }

    fn place_type(&self, expression: &Expression) -> Type {
        match &expression.kind {
            ExpressionKind::Index { base, .. } => {
                return match self.place_type(base) {
                    Type::Array(id) => self.types.arrays[id].element,
                    Type::Pointer(id) => self.types.pointers[id].pointee,
                    _ => unreachable!(),
                };
            }
            ExpressionKind::Field { base, name, .. } => {
                return self
                    .types
                    .field(self.place_type(base), name, expression.span)
                    .expect("checked field");
            }
            ExpressionKind::Variable { name, .. } => {
                return self.local(name).ty.referent();
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.place_type(operand) {
                    Type::BoxInt => Type::Integer(IntType::I32),
                    Type::Pointer(id) => self.types.pointers[id].pointee,
                    _ => unreachable!("checker requires a pointer-like value"),
                };
            }
            _ => return self.expression_type(expression).referent(),
        }
    }

    fn place_mutable(&self, expression: &Expression) -> bool {
        match &expression.kind {
            ExpressionKind::Variable { name, .. } => {
                return match self.local(name).ty {
                    Type::Borrow { mutable, .. } => mutable,
                    _ => true,
                };
            }
            ExpressionKind::Index { base, .. }
                if matches!(self.place_type(base), Type::Pointer(_)) =>
            {
                let Type::Pointer(id) = self.place_type(base) else {
                    unreachable!()
                };
                return self.types.pointers[id].mutable;
            }
            ExpressionKind::Field { base, .. } | ExpressionKind::Index { base, .. } => {
                return self.place_mutable(base);
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.place_type(operand) {
                    Type::Pointer(id) => self.types.pointers[id].mutable,
                    Type::BoxInt => self.place_mutable(operand),
                    _ => unreachable!(),
                };
            }
            _ => {
                return matches!(
                    self.expression_type(expression),
                    Type::Borrow { mutable: true, .. }
                );
            }
        }
    }

    fn cleanup_scope(&mut self, index: usize) {
        let scope = &self.scopes[index];
        let owners: Vec<Local> = scope
            .declaration_order
            .iter()
            .rev()
            .filter_map(|name| {
                let local = &scope.variables[name];
                if self.types.owns(local.ty) {
                    return Some(local.clone());
                }
                return None;
            })
            .collect();
        for owner in owners {
            self.drop_local(&owner);
        }
    }

    fn temporary(&mut self, ty: Type, expression: &str) -> String {
        let name = self.fresh_name("temp");
        self.line(&format!("{} {name} = {expression};", c_type(ty)));
        return name;
    }

    fn fresh_name(&mut self, prefix: &str) -> String {
        let name = format!("crust_{prefix}_{}", self.next_name);
        self.next_name += 1;
        return name;
    }

    fn local(&self, name: &str) -> Local {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.variables.get(name) {
                return local.clone();
            }
        }
        unreachable!("checker resolved the variable before code generation");
    }

    fn line(&mut self, text: &str) {
        if let Some(offset) = self.source_offset
            && let Some(index) = self
                .source_files
                .partition_point(|source| source.start <= offset)
                .checked_sub(1)
        {
            let source = &self.source_files[index];
            let line = source.line_starts.partition_point(|start| *start <= offset);
            if self.mapped_location != Some((index, line)) {
                let filename = if self.mapped_location.is_some_and(|(file, _)| file == index) {
                    String::new()
                } else {
                    format!(" {}", c_string(&source.name))
                };
                self.output.push_str(&format!("#line {line}{filename}\n"));
                self.mapped_location = Some((index, line));
            }
        }
        self.output.push_str(&"    ".repeat(self.indent));
        self.output.push_str(text);
        self.output.push('\n');
        if let Some((_, line)) = &mut self.mapped_location {
            *line += text.bytes().filter(|byte| *byte == b'\n').count() + 1;
        }
    }
}

const RUNTIME: &str = r#"/* Generated by Crust. */
#include <stdint.h>
#include <stddef.h>
#include <limits.h>
#include <stdbool.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <float.h>
#include <math.h>

#if defined(_WIN32) && defined(CRUST_BUILD_SHARED)
#define CRUST_EXPORT __declspec(dllexport)
#elif defined(CRUST_BUILD_SHARED)
#define CRUST_EXPORT __attribute__((visibility("default")))
#else
#define CRUST_EXPORT
#endif

static inline _Noreturn void crust_rt_fail(const char *message, const char *crust_file, size_t line, size_t column)
{
    if (crust_file != NULL) {
        fprintf(stderr, "%s:%zu:%zu: runtime error: %s\n", crust_file, line, column, message);
    } else {
        fprintf(stderr, "runtime error at %zu:%zu: %s\n", line, column, message);
    }
    exit(EXIT_FAILURE);
}

static inline int32_t crust_rt_range(int64_t value, const char *crust_file, size_t line, size_t column)
{
    if (value < INT32_MIN || value > INT32_MAX) {
        crust_rt_fail("integer overflow", crust_file, line, column);
    }
    return (int32_t)value;
}

static inline int32_t crust_rt_add(int32_t a, int32_t b, const char *crust_file, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a + b, crust_file, line, column);
}

static inline int32_t crust_rt_sub(int32_t a, int32_t b, const char *crust_file, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a - b, crust_file, line, column);
}

static inline int32_t crust_rt_mul(int32_t a, int32_t b, const char *crust_file, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a * b, crust_file, line, column);
}

static inline int32_t crust_rt_neg(int32_t value, const char *crust_file, size_t line, size_t column)
{
    return crust_rt_range(-(int64_t)value, crust_file, line, column);
}

static inline int32_t crust_rt_div(int32_t a, int32_t b, const char *crust_file, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("division by zero", crust_file, line, column); }
    if (a == INT32_MIN && b == -1) { crust_rt_fail("integer overflow", crust_file, line, column); }
    return a / b;
}

static inline int32_t crust_rt_rem(int32_t a, int32_t b, const char *crust_file, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("remainder by zero", crust_file, line, column); }
    if (a == INT32_MIN && b == -1) { crust_rt_fail("integer overflow", crust_file, line, column); }
    return a % b;
}

static inline void crust_rt_print(int32_t value)
{
    printf("%" PRId32 "\n", value);
}

static inline int32_t *crust_rt_box_int(int32_t value, const char *crust_file, size_t line, size_t column)
{
    int32_t *owner = (int32_t *)malloc(sizeof(int32_t));
    if (owner == NULL) { crust_rt_fail("out of memory", crust_file, line, column); }
    *owner = value;
    return owner;
}

static inline void crust_rt_drop(int32_t **owner)
{
    free(*owner);
    *owner = NULL;
}

"#;

fn c_string(value: &str) -> String {
    let mut result = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            b'?' => result.push_str("\\?"),
            32..=126 => result.push(char::from(byte)),
            _ => result.push_str(&format!("\\{byte:03o}")),
        }
    }
    result.push('"');
    return result;
}
const NUMERIC_RUNTIME: &str = include_str!("runtime_numbers.h");
