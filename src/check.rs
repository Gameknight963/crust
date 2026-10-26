use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::types::Types;

// Code generation accepts only a program that has passed this checker.
pub(crate) struct CheckedProgram {
    pub program: Program,
    pub types: Types,
    pub analyses: HashMap<String, FunctionAnalysis>,
}

#[derive(Default)]
pub(crate) struct FunctionAnalysis {
    pub function_values: HashMap<Span, String>,
    pub expression_types: HashMap<Span, Type>,
    // Lowered calls retain receiver-first evaluation and ordinary call rules.
    pub method_calls: HashMap<Span, Expression>,
    pub conversions: HashMap<Span, Type>,
    pub constants: HashMap<Span, i128>,
}

#[derive(Clone, PartialEq, Eq)]
struct Signature {
    link_name: Option<String>,
    variadic: bool,
    requires_unsafe: bool,
    external: bool,
    return_type: Type,
    parameters: Vec<Type>,
    return_borrow: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Uninitialized,
    Available,
    Moved,
    MaybeMoved,
}

#[derive(Clone, Copy)]
struct Variable {
    ty: Type,
    state: State,
    reference: Option<usize>,
}

type Scopes = Vec<HashMap<String, Variable>>;

struct Place {
    ty: Type,
    mutable: bool,
    root: (usize, String),
    fields: Vec<String>,
    via: Option<usize>,
}

#[derive(Clone)]
struct Loan {
    id: usize,
    root: (usize, String),
    fields: Vec<String>,
    mutable: bool,
    parent: Option<usize>,
    scope: Option<usize>,
}

struct LoopContext {
    scope_count: usize,
    breaks: Vec<Scopes>,
    continues: Vec<(Scopes, Span)>,
}

pub(crate) fn check(program: Program) -> Result<CheckedProgram, Diagnostic> {
    let mut types = Types::build(&program)?;
    let mut analyses = HashMap::new();
    let mut signatures = HashMap::new();
    let mut native_signatures = HashMap::new();
    for (name, return_type, parameters) in [
        ("print", Type::Void, vec![Type::Integer(IntType::I32)]),
        ("box_int", Type::BoxInt, vec![Type::Integer(IntType::I32)]),
        ("move", Type::BoxInt, vec![Type::BoxInt]),
    ] {
        signatures.insert(
            name.to_owned(),
            Signature {
                link_name: None,
                variadic: false,
                requires_unsafe: false,
                external: false,
                return_type,
                parameters,
                return_borrow: None,
            },
        );
    }

    // Signatures are known before bodies, allowing forward calls and recursion.
    for function in &program.functions {
        if (function.return_type != Type::Void && !types.is_sized(function.return_type))
            || function
                .parameters
                .iter()
                .any(|parameter| !types.is_sized(parameter.ty))
        {
            return Err(Diagnostic::new(
                function.span,
                "function values require complete object types. Use pointers for opaque structs",
            ));
        }
        if function.external
            && (function.return_type.is_borrow()
                || types.owns(function.return_type)
                || matches!(function.return_type, Type::Array(_))
                || function.parameters.iter().any(|parameter| {
                    parameter.ty.is_borrow()
                        || types.owns(parameter.ty)
                        || matches!(parameter.ty, Type::Array(_))
                }))
        {
            return Err(Diagnostic::new(
                function.span,
                "external functions require C values and raw pointers; pass arrays by pointer",
            ));
        }
        if types.enumerator(&function.name).is_some() {
            return Err(Diagnostic::new(
                function.span,
                "function name conflicts with an enum value",
            ));
        }
        if let Some(method) = function
            .name
            .strip_prefix('@')
            .or_else(|| function.name.strip_prefix('#'))
        {
            let (id, name) = method.split_once('_').unwrap();
            let id = id.parse::<usize>().unwrap();
            let info = &types.structs[id];
            let other = if function.name.starts_with('@') {
                static_method_name(id, name)
            } else {
                method_name(id, name)
            };
            if signatures.contains_key(&other) {
                return Err(Diagnostic::new(
                    function.span,
                    format!("method '{name}' is already defined"),
                ));
            }
            if info.fields.iter().any(|field| field.name == name) {
                return Err(Diagnostic::new(
                    function.span,
                    format!(
                        "method '{name}' conflicts with a field in struct '{}'",
                        info.name
                    ),
                ));
            }
        }
        let return_borrow = if let Type::Borrow { mutable, .. } = function.return_type {
            let borrowed: Vec<_> = function
                .parameters
                .iter()
                .enumerate()
                .filter(|(_, parameter)| parameter.ty.is_borrow())
                .collect();
            if borrowed.len() != 1 {
                return Err(Diagnostic::new(
                    function.span,
                    "borrowed return lifetime contracts require exactly one borrowed parameter",
                ));
            }
            let (index, parameter) = borrowed[0];
            if mutable && !matches!(parameter.ty, Type::Borrow { mutable: true, .. }) {
                return Err(Diagnostic::new(
                    function.span,
                    "a mutable borrowed return requires a mutable borrowed parameter",
                ));
            }
            Some(index)
        } else {
            None
        };
        let signature = Signature {
            link_name: function.link_name.clone(),
            variadic: function.variadic,
            requires_unsafe: function.requires_unsafe,
            external: function.external,
            return_type: function.return_type,
            return_borrow,
            parameters: function
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect(),
        };
        if let Some(symbol) = &function.link_name {
            if let Some(previous) = native_signatures.get(symbol)
                && previous != &signature
            {
                return Err(Diagnostic::new(
                    function.span,
                    format!("conflicting declarations for C symbol '{symbol}'"),
                ));
            }
            native_signatures.insert(symbol.clone(), signature.clone());
        }
        if let Some(previous) = signatures.get(&function.name) {
            if previous.external && signature.external {
                if previous == &signature {
                    continue;
                }
                return Err(Diagnostic::new(
                    function.span,
                    format!("conflicting C declarations for '{}'", function.name),
                ));
            }
            return Err(Diagnostic::new(
                function.span,
                format!("{} is already defined or reserved", function.description()),
            ));
        }
        signatures.insert(function.name.clone(), signature);
    }

    for constraint in &program.interface_checks {
        for requirement in &constraint.requirements {
            let expected: Vec<_> = requirement
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect();
            let satisfied = signatures.get(&requirement.name).is_some_and(|signature| {
                signature.return_type == requirement.return_type
                    && signature.variadic == requirement.variadic
                    && signature.parameters == expected
                    && (!signature.requires_unsafe || requirement.requires_unsafe)
            });
            if !satisfied {
                return Err(Diagnostic::new(
                    constraint.span,
                    format!(
                        "type '{}' does not satisfy interface '{}': missing or incompatible {}",
                        types.describe(constraint.ty),
                        constraint.name,
                        requirement.description()
                    ),
                ));
            }
        }
    }

    match signatures.get("main") {
        Some(signature)
            if signature.return_type == Type::Integer(IntType::I32)
                && !signature.external
                && !signature.requires_unsafe
                && signature.parameters.is_empty() => {}
        Some(_) => {
            let function = program
                .functions
                .iter()
                .find(|function| function.name == "main")
                .unwrap();
            return Err(Diagnostic::new(
                function.span,
                "entry point must be 'int main(void)' or 'int main()'",
            ));
        }
        None => {
            return Err(Diagnostic::new(
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    column: 1,
                },
                "program needs an 'int main(void)' entry point",
            ));
        }
    }

    for function in &program.functions {
        if function.external {
            continue;
        }
        let mut analysis = FunctionAnalysis::default();
        let mut checker = FunctionChecker {
            signatures: &signatures,
            scopes: vec![HashMap::new()],
            return_type: function.return_type,
            return_borrow_root: signatures[&function.name]
                .return_borrow
                .map(|index| (0, function.parameters[index].name.clone())),
            loans: Vec::new(),
            next_loan: 0,
            references: HashMap::new(),
            types: &mut types,
            expression_types: &mut analysis.expression_types,
            method_calls: &mut analysis.method_calls,
            loops: Vec::new(),
            conversions: &mut analysis.conversions,
            constants: &mut analysis.constants,
            unevaluated: false,
            unsafe_depth: usize::from(function.requires_unsafe),
            current_struct: if let Some(method) = function
                .name
                .strip_prefix('@')
                .or_else(|| function.name.strip_prefix('#'))
            {
                Some(method.split_once('_').unwrap().0.parse().unwrap())
            } else {
                function
                    .name
                    .strip_prefix('~')
                    .map(|id| id.parse().unwrap())
            },
            function_values: &mut analysis.function_values,
        };
        for (index, parameter) in function.parameters.iter().enumerate() {
            if parameter.name == "this"
                && !(index == 0
                    && (function.name.starts_with('@') || function.name.starts_with('~')))
            {
                return Err(Diagnostic::new(
                    parameter.span,
                    "this is reserved for the instance receiver",
                ));
            }
            checker.declare(
                &parameter.name,
                parameter.ty,
                State::Available,
                parameter.span,
            )?;
        }

        let returns = checker.statements(&function.body.statements)?;
        if function.return_type != Type::Void && !returns {
            return Err(Diagnostic::new(
                function.span,
                format!(
                    "{} must return a value on every path",
                    function.description()
                ),
            ));
        }
        analyses.insert(function.name.clone(), analysis);
    }

    return Ok(CheckedProgram {
        program,
        types,
        analyses,
    });
}

