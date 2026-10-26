use crate::diagnostic::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Void,
    BoxInt,
    IntBorrow { mutable: bool },
    BoxBorrow { mutable: bool },
    Struct(usize),
    StructBorrow { id: usize, mutable: bool },
}

impl Type {
    pub fn name(self) -> &'static str {
        return match self {
            Type::Int => "int",
            Type::Void => "void",
            Type::BoxInt => "BoxInt",
            Type::IntBorrow { mutable: true } => "int^",
            Type::IntBorrow { mutable: false } => "const int^",
            Type::BoxBorrow { mutable: true } => "BoxInt^",
            Type::BoxBorrow { mutable: false } => "const BoxInt^",
            Type::Struct(_) => "struct",
            Type::StructBorrow { mutable: true, .. } => "struct^",
            Type::StructBorrow { mutable: false, .. } => "const struct^",
        };
    }

    pub fn is_borrow(self) -> bool {
        return matches!(
            self,
            Type::IntBorrow { .. } | Type::BoxBorrow { .. } | Type::StructBorrow { .. }
        );
    }

    pub fn borrowed(self, mutable: bool) -> Option<Type> {
        return match self {
            Type::Int => Some(Type::IntBorrow { mutable }),
            Type::BoxInt => Some(Type::BoxBorrow { mutable }),
            Type::Struct(id) => Some(Type::StructBorrow { id, mutable }),
            _ => None,
        };
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
        if let Some(method) = self.name.strip_prefix('@') {
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

#[derive(Debug)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug)]
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
    Expression(Expression),
    Return(Option<Expression>),
    Block(Block),
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expression,
        body: Block,
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
