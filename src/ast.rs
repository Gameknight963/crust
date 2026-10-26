use crate::diagnostic::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

impl IntType {
    pub fn bits(self) -> u8 {
        return match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 => 64,
        };
    }

    pub fn signed(self) -> bool {
        return matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64);
    }

    pub fn minimum(self) -> i128 {
        return if self.signed() {
            -(1i128 << (self.bits() - 1))
        } else {
            0
        };
    }

    pub fn maximum(self) -> i128 {
        return if self.signed() {
            (1i128 << (self.bits() - 1)) - 1
        } else {
            (1i128 << self.bits()) - 1
        };
    }

    pub fn fits(self, value: i128) -> bool {
        return value >= self.minimum() && value <= self.maximum();
    }

    pub fn promoted(self) -> IntType {
        return if self.bits() < 32 { Self::I32 } else { self };
    }

    pub fn common(self, other: IntType) -> IntType {
        let a = self.promoted();
        let b = other.promoted();
        if a.signed() == b.signed() {
            return if a.bits() >= b.bits() { a } else { b };
        }
        let (signed, unsigned) = if a.signed() { (a, b) } else { (b, a) };
        return if signed.bits() > unsigned.bits() {
            signed
        } else {
            unsigned
        };
    }

    pub fn c_name(self) -> &'static str {
        return match self {
            Self::I8 => "int8_t",
            Self::I16 => "int16_t",
            Self::I32 => "int32_t",
            Self::I64 => "int64_t",
            Self::U8 => "uint8_t",
            Self::U16 => "uint16_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
        };
    }

    pub fn named(name: &str) -> Option<IntType> {
        return Some(match name {
            "int8_t" => Self::I8,
            "int16_t" => Self::I16,
            "int32_t" => Self::I32,
            "int64_t" => Self::I64,
            "uint8_t" => Self::U8,
            "uint16_t" => Self::U16,
            "uint32_t" => Self::U32,
            "uint64_t" => Self::U64,
            "size_t" | "uintptr_t" => {
                if usize::BITS == 64 {
                    Self::U64
                } else {
                    Self::U32
                }
            }
            "ptrdiff_t" | "intptr_t" => {
                if usize::BITS == 64 {
                    Self::I64
                } else {
                    Self::I32
                }
            }
            _ => return None,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Integer(IntType),
    Bool,
    Void,
    BoxInt,
    Struct(usize),
    Array(usize),
    Borrow { pointee: Pointee, mutable: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointee {
    Integer(IntType),
    Bool,
    BoxInt,
    Struct(usize),
    Array(usize),
}

impl Pointee {
    pub fn ty(self) -> Type {
        return match self {
            Pointee::Integer(kind) => Type::Integer(kind),
            Pointee::Bool => Type::Bool,
            Pointee::BoxInt => Type::BoxInt,
            Pointee::Struct(id) => Type::Struct(id),
            Pointee::Array(id) => Type::Array(id),
        };
    }
}

impl Type {
    pub fn is_borrow(self) -> bool {
        return matches!(self, Type::Borrow { .. });
    }

    pub fn borrowed(self, mutable: bool) -> Option<Type> {
        let pointee = match self {
            Type::Integer(kind) => Pointee::Integer(kind),
            Type::Bool => Pointee::Bool,
            Type::BoxInt => Pointee::BoxInt,
            Type::Struct(id) => Pointee::Struct(id),
            Type::Array(id) => Pointee::Array(id),
            _ => return None,
        };
        return Some(Type::Borrow { pointee, mutable });
    }

    pub fn integer(self) -> Option<IntType> {
        return match self {
            Self::Integer(kind) => Some(kind),
            Self::Bool => Some(IntType::I32),
            _ => None,
        };
    }
}

#[derive(Debug)]
pub struct Program {
    pub arrays: Vec<ArrayType>,
    pub struct_names: Vec<(String, Span)>,
    pub structs: Vec<StructDefinition>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayType {
    pub element: Type,
    pub length: usize,
    pub span: Span,
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
    StringLiteral {
        id: usize,
        bytes: Vec<u8>,
    },
    Index {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    ArrayLiteral {
        id: Option<usize>,
        elements: Vec<Expression>,
    },
    Cast {
        ty: Type,
        operand: Box<Expression>,
    },
    SizeofType(Type),
    Alignof(Type),
    SizeofValue(Box<Expression>),
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
    Integer(u64),
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
    BitNot,
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
