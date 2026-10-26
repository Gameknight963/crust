use std::collections::HashMap;

use crate::ast::*;
use crate::check::CheckedProgram;

pub(crate) fn generate(checked: &CheckedProgram) -> String {
    let program = &checked.program;
    let mut emitter = Emitter {
        output: String::from(RUNTIME),
        indent: 0,
        next_name: 0,
        scopes: Vec::new(),
        return_types: program
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.return_type))
            .collect(),
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

    for function in &program.functions {
        emitter.line(&format!("{};", header(function)));
    }
    for function in &program.functions {
        emitter.line("");
        emitter.line(&header(function));
        let mut scope = Scope::default();
        for (index, parameter) in function.parameters.iter().enumerate() {
            scope.insert(
                parameter.name.clone(),
                Local {
                    name: format!("crust_arg_{index}"),
                    ty: parameter.ty,
                },
            );
        }
        emitter.block_with_scope(&function.body, scope);
    }
    return emitter.output;
}

fn function_name(name: &str) -> String {
    if name == "main" {
        return String::from("main");
    }
    if name == "print" || name == "box_int" {
        return format!("crust_rt_{name}");
    }
    return format!("crust_fn_{name}");
}

fn c_type(ty: Type) -> &'static str {
    return match ty {
        Type::Int => "int32_t",
        Type::Void => "void",
        Type::BoxInt => "int32_t *",
        Type::IntBorrow { mutable: true } => "int32_t *",
        Type::IntBorrow { mutable: false } => "const int32_t *",
        Type::BoxBorrow { mutable: true } => "int32_t **",
        Type::BoxBorrow { mutable: false } => "int32_t * const *",
    };
}

