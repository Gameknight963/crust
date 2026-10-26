use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::check::{CheckedProgram, FunctionAnalysis};
use crate::diagnostic::Span;
use crate::types::Types;

pub(crate) fn generate(checked: &CheckedProgram) -> String {
    let program = &checked.program;
    let empty_analysis = FunctionAnalysis::default();
    let mut emitter = Emitter {
        output: format!("{RUNTIME}{NUMERIC_RUNTIME}"),
        indent: 0,
        next_name: 0,
        scopes: Vec::new(),
        return_types: program
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.return_type))
            .collect(),
        types: &checked.types,
        expression_types: &empty_analysis.expression_types,
        method_calls: &empty_analysis.method_calls,
        conversions: &empty_analysis.conversions,
        constants: &empty_analysis.constants,
        loops: Vec::new(),
        external_names: program
            .functions
            .iter()
            .filter(|function| function.external)
            .map(|function| function.name.clone())
            .collect(),
        function_values: &empty_analysis.function_values,
    };
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
    for definition in &checked.types.enums {
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
            info.name.clone()
        };
        let kind = if info.is_union { "union" } else { "struct" };
        emitter.line(&format!("typedef {kind} {tag} crust_type_{id};"));
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
    for function in &program.functions {
        emitter.line(&format!("{};", header(function)));
    }
    emitter.drop_functions();
    for function in &program.functions {
        if function.external {
            continue;
        }
        let analysis = &checked.analyses[&function.name];
        emitter.expression_types = &analysis.expression_types;
        emitter.method_calls = &analysis.method_calls;
        emitter.conversions = &analysis.conversions;
        emitter.constants = &analysis.constants;
        emitter.function_values = &analysis.function_values;
        emitter.line("");
        emitter.line(&header(function));
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
    }
    return emitter.output;
}

