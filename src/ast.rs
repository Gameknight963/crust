use crate::diagnostic::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Void,
    BoxInt,
    IntBorrow { mutable: bool },
    BoxBorrow { mutable: bool },
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
        };
    }

    pub fn is_borrow(self) -> bool {
        return matches!(self, Type::IntBorrow { .. } | Type::BoxBorrow { .. });
    }
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub return_type: Type,
    pub parameters: Vec<Parameter>,
    pub body: Block,
    pub span: Span,
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

#[derive(Debug)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExpressionKind {
    Integer(i64),
    Variable(String),
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
