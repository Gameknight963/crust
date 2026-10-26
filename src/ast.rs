use crate::diagnostic::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Void,
    BoxInt,
    Struct(usize),
    Borrow { pointee: Pointee, mutable: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointee {
    Int,
    Bool,
    BoxInt,
    Struct(usize),
}

impl Pointee {
    pub fn ty(self) -> Type {
        return match self {
            Pointee::Int => Type::Int,
            Pointee::Bool => Type::Bool,
            Pointee::BoxInt => Type::BoxInt,
            Pointee::Struct(id) => Type::Struct(id),
        };
    }
}

impl Type {
    pub fn is_borrow(self) -> bool {
        return matches!(self, Type::Borrow { .. });
    }

    pub fn borrowed(self, mutable: bool) -> Option<Type> {
        let pointee = match self {
            Type::Int => Pointee::Int,
            Type::Bool => Pointee::Bool,
            Type::BoxInt => Pointee::BoxInt,
            Type::Struct(id) => Pointee::Struct(id),
            _ => return None,
        };
        return Some(Type::Borrow { pointee, mutable });
    }
}

#[derive(Debug)]
pub struct Program {
    pub struct_names: Vec<(String, Span)>,
    pub structs: Vec<StructDefinition>,
    pub functions: Vec<Function>,
}

#[derive(Debug)]
pub struct StructDefinition {
    pub id: usize,
    pub fields: Vec<Field>,
    pub has_destructor: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub return_type: Type,
    pub parameters: Vec<Parameter>,
    pub body: Block,
    pub span: Span,
}

impl Function {
    pub fn description(&self) -> String {
        if self.name.starts_with('~') {
            return String::from("destructor");
        }
        if let Some(method) = self
            .name
            .strip_prefix('@')
            .or_else(|| self.name.strip_prefix('#'))
        {
            let (_, name) = method.split_once('_').expect("internal method name");
            return format!("method '{name}'");
        }
        return format!("function '{}'", self.name);
    }
}

#[derive(Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StatementKind {
    Local {
        name: String,
        ty: Type,
        initializer: Expression,
    },
    Assign {
        name: String,
        value: Expression,
    },
    Store {
        target: Expression,
        value: Expression,
    },
    CompoundAssign {
        target: Expression,
        operator: BinaryOperator,
        value: Expression,
    },
    Expression(Expression),
    Return(Option<Expression>),
    Break,
    Continue,
    Block(Block),
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expression,
        body: Block,
        step: Option<Box<Statement>>,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExpressionKind {
    Boolean(bool),
    StaticCall {
        id: usize,
        name: String,
        arguments: Vec<Expression>,
    },
    MethodCall {
        receiver: Box<Expression>,
        name: String,
        arguments: Vec<Expression>,
    },
    Integer(i64),
    Variable(String),
    Field {
        base: Box<Expression>,
        name: String,
    },
    StructLiteral {
        id: Option<usize>,
        fields: Vec<FieldInitializer>,
    },
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldInitializer {
    pub name: String,
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,
    Negate,
    Not,
    Dereference,
    Borrow { mutable: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
}

// The '@' cannot occur in a source identifier, keeping methods separate from
// free functions, builtins, and methods on other structs.
pub fn method_name(id: usize, name: &str) -> String {
    return format!("@{id}_{name}");
}

pub fn destructor_name(id: usize) -> String {
    return format!("~{id}");
}

pub fn static_method_name(id: usize, name: &str) -> String {
    return format!("#{id}_{name}");
}
