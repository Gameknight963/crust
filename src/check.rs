use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::types::Types;

// Code generation accepts only a program that has passed this checker.
pub(crate) struct CheckedProgram {
    pub program: Program,
    pub types: Types,
    pub expression_types: HashMap<Span, Type>,
    // Resolved methods reuse ordinary call checking and generation, including
    // receiver-first evaluation, argument loans, moves, and parameter cleanup.
    pub method_calls: HashMap<Span, Expression>,
}

#[derive(Clone)]
struct Signature {
    return_type: Type,
    parameters: Vec<Type>,
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
}

type Scopes = Vec<HashMap<String, Variable>>;

struct Place {
    ty: Type,
    mutable: bool,
    root: (usize, String),
}

struct Loan {
    root: (usize, String),
    mutable: bool,
}

struct LoopContext {
    scope_count: usize,
    breaks: Vec<Scopes>,
    continues: Vec<(Scopes, Span)>,
}

pub(crate) fn check(program: Program) -> Result<CheckedProgram, Diagnostic> {
    let types = Types::build(&program)?;
    let mut expression_types = HashMap::new();
    let mut method_calls = HashMap::new();
    let mut signatures = HashMap::new();
    for (name, return_type, parameters) in [
        ("print", Type::Void, vec![Type::Int]),
        ("box_int", Type::BoxInt, vec![Type::Int]),
        ("move", Type::BoxInt, vec![Type::BoxInt]),
    ] {
        signatures.insert(
            name.to_owned(),
            Signature {
                return_type,
                parameters,
            },
        );
    }

    // Signatures are known before bodies, allowing forward calls and recursion.
    for function in &program.functions {
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
        if function.return_type.is_borrow() {
            return Err(Diagnostic::new(
                function.span,
                "borrowed return values need lifetime contracts and are not supported yet",
            ));
        }
        if signatures.contains_key(&function.name) {
            return Err(Diagnostic::new(
                function.span,
                format!("{} is already defined or reserved", function.description()),
            ));
        }
        signatures.insert(
            function.name.clone(),
            Signature {
                return_type: function.return_type,
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.ty)
                    .collect(),
            },
        );
    }

    match signatures.get("main") {
        Some(signature)
            if signature.return_type == Type::Int && signature.parameters.is_empty() => {}
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
        let mut checker = FunctionChecker {
            signatures: &signatures,
            scopes: vec![HashMap::new()],
            return_type: function.return_type,
            loans: Vec::new(),
            types: &types,
            expression_types: &mut expression_types,
            method_calls: &mut method_calls,
            loops: Vec::new(),
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
    }

    return Ok(CheckedProgram {
        program,
        types,
        expression_types,
        method_calls,
    });
}