fn header(function: &Function) -> String {
    let result = if function.name == "main" {
        "int"
    } else {
        c_type(function.return_type)
    };
    let parameters = if function.parameters.is_empty() {
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
    return format!("{result} {}({parameters})", function_name(&function.name));
}

#[derive(Clone)]
struct Local {
    name: String,
    ty: Type,
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

struct Emitter {
    output: String,
    indent: usize,
    next_name: usize,
    scopes: Vec<Scope>,
    return_types: HashMap<String, Type>,
}

impl Emitter {
    fn block(&mut self, block: &Block) {
        self.block_with_scope(block, Scope::default());
    }

    fn block_with_scope(&mut self, block: &Block, scope: Scope) {
        self.line("{");
        self.indent += 1;
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
            StatementKind::Local {
                name,
                ty,
                initializer,
            } => {
                let value = self.value_expression(initializer);
                let local = self.fresh_name("local");
                self.line(&format!("{} {local} = {value};", c_type(*ty)));
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        name: local,
                        ty: *ty,
                    },
                );
            }
            StatementKind::Assign { name, value } => {
                // Finish the right side first, including a possible move of the
                // old value, before dropping the destination's previous owner.
                let value = self.value_expression(value);
                let local = self.local(name);
                if local.ty == Type::BoxInt {
                    self.line(&format!("crust_rt_drop(&{});", local.name));
                }
                self.line(&format!("{} = {value};", local.name));
            }
            StatementKind::Store { target, value } => {
                let value = self.value_expression(value);
                let place = self.place(target);
                if self.place_type(target) == Type::BoxInt {
                    self.line(&format!("crust_rt_drop(&({place}));"));
                }
                self.line(&format!("{place} = {value};"));
            }
            StatementKind::Expression(expression) => {
                if let Some(value) = self.expression(expression) {
                    if self.expression_type(expression) == Type::BoxInt {
                        // Discarded owning results still need a destructor.
                        self.line(&format!("crust_rt_drop(&{value});"));
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
            StatementKind::While { condition, body } => {
                self.line("while (1) {");
                self.indent += 1;
                let condition = self.value_expression(condition);
                self.line(&format!("if (!{condition}) {{ break; }}"));
                self.block(body);
                self.indent -= 1;
                self.line("}");
            }
            StatementKind::Empty => self.line(";"),
        }
    }

    fn value_expression(&mut self, expression: &Expression) -> String {
        return self
            .expression(expression)
            .expect("checker requires a non-void value here");
    }

    // An owning expression result has exactly one consumer: an initializer,
    // assignment, argument, return, or discard. Its temporary is transferred to
    // that consumer; only named owning locals are registered for scope cleanup.
    fn expression(&mut self, expression: &Expression) -> Option<String> {
        match &expression.kind {
            ExpressionKind::Integer(value) => {
                return Some(format!("INT32_C({value})"));
            }
            ExpressionKind::Variable(name) => {
                let local = self.local(name);
                let temporary = self.temporary(local.ty, &local.name);
                return Some(temporary);
            }
            ExpressionKind::Call { name, arguments } => {
                if name == "move" {
                    let ExpressionKind::Variable(owner) = &arguments[0].kind else {
                        unreachable!("checker requires an owner name in move");
                    };
                    let local = self.local(owner);
                    let temporary = self.temporary(Type::BoxInt, &local.name);
                    // NULL is an internal drop flag, not a usable language value.
                    self.line(&format!("{} = NULL;", local.name));
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
                let call = format!("{}({})", function_name(name), values.join(", "));
                let ty = self.return_types[name];
                if ty == Type::Void {
                    self.line(&format!("{call};"));
                    return None;
                }
                let value = self.temporary(ty, &call);
                return Some(value);
            }
            ExpressionKind::Unary { operator, operand } => {
                if let UnaryOperator::Borrow { mutable } = operator {
                    let ty = match self.place_type(operand) {
                        Type::Int => Type::IntBorrow { mutable: *mutable },
                        Type::BoxInt => Type::BoxBorrow { mutable: *mutable },
                        _ => unreachable!("checker rejects references to references"),
                    };
                    let place = self.place(operand);
                    let value = self.temporary(ty, &format!("&({place})"));
                    return Some(value);
                }
                if *operator == UnaryOperator::Dereference {
                    let place = self.place(expression);
                    let value = self.temporary(Type::Int, &place);
                    return Some(value);
                }
                if *operator == UnaryOperator::Negate
                    && matches!(operand.kind, ExpressionKind::Integer(2147483648))
                {
                    return Some(String::from("INT32_MIN"));
                }
                let value = self.value_expression(operand);
                let operation = match operator {
                    UnaryOperator::Plus => {
                        return Some(value);
                    }
                    UnaryOperator::Not => format!("!{value}"),
                    UnaryOperator::Negate => format!(
                        "crust_rt_neg({value}, {}, {})",
                        expression.span.line, expression.span.column
                    ),
                    UnaryOperator::Dereference => unreachable!(),
                    UnaryOperator::Borrow { .. } => unreachable!(),
                };
                let temporary = self.temporary(Type::Int, &operation);
                return Some(temporary);
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.value_expression(left);
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    let result = self.temporary(Type::Int, &format!("({left} != 0)"));
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
                let right = self.value_expression(right);
                let helper = match operator {
                    BinaryOperator::Add => Some("add"),
                    BinaryOperator::Subtract => Some("sub"),
                    BinaryOperator::Multiply => Some("mul"),
                    BinaryOperator::Divide => Some("div"),
                    BinaryOperator::Remainder => Some("rem"),
                    _ => None,
                };
                let operation;
                if let Some(helper) = helper {
                    operation = format!(
                        "crust_rt_{helper}({left}, {right}, {}, {})",
                        expression.span.line, expression.span.column
                    );
                } else {
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
                        _ => unreachable!("other binary operators were handled above"),
                    };
                    operation = format!("({left} {symbol} {right})");
                }
                let temporary = self.temporary(Type::Int, &operation);
                return Some(temporary);
            }
        }
    }

    fn expression_type(&self, expression: &Expression) -> Type {
        if let ExpressionKind::Call { name, .. } = &expression.kind {
            return self.return_types[name];
        }
        return Type::Int;
    }

    // Places have no calls or side effects in this subset, so evaluating the
    // right side before resolving an assignment destination is unambiguous.
    fn place(&self, expression: &Expression) -> String {
        match &expression.kind {
            ExpressionKind::Variable(name) => {
                return self.local(name).name;
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return format!("*({})", self.place(operand));
            }
            _ => unreachable!("checker requires an assignable or borrowable place"),
        }
    }

    fn place_type(&self, expression: &Expression) -> Type {
        match &expression.kind {
            ExpressionKind::Variable(name) => {
                return self.local(name).ty;
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.place_type(operand) {
                    Type::BoxInt | Type::IntBorrow { .. } => Type::Int,
                    Type::BoxBorrow { .. } => Type::BoxInt,
                    _ => unreachable!("checker requires a pointer-like value"),
                };
            }
            _ => unreachable!("checker requires a place"),
        }
    }

    fn cleanup_scope(&mut self, index: usize) {
        let scope = &self.scopes[index];
        let owners: Vec<String> = scope
            .declaration_order
            .iter()
            .rev()
            .filter_map(|name| {
                let local = &scope.variables[name];
                if local.ty == Type::BoxInt {
                    return Some(local.name.clone());
                }
                return None;
            })
            .collect();
        for owner in owners {
            self.line(&format!("crust_rt_drop(&{owner});"));
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