fn function_name(name: &str) -> String {
    if let Some(generic) = name.strip_prefix('$') {
        return format!("crust_generic_{generic}");
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

fn c_type(ty: Type) -> String {
    return match ty {
        Type::Function(id) => format!("crust_function_{id}"),
        Type::Integer(kind) => kind.c_name().to_owned(),
        Type::Bool => String::from("bool"),
        Type::Void => String::from("void"),
        Type::BoxInt => String::from("int32_t *"),
        Type::Struct(id) => format!("crust_type_{id}"),
        Type::Array(id) => format!("struct crust_array_{id}"),
        Type::Enum(id) => format!("crust_enum_{id}"),
        Type::Pointer(id) => format!("crust_ptr_{id}"),
        Type::Borrow { pointee, mutable } => format!(
            "{}{} *",
            c_type(pointee.ty()),
            if mutable { "" } else { " const" }
        ),
    };
}

fn header(function: &Function) -> String {
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
        "{result} {}({parameters})",
        if function.external {
            function.name.clone()
        } else {
            function_name(&function.name)
        }
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
    function_values: &'checked HashMap<Span, String>,
    output: String,
    indent: usize,
    next_name: usize,
    scopes: Vec<Scope>,
    return_types: HashMap<String, Type>,
    types: &'checked Types,
    expression_types: &'checked HashMap<Span, Type>,
    method_calls: &'checked HashMap<Span, Expression>,
    conversions: &'checked HashMap<Span, Type>,
    constants: &'checked HashMap<Span, i128>,
    loops: Vec<LoopContext>,
    external_names: HashSet<String>,
}

struct LoopContext {
    scope_count: usize,
    step: Option<Box<Statement>>,
}

impl Emitter<'_> {
    fn type_declaration(&mut self, ty: Type, declared: &mut HashSet<Type>) {
        if !declared.insert(ty) {
            return;
        }
        match ty {
            Type::Pointer(id) => {
                let pointer = self.types.pointers[id];
                self.type_declaration(pointer.pointee, declared);
                self.line(&format!(
                    "typedef {}{} *crust_ptr_{id};",
                    c_type(pointer.pointee),
                    if pointer.mutable { "" } else { " const" }
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
                    "typedef {} crust_function_{id}({parameters});",
                    c_type(function.result)
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
                self.line(&format!("static inline {ty} crust_rt_u{bits}_{name}({ty} a, {ty} b, size_t line, size_t column) {{"));
                self.indent += 1;
                self.line("(void)line; (void)column;");
                if name == "div" || name == "rem" {
                    let reason = if name == "div" {
                        "division by zero"
                    } else {
                        "remainder by zero"
                    };
                    self.line(&format!(
                        "if (b == 0) {{ crust_rt_fail(\"{reason}\", line, column); }}"
                    ));
                }
                self.line(&format!("return ({ty})(a {symbol} b);"));
                self.indent -= 1;
                self.line("}");
            }
            self.line(&format!("static inline {ty} crust_rt_u{bits}_neg({ty} value, size_t line, size_t column) {{ (void)line; (void)column; return ({ty})(0 - value); }}"));
        }
    }

    fn struct_definitions(&mut self) {
        for ty in &self.types.order {
            if let Type::Array(id) = ty {
                let array = self.types.arrays[*id];
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
            for field in &info.fields {
                self.line(&format!("{} crust_field_{};", c_type(field.ty), field.name));
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
                for field in info.fields.iter().rev() {
                    self.drop_value(field.ty, &format!("value->crust_field_{}", field.name));
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
        self.cleanup_scope(self.scopes.len() - 1);
        self.scopes.pop();
        self.indent -= 1;
        self.line("}");
    }

    fn statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Unsafe(block) => self.block(block),
            StatementKind::Local {
                name,
                ty,
                initializer,
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
            StatementKind::Assign { name, value } => {
                // Finish the right side first, including a possible move of the
                // old value, before dropping the destination's previous owner.
                let value = self.value_expression(value);
                let local = self.local(name);
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
                        self.temporary(target_type.borrowed(true).unwrap(), &format!("&({place})"));
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
                let common = target_type
                    .integer()
                    .unwrap()
                    .common(self.expression_type(value).integer().unwrap());
                let place = self.place(target);
                let address =
                    self.temporary(target_type.borrowed(true).unwrap(), &format!("&({place})"));
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
                let context = self.loops.last().expect("checked loop control");
                let scope_count = context.scope_count;
                let step = context.step.clone();
                for index in (scope_count..self.scopes.len()).rev() {
                    self.cleanup_scope(index);
                }
                if matches!(statement.kind, StatementKind::Continue) {
                    if let Some(step) = step {
                        let inner_scopes = self.scopes.split_off(scope_count);
                        self.statement(&step);
                        self.scopes.extend(inner_scopes);
                    }
                    self.line("continue;");
                } else {
                    self.line("break;");
                }
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
                    scope_count: self.scopes.len(),
                    step: step.clone(),
                });
                self.block(body);
                if let Some(step) = step {
                    self.statement(step);
                }
                self.loops.pop();
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::Empty => self.line(";"),
        }
    }

    fn value_expression(&mut self, expression: &Expression) -> String {
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
        let cast = format!("({})({value})", c_type(to));
        if matches!(from, Type::Pointer(_)) || matches!(to, Type::Pointer(_)) {
            return self.temporary(to, &cast);
        }
        if from == Type::Bool || to == Type::Bool {
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
        let call = match (source.signed(), target.signed()) {
            (true, true) => format!(
                "crust_rt_cast_signed({value}, INT{bits}_MIN, INT{bits}_MAX, {}, {})",
                span.line, span.column
            ),
            (true, false) => format!(
                "crust_rt_cast_unsigned_from_signed({value}, UINT{bits}_MAX, {}, {})",
                span.line, span.column
            ),
            (false, true) => format!(
                "crust_rt_cast_signed_from_unsigned({value}, INT{bits}_MAX, {}, {})",
                span.line, span.column
            ),
            (false, false) => format!(
                "crust_rt_cast_unsigned({value}, UINT{bits}_MAX, {}, {})",
                span.line, span.column
            ),
        };
        return self.temporary(to, &format!("({})({call})", c_type(to)));
    }

    fn operation(
        &self,
        operator: BinaryOperator,
        kind: IntType,
        left: &str,
        right: &str,
        span: Span,
    ) -> String {
        let helper = match operator {
            BinaryOperator::Add => Some("add"),
            BinaryOperator::Subtract => Some("sub"),
            BinaryOperator::Multiply => Some("mul"),
            BinaryOperator::Divide => Some("div"),
            BinaryOperator::Remainder => Some("rem"),
            _ => None,
        };
        if let Some(helper) = helper {
            let family = match kind.promoted() {
                IntType::I32 => "",
                IntType::I64 => "i64_",
                IntType::U32 => "u32_",
                IntType::U64 => "u64_",
                _ => unreachable!("promoted integer type"),
            };
            return format!(
                "crust_rt_{family}{helper}({left}, {right}, {}, {})",
                span.line, span.column
            );
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
        if value < 0 {
            if value == kind.minimum() {
                return format!("INT{bits}_MIN");
            }
            return format!("(-INT{bits}_C({}))", -value);
        }
        return format!(
            "{}INT{bits}_C({value})",
            if kind.signed() { "" } else { "U" }
        );
    }

    // An owning expression result has exactly one consumer: an initializer,
    // assignment, argument, return, or discard. Its temporary is transferred to
    // that consumer; only named owning locals are registered for scope cleanup.
    fn expression(&mut self, expression: &Expression) -> Option<String> {
        if let Some(name) = self.function_values.get(&expression.span) {
            let function = if self.external_names.contains(name) {
                name.clone()
            } else {
                function_name(name)
            };
            return Some(format!("(&{function})"));
        }
        if let Some(value) = self.constants.get(&expression.span) {
            return Some(self.integer_constant(expression, *value));
        }
        match &expression.kind {
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
            ExpressionKind::MethodCall { .. } | ExpressionKind::StaticCall { .. } => {
                let call = self.method_calls[&expression.span].clone();
                return self.expression(&call);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                let ty = self.expression_type(expression);
                let mut values = Vec::new();
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
            ExpressionKind::Integer(_) => unreachable!("checked literal has a constant value"),
            ExpressionKind::Variable(name) => {
                let local = self.local(name);
                let temporary = self.temporary(local.ty, &local.name);
                return Some(temporary);
            }
            ExpressionKind::Call { name, arguments } => {
                if let Some(call) = self.method_calls.get(&expression.span)
                    && matches!(call.kind, ExpressionKind::IndirectCall { .. })
                {
                    return self.expression(&call.clone());
                }
                if name == "move" {
                    let ExpressionKind::Variable(owner) = &arguments[0].kind else {
                        let place = self.place(&arguments[0]);
                        return Some(self.temporary(self.expression_type(expression), &place));
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
                if name == "box_int" {
                    values.push(expression.span.line.to_string());
                    values.push(expression.span.column.to_string());
                }
                let function = if name == "print" {
                    let argument_type = self.expression_type(&arguments[0]);
                    if let Some(length) = self.types.byte_array_length(argument_type) {
                        let value = &values[0];
                        let member = if argument_type.is_borrow() { "->" } else { "." };
                        self.line(&format!(
                            "crust_rt_print_bytes(({value}){member}crust_elements, {length});"
                        ));
                        return None;
                    }
                    match self.expression_type(&arguments[0]).integer().unwrap() {
                        IntType::I64 => String::from("crust_rt_print_i64"),
                        IntType::U32 | IntType::U64 => String::from("crust_rt_print_u64"),
                        _ => function_name(name),
                    }
                } else if self.external_names.contains(name) {
                    name.clone()
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
                    return Some(
                        self.temporary(self.expression_type(expression), &format!("&({place})")),
                    );
                }
                if let UnaryOperator::Borrow { mutable } = operator {
                    let ty = self
                        .place_type(operand)
                        .borrowed(*mutable)
                        .expect("checked borrow target");
                    let place = self.place(operand);
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
                        let family = match self.expression_type(expression).integer().unwrap() {
                            IntType::I32 => "",
                            IntType::I64 => "i64_",
                            IntType::U32 => "u32_",
                            IntType::U64 => "u64_",
                            _ => unreachable!("promoted unary type"),
                        };
                        format!(
                            "crust_rt_{family}neg({value}, {}, {})",
                            expression.span.line, expression.span.column
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
                let kind = if matches!(self.expression_type(left), Type::Pointer(_))
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
                    base_type.borrowed(self.place_mutable(base)).unwrap(),
                    &format!("&({place})"),
                );
                let value = self.value_expression(index);
                let signed = self.expression_type(index).integer().unwrap().signed();
                let check = format!(
                    "crust_rt_index_{}({value}, {length}, {}, {})",
                    if signed { "signed" } else { "unsigned" },
                    index.span.line,
                    index.span.column
                );
                let checked_index =
                    self.temporary(Type::Integer(IntType::named("size_t").unwrap()), &check);
                return format!("({address})->crust_elements[{checked_index}]");
            }
            ExpressionKind::Field { base, name } => {
                return format!("({}).crust_field_{name}", self.place(base));
            }
            ExpressionKind::Variable(name) => {
                return self.local(name).name;
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                let pointer = if operand.is_place() {
                    self.place(operand)
                } else {
                    self.value_expression(operand)
                };
                return format!("*({pointer})");
            }
            _ => unreachable!("checker requires an assignable or borrowable place"),
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
            ExpressionKind::Field { base, name } => {
                return self
                    .types
                    .field(self.place_type(base), name, expression.span)
                    .expect("checked field");
            }
            ExpressionKind::Variable(name) => {
                return self.local(name).ty;
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.place_type(operand) {
                    Type::BoxInt => Type::Integer(IntType::I32),
                    Type::Borrow { pointee, .. } => pointee.ty(),
                    Type::Pointer(id) => self.types.pointers[id].pointee,
                    _ => unreachable!("checker requires a pointer-like value"),
                };
            }
            _ => return self.expression_type(expression),
        }
    }

    fn place_mutable(&self, expression: &Expression) -> bool {
        match &expression.kind {
            ExpressionKind::Variable(name) => return !self.local(name).ty.is_borrow(),
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
                    Type::Borrow { mutable, .. } => mutable,
                    Type::Pointer(id) => self.types.pointers[id].mutable,
                    Type::BoxInt => self.place_mutable(operand),
                    _ => unreachable!(),
                };
            }
            _ => unreachable!(),
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
        self.output.push_str(&"    ".repeat(self.indent));
        self.output.push_str(text);
        self.output.push('\n');
    }
}

const RUNTIME: &str = r#"/* Generated by the Crust prototype compiler. */
#include <stdint.h>
#include <limits.h>
#include <stdbool.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>

static inline void crust_rt_fail(const char *message, size_t line, size_t column)
{
    fprintf(stderr, "runtime error at %zu:%zu: %s\n", line, column, message);
    exit(EXIT_FAILURE);
}

static inline int32_t crust_rt_range(int64_t value, size_t line, size_t column)
{
    if (value < INT32_MIN || value > INT32_MAX) {
        crust_rt_fail("integer overflow", line, column);
    }
    return (int32_t)value;
}

static inline int32_t crust_rt_add(int32_t a, int32_t b, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a + b, line, column);
}

static inline int32_t crust_rt_sub(int32_t a, int32_t b, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a - b, line, column);
}

static inline int32_t crust_rt_mul(int32_t a, int32_t b, size_t line, size_t column)
{
    return crust_rt_range((int64_t)a * b, line, column);
}

static inline int32_t crust_rt_neg(int32_t value, size_t line, size_t column)
{
    return crust_rt_range(-(int64_t)value, line, column);
}

static inline int32_t crust_rt_div(int32_t a, int32_t b, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("division by zero", line, column); }
    if (a == INT32_MIN && b == -1) { crust_rt_fail("integer overflow", line, column); }
    return a / b;
}

static inline int32_t crust_rt_rem(int32_t a, int32_t b, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("remainder by zero", line, column); }
    if (a == INT32_MIN && b == -1) { crust_rt_fail("integer overflow", line, column); }
    return a % b;
}

static inline void crust_rt_print(int32_t value)
{
    printf("%" PRId32 "\n", value);
}

static inline int32_t *crust_rt_box_int(int32_t value, size_t line, size_t column)
{
    int32_t *owner = (int32_t *)malloc(sizeof(int32_t));
    if (owner == NULL) { crust_rt_fail("out of memory", line, column); }
    *owner = value;
    return owner;
}

static inline void crust_rt_drop(int32_t **owner)
{
    free(*owner);
    *owner = NULL;
}

"#;
const NUMERIC_RUNTIME: &str = include_str!("runtime_numbers.h");