struct FunctionChecker<'program> {
    signatures: &'program HashMap<String, Signature>,
    scopes: Scopes,
    return_type: Type,
    // Loans last through the enclosing call, including evaluation of later
    // arguments. No references can be stored or returned in this first version.
    loans: Vec<Loan>,
    types: &'program Types,
    expression_types: &'program mut HashMap<Span, Type>,
    method_calls: &'program mut HashMap<Span, Expression>,
    loops: Vec<LoopContext>,
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
        self.scopes.pop();
        return Ok(returns);
    }

    fn statement(&mut self, statement: &Statement) -> Result<bool, Diagnostic> {
        match &statement.kind {
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
                if ty.is_borrow() {
                    return Err(Diagnostic::new(
                        statement.span,
                        "borrowed locals are not supported yet; pass borrows directly to functions",
                    ));
                }
                self.declare(name, *ty, State::Uninitialized, statement.span)?;
                self.require_type(initializer, *ty)?;
                self.set_state(name, State::Available);
            }
            StatementKind::Assign { name, value } => {
                // Assignment may reinitialize an owner that has been moved.
                let target = self.lookup(name, statement.span)?;
                if target.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        statement.span,
                        "borrow parameters cannot be rebound; assign through '*parameter' instead",
                    ));
                }
                self.require_type(value, target.ty)?;
                self.access(&self.root(name), true, statement.span)?;
                self.set_state(name, State::Available);
            }
            StatementKind::CompoundAssign { target, value, .. } => {
                let place = self.place(target)?;
                if !place.mutable || place.ty != Type::Int {
                    return Err(Diagnostic::new(
                        target.span,
                        "compound assignment requires a mutable int place",
                    ));
                }
                let previous_loans = self.loans.len();
                // Reads such as n += n are allowed. Prevent mutation or moves
                // that could invalidate a captured destination during the RHS.
                self.borrow(place.root.clone(), false, target.span)?;
                self.require_type(value, Type::Int)?;
                self.loans.truncate(previous_loans);
                self.access(&place.root, true, target.span)?;
            }
            StatementKind::Store { target, value } => {
                let place = self.place(target)?;
                if !place.mutable || place.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        target.span,
                        "cannot assign through a shared borrow or rebind a borrow",
                    ));
                }
                self.require_type(value, place.ty)?;
                // The right side may move the owner of the destination.
                let place = self.place(target)?;
                self.access(&place.root, true, target.span)?;
            }
            StatementKind::Expression(expression) => {
                let ty = self.expression(expression)?;
                if ty.is_borrow() {
                    return Err(Diagnostic::new(
                        expression.span,
                        "a borrow must be passed directly to a function",
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
                    (ty, Some(expression)) => self.require_type(expression, ty)?,
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
                self.require_type(condition, Type::Int)?;
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
                self.require_type(condition, Type::Int)?;
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
        match &expression.kind {
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
            ExpressionKind::Field { .. } => {
                let place = self.place(expression)?;
                self.access(&place.root, false, expression.span)?;
                if self.types.owns(place.ty) {
                    return Err(Diagnostic::new(
                        expression.span,
                        "an owning field cannot be copied; moving individual fields is not supported yet",
                    ));
                }
                return Ok(place.ty);
            }
            ExpressionKind::Integer(value) => {
                if *value > i64::from(i32::MAX) {
                    return Err(Diagnostic::new(
                        expression.span,
                        "int literals must fit signed 32-bit int",
                    ));
                }
            }
            ExpressionKind::Variable(name) => {
                let variable = self.available(name, expression.span)?;
                if self.types.owns(variable.ty) {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!("owner '{name}' cannot be copied; use move({name}) to transfer it"),
                    ));
                }
                match variable.ty {
                    Type::Borrow { mutable, .. } => {
                        self.borrow(self.root(name), mutable, expression.span)?;
                    }
                    _ => self.access(&self.root(name), false, expression.span)?,
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
                if arguments.len() != signature.parameters.len() {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "function '{name}' expects {} argument(s), got {}",
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }

                if name == "move" {
                    if let ExpressionKind::Variable(owner) = &arguments[0].kind {
                        let variable = self.available(owner, arguments[0].span)?;
                        if !self.types.owns(variable.ty) {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "move currently requires a BoxInt owner or an owning struct",
                            ));
                        }
                        self.access(&self.root(owner), true, arguments[0].span)?;
                        self.set_state(owner, State::Moved);
                        return Ok(variable.ty);
                    }
                    return Err(Diagnostic::new(
                        arguments[0].span,
                        "move requires the name of a whole owning variable; moving individual fields is not supported yet",
                    ));
                }

                // This ordering is also the language's evaluation order.
                let previous_loans = self.loans.len();
                for (argument, expected) in arguments.iter().zip(signature.parameters) {
                    self.require_type(argument, expected)?;
                }
                self.loans.truncate(previous_loans);
                return Ok(signature.return_type);
            }
            ExpressionKind::Unary { operator, operand } => {
                if let UnaryOperator::Borrow { mutable } = operator {
                    let place = self.place(operand)?;
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
                    self.borrow(place.root, *mutable, expression.span)?;
                    return Ok(ty);
                }
                if *operator == UnaryOperator::Dereference {
                    let place = self.place(expression)?;
                    self.access(&place.root, false, expression.span)?;
                    if self.types.owns(place.ty) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "a borrowed owner cannot be copied or moved out; read it through another dereference",
                        ));
                    }
                    return Ok(place.ty);
                }
                if *operator == UnaryOperator::Negate
                    && matches!(operand.kind, ExpressionKind::Integer(2147483648))
                {
                    return Ok(Type::Int);
                }
                self.require_type(operand, Type::Int)?;
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                self.require_type(left, Type::Int)?;
                let before_right = self.scopes.clone();
                self.require_type(right, Type::Int)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    // The right operand may not execute.
                    self.scopes = merge(&before_right, &self.scopes);
                }
            }
        }
        return Ok(Type::Int);
    }

    fn require_type(&mut self, expression: &Expression, expected: Type) -> Result<(), Diagnostic> {
        let actual = if matches!(
            expression.kind,
            ExpressionKind::StructLiteral { id: None, .. }
        ) {
            self.struct_literal(expression, Some(expected))?
        } else {
            self.expression(expression)?
        };
        let scalar_conversion = matches!(
            (actual, expected),
            (Type::Int, Type::Bool) | (Type::Bool, Type::Int)
        );
        if actual != expected && !scalar_conversion {
            return Err(Diagnostic::new(
                expression.span,
                format!(
                    "expected an {} value, found {}",
                    self.types.describe(expected),
                    self.types.describe(actual)
                ),
            ));
        }
        return Ok(());
    }

    // Resolve the receiver's type without evaluating it. In particular, probing
    // move(value).method() must not move value before checking the actual call.
    fn receiver_type(&self, expression: &Expression) -> Result<Type, Diagnostic> {
        match &expression.kind {
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
                return self
                    .signatures
                    .get(name)
                    .map(|signature| signature.return_type)
                    .ok_or_else(|| {
                        Diagnostic::new(expression.span, format!("unknown function '{name}'"))
                    });
            }
            ExpressionKind::MethodCall { receiver, name, .. } => {
                if let Type::Struct(id) = self.receiver_type(receiver)?
                    && let Some(signature) = self.signatures.get(&method_name(id, name))
                {
                    return Ok(signature.return_type);
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
        let declared = self.types.structs[id].fields.clone();
        let mut seen = std::collections::HashSet::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::new(
                    field.span,
                    format!("field '{}' is initialized more than once", field.name),
                ));
            }
            let ty = self
                .types
                .field(Type::Struct(id), &field.name, field.span)?;
            self.require_type(&field.value, ty)?;
        }
        for field in &declared {
            if !seen.contains(field.name.as_str()) {
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

    fn place(&self, expression: &Expression) -> Result<Place, Diagnostic> {
        match &expression.kind {
            ExpressionKind::Field { base, name } => {
                let place = self.place(base)?;
                let ty = self.types.field(place.ty, name, expression.span)?;
                return Ok(Place {
                    ty,
                    mutable: place.mutable,
                    root: place.root,
                });
            }
            ExpressionKind::Variable(name) => {
                let variable = self.available(name, expression.span)?;
                return Ok(Place {
                    ty: variable.ty,
                    mutable: !variable.ty.is_borrow(),
                    root: self.root(name),
                });
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                let place = self.place(operand)?;
                let (ty, mutable) = match place.ty {
                    Type::BoxInt => (Type::Int, place.mutable),
                    Type::Borrow { pointee, mutable } => (pointee.ty(), mutable),
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
                    root: place.root,
                });
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "borrow or dereference target must be a named variable or its pointee",
                ));
            }
        }
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
        exclusive: bool,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self
            .loans
            .iter()
            .any(|loan| &loan.root == root && (exclusive || loan.mutable))
        {
            return Err(Diagnostic::new(
                span,
                format!(
                    "conflicting access to '{}' while it is borrowed for this call",
                    root.1
                ),
            ));
        }
        return Ok(());
    }

    fn borrow(
        &mut self,
        root: (usize, String),
        mutable: bool,
        span: Span,
    ) -> Result<(), Diagnostic> {
        self.access(&root, mutable, span)?;
        self.loans.push(Loan { root, mutable });
        return Ok(());
    }

    fn declare(
        &mut self,
        name: &str,
        ty: Type,
        state: State,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(Diagnostic::new(
                span,
                format!("variable '{name}' is already declared in this scope"),
            ));
        }
        scope.insert(name.to_owned(), Variable { ty, state });
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
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable.state = state;
                return;
            }
        }
        unreachable!("variable was resolved before changing its state");
    }
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