struct FunctionChecker<'program> {
    function_values: &'program mut HashMap<Span, String>,
    signatures: &'program HashMap<String, Signature>,
    scopes: Scopes,
    return_type: Type,
    return_borrow_root: Option<(usize, String)>,
    // Argument loans last through the call. Local references retain their
    // loans until the binding's block ends, including their reborrow ancestry.
    loans: Vec<Loan>,
    next_loan: usize,
    references: HashMap<Span, usize>,
    types: &'program mut Types,
    expression_types: &'program mut HashMap<Span, Type>,
    method_calls: &'program mut HashMap<Span, Expression>,
    loops: Vec<LoopContext>,
    conversions: &'program mut HashMap<Span, Type>,
    constants: &'program mut HashMap<Span, i128>,
    unevaluated: bool,
    unsafe_depth: usize,
    current_struct: Option<usize>,
}

impl FunctionChecker<'_> {
    fn statements(&mut self, statements: &[Statement]) -> Result<bool, Diagnostic> {
        let mut returns = false;
        for statement in statements {
            let statement_returns = self.statement(statement)?;
            returns = returns || statement_returns;
        }
        return Ok(returns);
    }

    fn block(&mut self, block: &Block) -> Result<bool, Diagnostic> {
        self.scopes.push(HashMap::new());
        let returns = self.statements(&block.statements)?;
        let scope = self.scopes.len() - 1;
        self.loans.retain(|loan| loan.scope != Some(scope));
        self.scopes.pop();
        return Ok(returns);
    }

    fn statement(&mut self, statement: &Statement) -> Result<bool, Diagnostic> {
        let result = self.statement_inner(statement);
        self.release_temporaries();
        return result;
    }

    fn release_temporaries(&mut self) {
        self.loans.retain(|loan| loan.scope.is_some());
    }

    fn adopt_reference(&mut self, id: usize, scope: usize) {
        let mut current = Some(id);
        while let Some(id) = current {
            let loan = self
                .loans
                .iter_mut()
                .find(|loan| loan.id == id)
                .expect("reference has a live loan");
            if loan.scope.is_some() {
                break;
            }
            loan.scope = Some(scope);
            current = loan.parent;
        }
    }

    fn statement_inner(&mut self, statement: &Statement) -> Result<bool, Diagnostic> {
        match &statement.kind {
            StatementKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let result = self.block(block);
                self.unsafe_depth -= 1;
                return result;
            }
            StatementKind::Local {
                name,
                ty,
                initializer,
            } => {
                if name == "this" {
                    return Err(Diagnostic::new(
                        statement.span,
                        "this is reserved for the instance receiver",
                    ));
                }
                self.declare(name, *ty, State::Uninitialized, statement.span)?;
                self.require_type(initializer, *ty)?;
                if ty.is_borrow() {
                    let id = self.references[&initializer.span];
                    let scope = self.scopes.len() - 1;
                    self.adopt_reference(id, scope);
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .get_mut(name)
                        .unwrap()
                        .reference = Some(id);
                }
                self.set_state(name, State::Available);
            }
            StatementKind::Assign { name, value } => {
                // Assignment may reinitialize an owner that has been moved.
                let target = self.lookup(name, statement.span)?;
                if target.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        statement.span,
                        "borrow bindings cannot be rebound; assign through the pointee instead",
                    ));
                }
                self.require_type(value, target.ty)?;
                self.access(&self.root(name), &[], true, None, statement.span)?;
                self.set_state(name, State::Available);
            }
            StatementKind::CompoundAssign {
                target,
                value,
                operator,
            } => {
                let place = self.checked_place(target)?;
                if !place.mutable || !matches!(place.ty, Type::Integer(_) | Type::Pointer(_)) {
                    return Err(Diagnostic::new(
                        target.span,
                        "compound assignment requires a mutable integer place",
                    ));
                }
                let previous_loans = self.loans.len();
                // Reads such as n += n are allowed. Prevent mutation or moves
                // that could invalidate a captured destination during the RHS.
                self.borrow(
                    place.root.clone(),
                    place.fields.clone(),
                    false,
                    place.via,
                    target.span,
                )?;
                let actual = self.expression(value)?;
                if matches!(place.ty, Type::Pointer(_)) {
                    if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                        return Err(Diagnostic::new(
                            statement.span,
                            "pointer compound assignments support += and -=",
                        ));
                    }
                    self.pointer_offset(place.ty, value, actual)?;
                } else {
                    let common = self
                        .integer(place.ty, target.span)?
                        .common(self.integer(actual, value.span)?);
                    self.convert(value, actual, Type::Integer(common))?;
                }
                self.loans.truncate(previous_loans);
                self.access(&place.root, &place.fields, true, place.via, target.span)?;
            }
            StatementKind::Store { target, value } => {
                let target_type = self.receiver_type(target)?;
                self.require_type(value, target_type)?;
                let place = self.checked_place(target)?;
                if !place.mutable || place.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        target.span,
                        "cannot assign through a shared borrow or rebind a borrow",
                    ));
                }
                self.access(&place.root, &place.fields, true, place.via, target.span)?;
            }
            StatementKind::Expression(expression) => {
                let ty = self.expression(expression)?;
                if ty.is_borrow() {
                    return Err(Diagnostic::new(
                        expression.span,
                        "a borrow must initialize a local or be passed directly to a function",
                    ));
                }
            }
            StatementKind::Return(value) => {
                match (self.return_type, value) {
                    (Type::Void, None) => {}
                    (Type::Void, Some(_)) => {
                        return Err(Diagnostic::new(
                            statement.span,
                            "a void function must use 'return;' without a value",
                        ));
                    }
                    (ty, Some(expression)) => {
                        self.require_type(expression, ty)?;
                        if let Some(expected_root) = &self.return_borrow_root {
                            let id = self.references[&expression.span];
                            let loan = self
                                .loans
                                .iter()
                                .find(|loan| loan.id == id)
                                .expect("returned reference has a loan");
                            if loan.root != *expected_root {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "returned borrow must originate from the function's borrowed parameter",
                                ));
                            }
                        }
                    }
                    (_, None) => {
                        return Err(Diagnostic::new(
                            statement.span,
                            "a non-void function must return a value",
                        ));
                    }
                }
                return Ok(true);
            }
            StatementKind::Block(block) => {
                return self.block(block);
            }
            StatementKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.condition(condition)?;
                let before = self.scopes.clone();
                let then_returns = self.block(then_block)?;
                let then_state = self.scopes.clone();
                self.scopes = before;
                let else_returns = match else_block {
                    Some(block) => self.block(block)?,
                    None => false,
                };
                let else_state = self.scopes.clone();

                // A branch ending in return/break/continue doesn't contribute
                // to the fallthrough state after this if.
                self.scopes = match (then_returns, else_returns) {
                    (true, false) => else_state,
                    (false, true) => then_state,
                    _ => merge(&then_state, &else_state),
                };
                return Ok(then_returns && else_returns);
            }
            StatementKind::Break | StatementKind::Continue => {
                let Some(context) = self.loops.last_mut() else {
                    return Err(Diagnostic::new(
                        statement.span,
                        "break and continue require an enclosing loop",
                    ));
                };
                let state = self.scopes[..context.scope_count].to_vec();
                if matches!(statement.kind, StatementKind::Break) {
                    context.breaks.push(state);
                } else {
                    context.continues.push((state, statement.span));
                }
                return Ok(true);
            }
            StatementKind::While {
                condition,
                body,
                step,
            } => {
                let before = self.scopes.clone();
                self.condition(condition)?;
                if !same_owner_states(&before, &self.scopes, self.types) {
                    return Err(Diagnostic::new(
                        condition.span,
                        "a loop condition cannot consume an outer owner",
                    ));
                }
                let after_condition = self.scopes.clone();
                self.loops.push(LoopContext {
                    scope_count: self.scopes.len(),
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                let body_terminates = self.block(body)?;
                let mut context = self.loops.pop().expect("loop context");
                if !body_terminates {
                    context.continues.push((self.scopes.clone(), body.span));
                }
                if context.continues.is_empty() {
                    // Validate and record even an unreachable step so its
                    // generated expressions have checked types and callees.
                    self.scopes = after_condition.clone();
                    if let Some(step) = step {
                        self.statement(step)?;
                    }
                }
                for (state, span) in context.continues {
                    // These snapshots exclude body-local scopes, so the for
                    // step resolves header variables even after a shadowing local.
                    self.scopes = state;
                    if let Some(step) = step {
                        self.statement(step)?;
                    }
                    if !same_owner_states(&after_condition, &self.scopes, self.types) {
                        return Err(Diagnostic::new(
                            span,
                            "a loop must restore moved outer owners before the next iteration",
                        ));
                    }
                }
                self.scopes = after_condition;
                for state in context.breaks {
                    self.scopes = merge(&self.scopes, &state);
                }
            }
            StatementKind::Empty => {}
        }
        return Ok(false);
    }

    fn expression(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        let ty = self.expression_inner(expression)?;
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn expression_inner(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        if let Some(value) = constant_integer(expression) {
            let kind = if IntType::I32.fits(value) {
                IntType::I32
            } else if IntType::I64.fits(value) {
                IntType::I64
            } else if IntType::U64.fits(value) {
                IntType::U64
            } else {
                return Err(Diagnostic::new(
                    expression.span,
                    "integer literal exceeds the 64-bit range",
                ));
            };
            self.constants.insert(expression.span, value);
            return Ok(Type::Integer(kind));
        }
        match &expression.kind {
            ExpressionKind::Storage { pointer, value } => {
                self.require_unsafe(expression.span, "initializing or destroying raw storage")?;
                let ty = self.expression(pointer)?;
                let Type::Pointer(id) = ty else {
                    return Err(Diagnostic::new(
                        pointer.span,
                        "storage operations require a mutable raw pointer",
                    ));
                };
                let destination = self.types.pointers[id];
                if !destination.mutable || !self.types.is_sized(destination.pointee) {
                    return Err(Diagnostic::new(
                        pointer.span,
                        "storage operations require a mutable pointer to a complete object type",
                    ));
                }
                if let Some(value) = value {
                    self.require_type(value, destination.pointee)?;
                }
                return Ok(Type::Void);
            }
            ExpressionKind::IndirectCall { callee, arguments } => {
                let previous_loans = self.loans.len();
                let ty = self.expression(callee)?;
                let signature = self.types.callback(ty).cloned().ok_or_else(|| {
                    Diagnostic::new(callee.span, "indirect calls require a function pointer")
                })?;
                if arguments.len() < signature.parameters.len()
                    || (!signature.variadic && arguments.len() != signature.parameters.len())
                {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "callback expects {}{} argument(s), got {}",
                            if signature.variadic { "at least " } else { "" },
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }
                self.require_unsafe(expression.span, "calling a function pointer")?;
                for (argument, expected) in arguments.iter().zip(&signature.parameters) {
                    self.require_type(argument, *expected)?;
                }
                self.variadic_arguments(&arguments[signature.parameters.len()..])?;
                self.loans.truncate(previous_loans);
                return Ok(signature.result);
            }
            ExpressionKind::StaticReference { id, name } => {
                return self.static_reference(*id, name, expression.span);
            }
            ExpressionKind::EnumValue { id, name } => {
                let variant = self.types.enums[*id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            expression.span,
                            format!(
                                "enum '{}' has no value '{name}'",
                                self.types.enum_names[*id]
                            ),
                        )
                    })?;
                self.constants.insert(expression.span, variant.value);
                return Ok(Type::Enum(*id));
            }
            ExpressionKind::StringLiteral { id, .. } => return Ok(Type::Array(*id)),
            ExpressionKind::Cast { ty, operand } => {
                if *ty == Type::Void {
                    let previous_loans = self.loans.len();
                    self.expression(operand)?;
                    self.loans.truncate(previous_loans);
                    return Ok(Type::Void);
                }
                if matches!(ty, Type::Pointer(_)) {
                    self.require_unsafe(expression.span, "raw pointer casts")?;
                    let actual = self.expression(operand)?;
                    if !matches!(actual, Type::Pointer(_) | Type::Borrow { .. })
                        && !matches!(actual, Type::Integer(kind) if kind.bits() as u32 == usize::BITS)
                        && constant_integer(operand) != Some(0)
                    {
                        return Err(Diagnostic::new(
                            operand.span,
                            "raw pointer casts require a pointer, borrow, pointer-sized integer, or zero",
                        ));
                    }
                    self.conversions.insert(operand.span, *ty);
                    return Ok(*ty);
                }
                if self.types.integer_representation(*ty).is_none() {
                    return Err(Diagnostic::new(
                        expression.span,
                        "casts require a numeric type, enum, or void",
                    ));
                }
                if constant_integer(operand).is_some() {
                    let representation =
                        Type::Integer(self.types.integer_representation(*ty).unwrap());
                    if *ty == Type::Bool {
                        self.require_type(operand, *ty)?;
                    } else {
                        self.require_type(operand, representation)?;
                        self.conversions.insert(operand.span, *ty);
                    }
                } else {
                    let actual = self.expression(operand)?;
                    if matches!(actual, Type::Pointer(_)) {
                        self.require_unsafe(expression.span, "raw pointer casts")?;
                        if *ty != Type::Bool
                            && !matches!(ty, Type::Integer(kind) if kind.bits() as u32 == usize::BITS)
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "a raw pointer can be cast to bool or a pointer-sized integer",
                            ));
                        }
                        self.conversions.insert(operand.span, *ty);
                        return Ok(*ty);
                    }
                    if self.types.integer_representation(actual).is_none() {
                        return Err(Diagnostic::new(
                            operand.span,
                            "casts require a numeric or enum operand",
                        ));
                    }
                    self.conversions.insert(operand.span, *ty);
                }
                return Ok(*ty);
            }
            ExpressionKind::SizeofType(ty) | ExpressionKind::Alignof(ty) => {
                if *ty == Type::Void {
                    return Err(Diagnostic::new(
                        expression.span,
                        "void has no object size or alignment",
                    ));
                }
                if !self.types.is_sized(*ty) {
                    return Err(Diagnostic::new(
                        expression.span,
                        "size and alignment queries require a complete object type",
                    ));
                }
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::SizeofValue(operand) => {
                let previous = self.unevaluated;
                self.unevaluated = true;
                let result = self.expression(operand);
                self.unevaluated = previous;
                if result? == Type::Void {
                    return Err(Diagnostic::new(expression.span, "void has no object size"));
                }
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::Boolean(_) => return Ok(Type::Bool),
            ExpressionKind::StaticCall {
                id,
                name,
                arguments,
            } => {
                let key = static_method_name(*id, name);
                let signature = self.signatures.get(&key).ok_or_else(|| {
                    Diagnostic::new(
                        expression.span,
                        format!(
                            "unknown static method '{name}' on struct '{}'",
                            self.types.structs[*id].name
                        ),
                    )
                })?;
                if arguments.len() != signature.parameters.len() {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "static method '{name}' expects {} argument(s), got {}",
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }
                let call = Expression {
                    span: expression.span,
                    kind: ExpressionKind::Call {
                        name: key,
                        arguments: arguments.clone(),
                    },
                };
                let ty = self.expression(&call)?;
                self.method_calls.insert(expression.span, call);
                return Ok(ty);
            }
            ExpressionKind::MethodCall {
                receiver,
                name,
                arguments,
            } => {
                let ty = self.receiver_type(receiver)?;
                let Type::Struct(id) = ty else {
                    return Err(Diagnostic::new(
                        receiver.span,
                        "a method receiver must be a struct value; use '->' through a struct borrow",
                    ));
                };
                if self.types.structs[id]
                    .fields
                    .iter()
                    .any(|field| field.name == *name)
                {
                    let field_type = self.field_type(ty, name, expression.span)?;
                    if self.types.callback(field_type).is_some() {
                        let call = Expression {
                            span: expression.span,
                            kind: ExpressionKind::IndirectCall {
                                callee: Box::new(Expression {
                                    span: Span {
                                        end: expression.span.start,
                                        ..expression.span
                                    },
                                    kind: ExpressionKind::Field {
                                        base: receiver.clone(),
                                        name: name.clone(),
                                    },
                                }),
                                arguments: arguments.clone(),
                            },
                        };
                        let result = self.expression(&call)?;
                        self.method_calls.insert(expression.span, call);
                        return Ok(result);
                    }
                }
                let key = method_name(id, name);
                let signature = self.signatures.get(&key).ok_or_else(|| {
                    Diagnostic::new(
                        expression.span,
                        format!("unknown method '{name}' on {}", self.types.describe(ty)),
                    )
                })?;
                if arguments.len() + 1 != signature.parameters.len() {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "method '{name}' expects {} argument(s), got {}",
                            signature.parameters.len() - 1,
                            arguments.len()
                        ),
                    ));
                }
                let receiver = match signature.parameters[0] {
                    Type::Borrow { mutable, .. } => Expression {
                        span: receiver.span,
                        kind: ExpressionKind::Unary {
                            operator: UnaryOperator::Borrow { mutable },
                            operand: receiver.clone(),
                        },
                    },
                    Type::Struct(_) => receiver.as_ref().clone(),
                    _ => unreachable!("method has a struct receiver"),
                };
                let mut lowered_arguments = vec![receiver];
                lowered_arguments.extend(arguments.iter().cloned());
                let call = Expression {
                    span: expression.span,
                    kind: ExpressionKind::Call {
                        name: key,
                        arguments: lowered_arguments,
                    },
                };
                let result = self.expression(&call)?;
                self.method_calls.insert(expression.span, call);
                return Ok(result);
            }
            ExpressionKind::StructLiteral { .. } => {
                return self.struct_literal(expression, None);
            }
            ExpressionKind::ArrayLiteral { .. } => {
                return self.array_literal(expression, None);
            }
            ExpressionKind::Field { .. } | ExpressionKind::Index { .. } => {
                let place = self.checked_place(expression)?;
                self.access(
                    &place.root,
                    &place.fields,
                    false,
                    place.via,
                    expression.span,
                )?;
                if self.types.owns(place.ty) && !self.unevaluated {
                    return Err(Diagnostic::new(
                        expression.span,
                        "an owning field cannot be copied; moving individual fields is not supported yet",
                    ));
                }
                return Ok(place.ty);
            }
            ExpressionKind::Integer(_) => unreachable!("literal handled above"),
            ExpressionKind::Variable(name) => {
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && let Some((id, variant)) = self.types.enumerator(name)
                {
                    self.constants.insert(expression.span, variant.value);
                    return Ok(Type::Enum(id));
                }
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    return self.function_reference(name, expression.span);
                }
                let variable = self.available(name, expression.span)?;
                if self.types.owns(variable.ty) && !self.unevaluated {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!("owner '{name}' cannot be copied; use move({name}) to transfer it"),
                    ));
                }
                match variable.ty {
                    Type::Borrow { mutable, .. } => {
                        let place = self.place(expression)?;
                        let id = self.borrow(
                            place.root,
                            place.fields,
                            mutable,
                            place.via,
                            expression.span,
                        )?;
                        self.references.insert(expression.span, id);
                    }
                    _ => self.access(&self.root(name), &[], false, None, expression.span)?,
                }
                return Ok(variable.ty);
            }
            ExpressionKind::Call { name, arguments } => {
                if self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                {
                    let call = Expression {
                        span: expression.span,
                        kind: ExpressionKind::IndirectCall {
                            callee: Box::new(Expression {
                                span: Span {
                                    end: expression.span.start,
                                    ..expression.span
                                },
                                kind: ExpressionKind::Variable(name.clone()),
                            }),
                            arguments: arguments.clone(),
                        },
                    };
                    let variable = self.lookup(name, expression.span)?;
                    if self.types.callback(variable.ty).is_some() {
                        let result = self.expression(&call)?;
                        self.method_calls.insert(expression.span, call);
                        return Ok(result);
                    }
                    return Err(Diagnostic::new(
                        expression.span,
                        format!("variable '{name}' is not callable"),
                    ));
                }
                let signature = match self.signatures.get(name) {
                    Some(signature) => signature.clone(),
                    None => {
                        return Err(Diagnostic::new(
                            expression.span,
                            format!("unknown function '{name}'"),
                        ));
                    }
                };
                if arguments.len() < signature.parameters.len()
                    || (!signature.variadic && arguments.len() != signature.parameters.len())
                {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "function '{name}' expects {}{} argument(s), got {}",
                            if signature.variadic { "at least " } else { "" },
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }

                if signature.external {
                    self.require_unsafe(expression.span, "calling an external C function")?;
                } else if signature.requires_unsafe {
                    self.require_unsafe(expression.span, "calling an unsafe function")?;
                }

                if name == "move" {
                    if let ExpressionKind::Variable(owner) = &arguments[0].kind {
                        let variable = self.available(owner, arguments[0].span)?;
                        if !self.types.owns(variable.ty) {
                            if variable.ty.is_borrow() {
                                return Err(Diagnostic::new(
                                    arguments[0].span,
                                    "borrow bindings use reborrows rather than move",
                                ));
                            }
                            return self.expression(&arguments[0]);
                        }
                        self.access(&self.root(owner), &[], true, None, arguments[0].span)?;
                        self.set_state(owner, State::Moved);
                        return Ok(variable.ty);
                    }
                    let pointer = match &arguments[0].kind {
                        ExpressionKind::Unary {
                            operator: UnaryOperator::Dereference,
                            operand,
                        } => Some(operand.as_ref()),
                        ExpressionKind::Index { base, .. } => Some(base.as_ref()),
                        _ => None,
                    };
                    if let Some(pointer) = pointer
                        && matches!(self.receiver_type(pointer)?, Type::Pointer(_))
                    {
                        self.require_unsafe(expression.span, "moving a value out of raw storage")?;
                        let place = self.checked_place(&arguments[0])?;
                        if !self.types.is_sized(place.ty) {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "raw storage moves require a complete object type",
                            ));
                        }
                        let owning = self.types.owns(place.ty);
                        if owning && !place.mutable {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "moving an owner requires mutable raw storage",
                            ));
                        }
                        self.access(
                            &place.root,
                            &place.fields,
                            owning,
                            place.via,
                            arguments[0].span,
                        )?;
                        self.expression_types.insert(arguments[0].span, place.ty);
                        return Ok(place.ty);
                    }
                    return Err(Diagnostic::new(
                        arguments[0].span,
                        "move requires the name of a whole variable or a whole value in raw storage; moving individual fields is not supported yet",
                    ));
                }

                if name == "print" {
                    let previous_loans = self.loans.len();
                    let actual = self.expression(&arguments[0])?;
                    if self.types.byte_array_length(actual).is_none() {
                        self.integer(actual, arguments[0].span)?;
                    }
                    self.loans.truncate(previous_loans);
                    return Ok(Type::Void);
                }

                // This ordering is also the language's evaluation order.
                let previous_loans = self.loans.len();
                for (argument, expected) in arguments.iter().zip(&signature.parameters) {
                    self.require_type(argument, *expected)?;
                }
                self.variadic_arguments(&arguments[signature.parameters.len()..])?;
                let returned = signature
                    .return_borrow
                    .filter(|_| !self.unevaluated)
                    .map(|index| {
                        let id = self.references[&arguments[index].span];
                        self.loans
                            .iter()
                            .find(|loan| loan.id == id)
                            .expect("borrowed argument has a loan")
                            .clone()
                    });
                let returned = returned.map(|mut source| {
                    while let Some(parent) = source.parent {
                        if self.loans[..previous_loans]
                            .iter()
                            .any(|loan| loan.id == parent)
                        {
                            break;
                        }
                        source.parent = self
                            .loans
                            .iter()
                            .find(|loan| loan.id == parent)
                            .and_then(|loan| loan.parent);
                    }
                    source
                });
                self.loans.truncate(previous_loans);
                if let Some(source) = returned {
                    let Type::Borrow { mutable, .. } = signature.return_type else {
                        unreachable!()
                    };
                    let id = self.borrow(
                        source.root,
                        source.fields,
                        mutable,
                        source.parent,
                        expression.span,
                    )?;
                    self.references.insert(expression.span, id);
                }
                return Ok(signature.return_type);
            }
            ExpressionKind::Unary { operator, operand } => {
                if *operator == UnaryOperator::Address {
                    if let ExpressionKind::StaticReference { id, name } = &operand.kind {
                        return self.static_reference(*id, name, expression.span);
                    }
                    if let ExpressionKind::Variable(name) = &operand.kind
                        && !self
                            .scopes
                            .iter()
                            .rev()
                            .any(|scope| scope.contains_key(name))
                        && self.signatures.contains_key(name)
                    {
                        return self.function_reference(name, expression.span);
                    }
                    self.require_unsafe(expression.span, "creating a raw pointer")?;
                    let place = self.checked_place(operand)?;
                    if place.ty.is_borrow() {
                        return Err(Diagnostic::new(
                            operand.span,
                            "raw pointers cannot point to borrow bindings",
                        ));
                    }
                    self.access(
                        &place.root,
                        &place.fields,
                        false,
                        place.via,
                        expression.span,
                    )?;
                    return Ok(self.types.pointer_to(place.ty, place.mutable));
                }
                if let UnaryOperator::Borrow { mutable } = operator {
                    let place = self.checked_place(operand)?;
                    if *mutable && !place.mutable {
                        return Err(Diagnostic::new(
                            operand.span,
                            "cannot create a mutable borrow through a shared borrow",
                        ));
                    }
                    let ty = place.ty.borrowed(*mutable).ok_or_else(|| {
                        Diagnostic::new(
                            operand.span,
                            "borrows of borrows are not supported; reborrow the pointee instead",
                        )
                    })?;
                    let id = self.borrow(
                        place.root,
                        place.fields,
                        *mutable,
                        place.via,
                        expression.span,
                    )?;
                    self.references.insert(expression.span, id);
                    return Ok(ty);
                }
                if *operator == UnaryOperator::Dereference {
                    let place = self.checked_place(expression)?;
                    if matches!(place.ty, Type::Function(_)) {
                        self.access(
                            &place.root,
                            &place.fields,
                            false,
                            place.via,
                            expression.span,
                        )?;
                        return Ok(self.types.pointer_to(place.ty, true));
                    }
                    if !self.types.is_sized(place.ty) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "an incomplete object cannot be read as a value",
                        ));
                    }
                    self.access(
                        &place.root,
                        &place.fields,
                        false,
                        place.via,
                        expression.span,
                    )?;
                    if self.types.owns(place.ty) && !self.unevaluated {
                        return Err(Diagnostic::new(
                            expression.span,
                            "a borrowed owner cannot be copied or moved out; read it through another dereference",
                        ));
                    }
                    return Ok(place.ty);
                }
                let actual = self.expression(operand)?;
                if *operator == UnaryOperator::Not && matches!(actual, Type::Pointer(_)) {
                    return Ok(Type::Integer(IntType::I32));
                }
                let kind = self.integer(actual, operand.span)?.promoted();
                if *operator == UnaryOperator::Not {
                    return Ok(Type::Integer(IntType::I32));
                }
                self.convert(operand, actual, Type::Integer(kind))?;
                return Ok(Type::Integer(kind));
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left_type = self.expression(left)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    self.scalar_condition(left_type, left.span)?;
                    let before_right = self.scopes.clone();
                    let right_type = self.expression(right)?;
                    self.scalar_condition(right_type, right.span)?;
                    self.scopes = merge(&before_right, &self.scopes);
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(left_type, Type::Pointer(_)) {
                    let right_type = self.expression(right)?;
                    if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                        && right_type.integer().is_some()
                    {
                        self.pointer_offset(left_type, right, right_type)?;
                        return Ok(left_type);
                    }
                    if *operator == BinaryOperator::Subtract
                        && matches!(right_type, Type::Pointer(_))
                    {
                        self.require_unsafe(expression.span, "raw pointer subtraction")?;
                        let Type::Pointer(left_id) = left_type else {
                            unreachable!()
                        };
                        let Type::Pointer(right_id) = right_type else {
                            unreachable!()
                        };
                        if !self.types.is_sized(self.types.pointers[left_id].pointee)
                            || self.types.pointers[left_id].pointee
                                != self.types.pointers[right_id].pointee
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "pointer subtraction requires matching object types",
                            ));
                        }
                        return Ok(Type::Integer(IntType::named("ptrdiff_t").unwrap()));
                    }
                    if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "raw pointers currently support equality comparisons",
                        ));
                    }
                    let common = if let (Type::Pointer(left_id), Type::Pointer(right_id)) =
                        (left_type, right_type)
                    {
                        let a = self.types.pointers[left_id];
                        let b = self.types.pointers[right_id];
                        if a.pointee != b.pointee
                            && a.pointee != Type::Void
                            && b.pointee != Type::Void
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "pointer comparisons require compatible pointee types",
                            ));
                        }
                        self.types.pointer_to(
                            if a.pointee == Type::Void || b.pointee == Type::Void {
                                Type::Void
                            } else {
                                a.pointee
                            },
                            a.mutable && b.mutable,
                        )
                    } else {
                        left_type
                    };
                    self.convert(left, left_type, common)?;
                    self.convert(right, right_type, common)?;
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(left_type, Type::Enum(_)) {
                    let right_type = self.expression(right)?;
                    if left_type != right_type
                        || !matches!(
                            operator,
                            BinaryOperator::Equal
                                | BinaryOperator::NotEqual
                                | BinaryOperator::Less
                                | BinaryOperator::LessEqual
                                | BinaryOperator::Greater
                                | BinaryOperator::GreaterEqual
                        )
                    {
                        return Err(Diagnostic::new(
                            expression.span,
                            "enum comparisons require values of the same enum type; arithmetic requires an explicit integer cast",
                        ));
                    }
                    return Ok(Type::Integer(IntType::I32));
                }
                let left_kind = self.integer(left_type, left.span)?;
                let right_type = self.expression(right)?;
                if matches!(right_type, Type::Pointer(_)) {
                    if *operator == BinaryOperator::Add {
                        self.pointer_offset(right_type, left, left_type)?;
                        return Ok(right_type);
                    }
                    if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "raw pointers currently support equality comparisons",
                        ));
                    }
                    self.convert(left, left_type, right_type)?;
                    return Ok(Type::Integer(IntType::I32));
                }
                let right_kind = self.integer(right_type, right.span)?;
                let common = left_kind.common(right_kind);
                self.convert(left, left_type, Type::Integer(common))?;
                self.convert(right, right_type, Type::Integer(common))?;
                return Ok(Type::Integer(
                    if matches!(
                        operator,
                        BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual
                    ) {
                        IntType::I32
                    } else {
                        common
                    },
                ));
            }
        }
    }

    fn require_type(&mut self, expression: &Expression, expected: Type) -> Result<(), Diagnostic> {
        if let ExpressionKind::StringLiteral { bytes, .. } = &expression.kind
            && let Type::Array(_) = expected
        {
            let length = self.types.byte_array_length(expected).ok_or_else(|| {
                Diagnostic::new(
                    expression.span,
                    "string literals require a char or uint8_t array",
                )
            })?;
            if bytes.len() >= length {
                return Err(Diagnostic::new(
                    expression.span,
                    "string literal and its terminating zero do not fit the array",
                ));
            }
            self.expression_types.insert(expression.span, expected);
            return Ok(());
        }
        if let Type::Integer(kind) = expected
            && let Some(value) = constant_integer(expression)
        {
            if !kind.fits(value) {
                return Err(Diagnostic::new(
                    expression.span,
                    format!(
                        "integer literal must fit {}-bit {}",
                        kind.bits(),
                        if kind.signed() {
                            "signed integer"
                        } else {
                            "unsigned integer"
                        }
                    ),
                ));
            }
            self.constants.insert(expression.span, value);
            self.expression_types.insert(expression.span, expected);
            return Ok(());
        }
        let actual = if matches!(expression.kind, ExpressionKind::ArrayLiteral { .. }) {
            self.array_literal(expression, Some(expected))?
        } else if matches!(
            expression.kind,
            ExpressionKind::StructLiteral { id: None, .. }
        ) {
            self.struct_literal(expression, Some(expected))?
        } else {
            self.expression(expression)?
        };
        return self.convert(expression, actual, expected);
    }

    fn function_reference(&mut self, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if matches!(name, "print" | "box_int" | "move") {
            return Err(Diagnostic::new(
                span,
                "compiler builtins do not have callback addresses",
            ));
        }
        let signature = self.signatures[name].clone();
        let function_type = self.types.function_type(
            signature.return_type,
            signature.parameters,
            signature.variadic,
            span,
        )?;
        self.function_values.insert(span, name.to_owned());
        return Ok(self.types.pointer_to(function_type, true));
    }

    fn variadic_arguments(&mut self, arguments: &[Expression]) -> Result<(), Diagnostic> {
        for argument in arguments {
            let ty = self.expression(argument)?;
            if ty.is_borrow()
                || self.types.owns(ty)
                || !self.types.is_sized(ty)
                || matches!(ty, Type::Array(_))
            {
                return Err(Diagnostic::new(
                    argument.span,
                    "variadic arguments require C values or raw pointers",
                ));
            }
            if let Some(kind) = self.types.integer_representation(ty) {
                let promoted = Type::Integer(kind.promoted());
                if promoted != ty {
                    self.conversions.insert(argument.span, promoted);
                }
            }
        }
        return Ok(());
    }

    fn static_reference(&mut self, id: usize, name: &str, span: Span) -> Result<Type, Diagnostic> {
        let key = static_method_name(id, name);
        if !self.signatures.contains_key(&key) {
            return Err(Diagnostic::new(
                span,
                format!("unknown static method '{name}'"),
            ));
        }
        return self.function_reference(&key, span);
    }

    fn convert(
        &mut self,
        expression: &Expression,
        actual: Type,
        expected: Type,
    ) -> Result<(), Diagnostic> {
        if actual == expected {
            return Ok(());
        }
        if let Type::Pointer(target) = expected {
            let compatible = if let Type::Pointer(source) = actual {
                let source = self.types.pointers[source];
                let target = self.types.pointers[target];
                (source.pointee == target.pointee
                    || (target.pointee == Type::Void
                        && !matches!(source.pointee, Type::Function(_))))
                    && (source.mutable || !target.mutable)
            } else {
                constant_integer(expression) == Some(0)
            };
            if compatible {
                self.conversions.insert(expression.span, expected);
                return Ok(());
            }
        }
        if actual.integer().is_some() && expected.integer().is_some() {
            self.conversions.insert(expression.span, expected);
            return Ok(());
        }
        return Err(Diagnostic::new(
            expression.span,
            format!(
                "expected an {} value, found {}",
                self.types.describe(expected),
                self.types.describe(actual)
            ),
        ));
    }

    fn integer(&self, ty: Type, span: Span) -> Result<IntType, Diagnostic> {
        return ty.integer().ok_or_else(|| {
            Diagnostic::new(
                span,
                format!(
                    "expected an integer or bool value, found {}",
                    self.types.describe(ty)
                ),
            )
        });
    }

    fn condition(&mut self, expression: &Expression) -> Result<(), Diagnostic> {
        let ty = self.expression(expression)?;
        self.scalar_condition(ty, expression.span)?;
        self.release_temporaries();
        return Ok(());
    }

    fn scalar_condition(&self, ty: Type, span: Span) -> Result<(), Diagnostic> {
        if !matches!(ty, Type::Pointer(_)) {
            self.integer(ty, span)?;
        }
        return Ok(());
    }

    fn require_unsafe(&self, span: Span, operation: &str) -> Result<(), Diagnostic> {
        if self.unsafe_depth == 0 && !self.unevaluated {
            return Err(Diagnostic::new(
                span,
                format!("{operation} requires an unsafe block"),
            ));
        }
        return Ok(());
    }

    fn pointer_offset(
        &mut self,
        pointer: Type,
        offset: &Expression,
        offset_type: Type,
    ) -> Result<(), Diagnostic> {
        self.require_unsafe(offset.span, "raw pointer arithmetic or indexing")?;
        let Type::Pointer(id) = pointer else {
            unreachable!()
        };
        if !self.types.is_sized(self.types.pointers[id].pointee) {
            return Err(Diagnostic::new(
                offset.span,
                "pointer arithmetic and indexing require a complete object type",
            ));
        }
        self.integer(offset_type, offset.span)?;
        return self.convert(
            offset,
            offset_type,
            Type::Integer(IntType::named("ptrdiff_t").unwrap()),
        );
    }

    // Resolve the receiver's type without evaluating it. In particular, probing
    // move(value).method() must not move value before checking the actual call.
    fn receiver_type(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        if let Some(value) = constant_integer(expression) {
            return Ok(Type::Integer(if IntType::I32.fits(value) {
                IntType::I32
            } else if IntType::I64.fits(value) {
                IntType::I64
            } else {
                IntType::U64
            }));
        }
        match &expression.kind {
            ExpressionKind::Storage { .. } => return Ok(Type::Void),
            ExpressionKind::StaticReference { id, name } => {
                return self.static_reference(*id, name, expression.span);
            }
            ExpressionKind::IndirectCall { callee, .. } => {
                let ty = self.receiver_type(callee)?;
                let function = if let Type::Function(id) = ty {
                    Some(&self.types.function_types[id])
                } else {
                    self.types.callback(ty)
                };
                return function.map(|function| function.result).ok_or_else(|| {
                    Diagnostic::new(callee.span, "indirect calls require a function pointer")
                });
            }
            ExpressionKind::Boolean(_) => return Ok(Type::Bool),
            ExpressionKind::SizeofType(_)
            | ExpressionKind::SizeofValue(_)
            | ExpressionKind::Alignof(_) => {
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::StringLiteral { id, .. } => return Ok(Type::Array(*id)),
            ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } => {
                if let ExpressionKind::StaticReference { id, name } = &operand.kind {
                    return self.static_reference(*id, name, expression.span);
                }
                if let ExpressionKind::Variable(name) = &operand.kind
                    && !self
                        .scopes
                        .iter()
                        .rev()
                        .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    return self.function_reference(name, expression.span);
                }
                let pointee = self.receiver_type(operand)?;
                let mutable = self.probe_mutability(operand)?;
                return Ok(self.types.pointer_to(pointee, mutable));
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let a = self.receiver_type(left)?;
                let b = self.receiver_type(right)?;
                if matches!(
                    operator,
                    BinaryOperator::Equal
                        | BinaryOperator::NotEqual
                        | BinaryOperator::Less
                        | BinaryOperator::LessEqual
                        | BinaryOperator::Greater
                        | BinaryOperator::GreaterEqual
                        | BinaryOperator::And
                        | BinaryOperator::Or
                ) {
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if matches!(a, Type::Pointer(_)) {
                        return Ok(
                            if *operator == BinaryOperator::Subtract
                                && matches!(b, Type::Pointer(_))
                            {
                                Type::Integer(IntType::named("ptrdiff_t").unwrap())
                            } else {
                                a
                            },
                        );
                    }
                    if *operator == BinaryOperator::Add && matches!(b, Type::Pointer(_)) {
                        return Ok(b);
                    }
                }
                return Ok(Type::Integer(
                    self.integer(a, left.span)?
                        .common(self.integer(b, right.span)?),
                ));
            }
            ExpressionKind::Cast { ty, .. } => return Ok(*ty),
            ExpressionKind::Variable(name) => {
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    return self.function_reference(name, expression.span);
                }
                return Ok(self.available(name, expression.span)?.ty);
            }
            ExpressionKind::Field { base, name } => {
                let ty = self.receiver_type(base)?;
                return self.field_type(ty, name, expression.span);
            }
            ExpressionKind::Index { base, .. } => {
                return match self.receiver_type(base)? {
                    Type::Array(id) => Ok(self.types.arrays[id].element),
                    Type::Pointer(id) => Ok(self.types.pointers[id].pointee),
                    _ => Err(Diagnostic::new(
                        base.span,
                        "indexing requires an array or raw pointer",
                    )),
                };
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.receiver_type(operand)? {
                    Type::BoxInt => Ok(Type::Integer(IntType::I32)),
                    Type::Borrow { pointee, .. } => Ok(pointee.ty()),
                    Type::Pointer(id) => Ok(self.types.pointers[id].pointee),
                    _ => Err(Diagnostic::new(
                        expression.span,
                        "dereference needs a BoxInt or a borrow",
                    )),
                };
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Borrow { mutable },
                operand,
            } => {
                return self
                    .receiver_type(operand)?
                    .borrowed(*mutable)
                    .ok_or_else(|| Diagnostic::new(expression.span, "invalid borrow type"));
            }
            ExpressionKind::Unary { operator, operand } => {
                if *operator == UnaryOperator::Not {
                    return Ok(Type::Integer(IntType::I32));
                }
                let ty = self.receiver_type(operand)?;
                return Ok(Type::Integer(self.integer(ty, operand.span)?.promoted()));
            }
            ExpressionKind::StaticCall { id, name, .. } => {
                return self
                    .signatures
                    .get(&static_method_name(*id, name))
                    .map(|signature| signature.return_type)
                    .ok_or_else(|| {
                        Diagnostic::new(expression.span, format!("unknown static method '{name}'"))
                    });
            }
            ExpressionKind::Call { name, arguments } if name == "move" && arguments.len() == 1 => {
                return self.receiver_type(&arguments[0]);
            }
            ExpressionKind::Call { name, .. } => {
                if let Some(variable) = self.scopes.iter().rev().find_map(|scope| scope.get(name)) {
                    return self
                        .types
                        .callback(variable.ty)
                        .map(|signature| signature.result)
                        .ok_or_else(|| {
                            Diagnostic::new(expression.span, "variable is not callable")
                        });
                }
                return self
                    .signatures
                    .get(name)
                    .map(|signature| signature.return_type)
                    .ok_or_else(|| {
                        Diagnostic::new(expression.span, format!("unknown function '{name}'"))
                    });
            }
            ExpressionKind::MethodCall { receiver, name, .. } => {
                if let Type::Struct(id) = self.receiver_type(receiver)? {
                    if self.types.structs[id]
                        .fields
                        .iter()
                        .any(|field| field.name == *name)
                    {
                        let ty = self.field_type(Type::Struct(id), name, expression.span)?;
                        if let Some(signature) = self.types.callback(ty) {
                            return Ok(signature.result);
                        }
                    }
                    if let Some(signature) = self.signatures.get(&method_name(id, name)) {
                        return Ok(signature.return_type);
                    }
                }
                return Err(Diagnostic::new(
                    expression.span,
                    format!("unknown method '{name}'"),
                ));
            }
            ExpressionKind::StructLiteral { id: Some(id), .. } => return Ok(Type::Struct(*id)),
            _ => return Ok(self.place(expression)?.ty),
        }
    }

    fn probe_mutability(&mut self, expression: &Expression) -> Result<bool, Diagnostic> {
        return match &expression.kind {
            ExpressionKind::Variable(name) => {
                Ok(!self.available(name, expression.span)?.ty.is_borrow())
            }
            ExpressionKind::Field { base, .. } => self.probe_mutability(base),
            ExpressionKind::Index { base, .. } => match self.receiver_type(base)? {
                Type::Pointer(id) => Ok(self.types.pointers[id].mutable),
                _ => self.probe_mutability(base),
            },
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => match self.receiver_type(operand)? {
                Type::Borrow { mutable, .. } => Ok(mutable),
                Type::Pointer(id) => Ok(self.types.pointers[id].mutable),
                _ => self.probe_mutability(operand),
            },
            _ => Err(Diagnostic::new(
                expression.span,
                "address target must be a place",
            )),
        };
    }

    fn array_literal(
        &mut self,
        expression: &Expression,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        let ExpressionKind::ArrayLiteral { id, elements } = &expression.kind else {
            unreachable!()
        };
        let id = match (*id, expected) {
            (Some(id), _) | (None, Some(Type::Array(id))) => id,
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "an array initializer needs an array type",
                ));
            }
        };
        let array = self.types.arrays[id];
        if elements.len() != array.length {
            return Err(Diagnostic::new(
                expression.span,
                format!(
                    "array initializer needs {} elements, got {}",
                    array.length,
                    elements.len()
                ),
            ));
        }
        for element in elements {
            self.require_type(element, array.element)?;
        }
        let ty = Type::Array(id);
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn struct_literal(
        &mut self,
        expression: &Expression,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        let ExpressionKind::StructLiteral { id, fields } = &expression.kind else {
            unreachable!();
        };
        let id = match (*id, expected) {
            (Some(id), _) | (None, Some(Type::Struct(id))) => id,
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "a brace initializer needs a struct type",
                ));
            }
        };
        if !self.types.structs[id].complete {
            return Err(Diagnostic::new(
                expression.span,
                "cannot initialize an incomplete struct",
            ));
        }
        let declared = self.types.structs[id].fields.clone();
        let is_union = self.types.structs[id].is_union;
        if is_union {
            self.require_unsafe(expression.span, "initializing a union")?;
            if fields.len() != 1 {
                return Err(Diagnostic::new(
                    expression.span,
                    "a union initializer selects exactly one field",
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::new(
                    field.span,
                    format!("field '{}' is initialized more than once", field.name),
                ));
            }
            let ty = self.field_type(Type::Struct(id), &field.name, field.span)?;
            self.require_type(&field.value, ty)?;
        }
        for field in &declared {
            if !is_union && !seen.contains(field.name.as_str()) {
                return Err(Diagnostic::new(
                    expression.span,
                    format!("missing initializer for field '{}'", field.name),
                ));
            }
        }
        let ty = Type::Struct(id);
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn checked_place(&mut self, expression: &Expression) -> Result<Place, Diagnostic> {
        match &expression.kind {
            ExpressionKind::Index { base, index } => {
                let place = if base.is_place() {
                    self.checked_place(base)?
                } else {
                    self.temporary_pointer_place(base)?
                };
                let element = match place.ty {
                    Type::Array(id) => self.types.arrays[id].element,
                    Type::Pointer(id) => self.types.pointers[id].pointee,
                    _ => {
                        return Err(Diagnostic::new(base.span, "indexing requires an array"));
                    }
                };
                let previous_loans = self.loans.len();
                self.borrow(
                    place.root.clone(),
                    place.fields.clone(),
                    false,
                    place.via,
                    base.span,
                )?;
                let index_type = self.expression(index)?;
                self.integer(index_type, index.span)?;
                if let Type::Array(id) = place.ty {
                    if let Some(value) = constant_integer(index)
                        && (value < 0 || value >= self.types.arrays[id].length as i128)
                    {
                        return Err(Diagnostic::new(index.span, "array index is out of bounds"));
                    }
                } else {
                    self.pointer_offset(place.ty, index, index_type)?;
                }
                self.loans.truncate(previous_loans);
                let mutable = if let Type::Pointer(id) = place.ty {
                    self.types.pointers[id].mutable
                } else {
                    place.mutable
                };
                return Ok(Place {
                    ty: element,
                    mutable,
                    ..place
                });
            }
            ExpressionKind::Field { base, name } => {
                let place = self.checked_place(base)?;
                let ty = self.field_type(place.ty, name, expression.span)?;
                let projection = if matches!(place.ty, Type::Struct(id) if self.types.structs[id].is_union)
                {
                    "*"
                } else {
                    name
                };
                let mut fields = place.fields;
                fields.push(projection.to_owned());
                return Ok(Place {
                    ty,
                    fields,
                    ..place
                });
            }
            ExpressionKind::Variable(_) => return self.place(expression),
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                let place = if operand.is_place() {
                    self.checked_place(operand)?
                } else {
                    self.temporary_pointer_place(operand)?
                };
                let (ty, mutable) = match place.ty {
                    Type::BoxInt => (Type::Integer(IntType::I32), place.mutable),
                    Type::Borrow { pointee, mutable } => (pointee.ty(), mutable),
                    Type::Pointer(id) => {
                        self.require_unsafe(expression.span, "dereferencing a raw pointer")?;
                        let pointer = self.types.pointers[id];
                        if pointer.pointee == Type::Void {
                            return Err(Diagnostic::new(
                                expression.span,
                                "void pointers must be cast before dereferencing",
                            ));
                        }
                        (pointer.pointee, pointer.mutable)
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            expression.span,
                            "dereference needs a BoxInt or a borrow",
                        ));
                    }
                };
                return Ok(Place {
                    ty,
                    mutable,
                    ..place
                });
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "borrow or dereference target must be a named variable, its pointee, or a returned borrow",
                ));
            }
        }
    }

    fn temporary_pointer_place(&mut self, expression: &Expression) -> Result<Place, Diagnostic> {
        let ty = self.expression(expression)?;
        if !ty.is_borrow() && !matches!(ty, Type::Pointer(_)) {
            return Err(Diagnostic::new(
                expression.span,
                "dereference of a temporary requires a returned borrow or raw pointer",
            ));
        }
        if self.unevaluated || matches!(ty, Type::Pointer(_)) {
            return Ok(Place {
                ty,
                mutable: false,
                root: (self.scopes.len(), format!("@{}", expression.span.start)),
                fields: Vec::new(),
                via: None,
            });
        }
        let id = self.references[&expression.span];
        let loan = self
            .loans
            .iter()
            .find(|loan| loan.id == id)
            .expect("returned reference has a loan");
        return Ok(Place {
            ty,
            mutable: false,
            root: loan.root.clone(),
            fields: loan.fields.clone(),
            via: Some(id),
        });
    }

    fn field_type(&self, ty: Type, name: &str, span: Span) -> Result<Type, Diagnostic> {
        let field_type = self.types.field(ty, name, span)?;
        if matches!(ty, Type::Struct(id) if self.types.structs[id].is_union) {
            self.require_unsafe(span, "accessing a union field")?;
        }
        if let Type::Struct(id) = ty
            && self.current_struct != Some(id)
            && self.types.structs[id]
                .fields
                .iter()
                .any(|field| field.name == name && field.private)
        {
            return Err(Diagnostic::new(
                span,
                format!(
                    "field '{name}' is private to struct '{}'",
                    self.types.structs[id].name
                ),
            ));
        }
        return Ok(field_type);
    }

    fn place(&self, expression: &Expression) -> Result<Place, Diagnostic> {
        let ExpressionKind::Variable(name) = &expression.kind else {
            return Err(Diagnostic::new(
                expression.span,
                "expected a named variable",
            ));
        };
        let variable = self.available(name, expression.span)?;
        let (root, fields) = if let Some(id) = variable.reference {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == id)
                .expect("local reference has a live loan");
            (loan.root.clone(), loan.fields.clone())
        } else {
            (self.root(name), Vec::new())
        };
        return Ok(Place {
            ty: variable.ty,
            mutable: !variable.ty.is_borrow(),
            root,
            fields,
            via: variable.reference,
        });
    }

    fn root(&self, name: &str) -> (usize, String) {
        for (index, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return (index, name.to_owned());
            }
        }
        unreachable!("variable is resolved before finding its root");
    }

    fn access(
        &self,
        root: &(usize, String),
        fields: &[String],
        exclusive: bool,
        via: Option<usize>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.unevaluated {
            return Ok(());
        }
        let mut authorized = Vec::new();
        let mut current = via;
        while let Some(id) = current {
            authorized.push(id);
            current = self
                .loans
                .iter()
                .find(|loan| loan.id == id)
                .and_then(|loan| loan.parent);
        }
        if self.loans.iter().any(|loan| {
            &loan.root == root
                && fields_overlap(fields, &loan.fields)
                && !authorized.contains(&loan.id)
                && (exclusive || loan.mutable)
        }) {
            return Err(Diagnostic::new(
                span,
                format!("conflicting access to '{}' while it is borrowed", root.1),
            ));
        }
        return Ok(());
    }

    fn borrow(
        &mut self,
        root: (usize, String),
        fields: Vec<String>,
        mutable: bool,
        parent: Option<usize>,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        if self.unevaluated {
            return Ok(usize::MAX);
        }
        self.access(&root, &fields, mutable, parent, span)?;
        let id = self.next_loan;
        self.next_loan += 1;
        self.loans.push(Loan {
            id,
            root,
            fields,
            mutable,
            parent,
            scope: None,
        });
        return Ok(id);
    }

    fn declare(
        &mut self,
        name: &str,
        ty: Type,
        state: State,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if !self.types.is_sized(ty) {
            return Err(Diagnostic::new(
                span,
                "a value declaration requires a complete object type",
            ));
        }
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(Diagnostic::new(
                span,
                format!("variable '{name}' is already declared in this scope"),
            ));
        }
        scope.insert(
            name.to_owned(),
            Variable {
                ty,
                state,
                reference: None,
            },
        );
        return Ok(());
    }

    fn lookup(&self, name: &str, span: Span) -> Result<Variable, Diagnostic> {
        for scope in self.scopes.iter().rev() {
            if let Some(variable) = scope.get(name) {
                return Ok(*variable);
            }
        }
        return Err(Diagnostic::new(span, format!("unknown variable '{name}'")));
    }

    fn available(&self, name: &str, span: Span) -> Result<Variable, Diagnostic> {
        let variable = self.lookup(name, span)?;
        if self.unevaluated {
            return Ok(variable);
        }
        match variable.state {
            State::Available => {
                return Ok(variable);
            }
            State::Uninitialized => {
                return Err(Diagnostic::new(
                    span,
                    format!("variable '{name}' is read before it is initialized"),
                ));
            }
            State::Moved => {
                return Err(Diagnostic::new(
                    span,
                    format!("owner '{name}' has been moved"),
                ));
            }
            State::MaybeMoved => {
                return Err(Diagnostic::new(
                    span,
                    format!("owner '{name}' may have been moved on another path"),
                ));
            }
        }
    }

    fn set_state(&mut self, name: &str, state: State) {
        if self.unevaluated {
            return;
        }
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable.state = state;
                return;
            }
        }
        unreachable!("variable was resolved before changing its state");
    }
}

fn constant_integer(expression: &Expression) -> Option<i128> {
    return match &expression.kind {
        ExpressionKind::Integer(value) => Some(i128::from(*value)),
        ExpressionKind::Unary {
            operator: UnaryOperator::Plus,
            operand,
        } => match operand.kind {
            ExpressionKind::Integer(value) => Some(i128::from(value)),
            _ => None,
        },
        ExpressionKind::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => match operand.kind {
            ExpressionKind::Integer(value) => Some(-i128::from(value)),
            _ => None,
        },
        _ => None,
    };
}

fn merge(left: &Scopes, right: &Scopes) -> Scopes {
    let mut result = left.clone();
    for (scope, other) in result.iter_mut().zip(right) {
        for (name, variable) in scope {
            if variable.state != other[name].state {
                variable.state = State::MaybeMoved;
            }
        }
    }
    return result;
}

fn same_owner_states(left: &Scopes, right: &Scopes, types: &Types) -> bool {
    for (scope, other) in left.iter().zip(right) {
        for (name, variable) in scope {
            if types.owns(variable.ty) && variable.state != other[name].state {
                return false;
            }
        }
    }
    return true;
}

fn fields_overlap(left: &[String], right: &[String]) -> bool {
    for (a, b) in left.iter().zip(right) {
        if a == "*" || b == "*" {
            return true;
        }
        if a != b {
            return false;
        }
    }
    return true;
}
