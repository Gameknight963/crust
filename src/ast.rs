use crate::diagnostic::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FloatType {
    Float,
    Double,
    LongDouble,
}

impl FloatType {
    pub fn c_name(self) -> &'static str {
        return match self {
            Self::Float => "float",
            Self::Double => "double",
            Self::LongDouble => "long double",
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntType {
    Char,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Long,
    ULong,
    LongLong,
    ULongLong,
}

impl IntType {
    pub fn wrap_constant(self, value: i128) -> i128 {
        let modulus = 1i128 << self.bits();
        let wrapped = value & (modulus - 1);
        return if self.signed() && wrapped > self.maximum() {
            wrapped - modulus
        } else {
            wrapped
        };
    }
    pub fn bits(self) -> u8 {
        return match self {
            Self::Char | Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 => 64,
            Self::Long | Self::ULong => (std::mem::size_of::<std::ffi::c_long>() * 8) as u8,
            Self::LongLong | Self::ULongLong => 64,
        };
    }

    pub fn signed(self) -> bool {
        if self == Self::Char {
            return std::ffi::c_char::MIN != 0;
        }
        return matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Long | Self::LongLong
        );
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
            return if a.rank() >= b.rank() { a } else { b };
        }
        let (signed, unsigned) = if a.signed() { (a, b) } else { (b, a) };
        return if unsigned.rank() >= signed.rank() {
            unsigned
        } else if signed.bits() > unsigned.bits() {
            signed
        } else {
            signed.unsigned()
        };
    }

    pub fn native_long() -> IntType {
        return if std::mem::size_of::<std::ffi::c_long>() == 8 && !cfg!(target_vendor = "apple") {
            Self::I64
        } else {
            Self::Long
        };
    }

    pub fn native_long_long() -> IntType {
        return if Self::native_long() == Self::I64 {
            Self::LongLong
        } else {
            Self::I64
        };
    }

    pub fn unsigned(self) -> IntType {
        return match self {
            Self::Char | Self::I8 => Self::U8,
            Self::I16 => Self::U16,
            Self::I32 => Self::U32,
            Self::I64 => Self::U64,
            Self::Long => Self::ULong,
            Self::LongLong => Self::ULongLong,
            _ => self,
        };
    }

    fn rank(self) -> u8 {
        return match self {
            Self::Char | Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 => 3,
            Self::Long | Self::ULong => 4,
            Self::LongLong | Self::ULongLong => 5,
            Self::I64 | Self::U64 => {
                if Self::native_long() == Self::I64 {
                    4
                } else {
                    5
                }
            }
        };
    }

    pub fn runtime_prefix(self) -> &'static str {
        let promoted = self.promoted();
        return match (promoted.signed(), promoted.bits()) {
            (true, 32) => "",
            (true, 64) => "i64_",
            (false, 32) => "u32_",
            (false, 64) => "u64_",
            _ => unreachable!("promoted integer representation"),
        };
    }

    pub fn c_name(self) -> &'static str {
        return match self {
            Self::Char => "char",
            Self::I8 => "int8_t",
            Self::I16 => "int16_t",
            Self::I32 => "int32_t",
            Self::I64 => "int64_t",
            Self::U8 => "uint8_t",
            Self::U16 => "uint16_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
            Self::Long => "long",
            Self::ULong => "unsigned long",
            Self::LongLong => "long long",
            Self::ULongLong => "unsigned long long",
        };
    }

    pub fn named(name: &str) -> Option<IntType> {
        return Some(match name {
            "char" => Self::Char,
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
                    if std::mem::size_of::<std::ffi::c_long>() == 8 {
                        Self::native_long().unsigned()
                    } else {
                        Self::U64
                    }
                } else {
                    Self::U32
                }
            }
            "ptrdiff_t" | "intptr_t" => {
                if usize::BITS == 64 {
                    if std::mem::size_of::<std::ffi::c_long>() == 8 {
                        Self::native_long()
                    } else {
                        Self::I64
                    }
                } else {
                    Self::I32
                }
            }
            _ => return None,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Float(FloatType),
    Function(usize),
    Integer(IntType),
    Bool,
    Void,
    Struct(usize),
    Array(usize),
    Enum(usize),
    Pointer(usize),
    Borrow {
        pointee: Pointee,
        mutable: bool,
        volatile: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pointee {
    Float(FloatType),
    Slice(usize),
    Integer(IntType),
    Bool,
    Struct(usize),
    Array(usize),
    Enum(usize),
    Pointer(usize),
}

impl Pointee {
    pub fn ty(self) -> Type {
        return match self {
            Pointee::Float(kind) => Type::Float(kind),
            Pointee::Slice(id) => Type::Array(id),
            Pointee::Integer(kind) => Type::Integer(kind),
            Pointee::Bool => Type::Bool,
            Pointee::Struct(id) => Type::Struct(id),
            Pointee::Array(id) => Type::Array(id),
            Pointee::Enum(id) => Type::Enum(id),
            Pointee::Pointer(id) => Type::Pointer(id),
        };
    }
}

impl Type {
    pub fn referent(self) -> Type {
        return match self {
            Type::Borrow { pointee, .. } => pointee.ty(),
            _ => self,
        };
    }

    pub fn is_borrow(self) -> bool {
        return matches!(self, Type::Borrow { .. });
    }

    pub fn borrowed_qualified(self, mutable: bool, volatile: bool) -> Option<Type> {
        let pointee = match self {
            Type::Float(kind) => Pointee::Float(kind),
            Type::Integer(kind) => Pointee::Integer(kind),
            Type::Bool => Pointee::Bool,
            Type::Struct(id) => Pointee::Struct(id),
            Type::Array(id) => Pointee::Array(id),
            Type::Enum(id) => Pointee::Enum(id),
            Type::Pointer(id) => Pointee::Pointer(id),
            _ => return None,
        };
        return Some(Type::Borrow {
            pointee,
            mutable,
            volatile,
        });
    }

    pub fn integer(self) -> Option<IntType> {
        return match self {
            Self::Integer(kind) => Some(kind),
            Self::Bool => Some(IntType::I32),
            _ => None,
        };
    }

    pub fn numeric(self) -> bool {
        return self.integer().is_some() || matches!(self, Self::Float(_));
    }

    pub fn common_numeric(self, other: Type) -> Option<Type> {
        if !self.numeric() || !other.numeric() {
            return None;
        }
        return match (self, other) {
            (Self::Float(a), Self::Float(b)) => Some(Self::Float(a.max(b))),
            (Self::Float(kind), _) | (_, Self::Float(kind)) => Some(Self::Float(kind)),
            _ => Some(Self::Integer(
                self.integer().unwrap().common(other.integer().unwrap()),
            )),
        };
    }
}

#[derive(Debug)]
pub struct Program {
    pub native_headers: Vec<String>,
    pub globals: Vec<ExternalVariable>,
    pub source_declarations: Vec<SourceDeclaration>,
    pub type_references: Vec<TypeReference>,
    pub type_annotations: Vec<(Span, Type)>,
    pub union_tags: std::collections::HashSet<usize>,
    pub native_tags: std::collections::HashMap<usize, String>,
    pub interface_checks: Vec<InterfaceCheck>,
    pub function_types: Vec<FunctionType>,
    pub pointers: Vec<PointerType>,
    pub enum_names: Vec<(String, Span)>,
    pub enums: Vec<EnumDefinition>,
    pub arrays: Vec<ArrayType>,
    pub struct_names: Vec<(String, Span)>,
    pub structs: Vec<StructDefinition>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct ExternalVariable {
    pub external: bool,
    pub function_local: bool,
    pub initializer: Option<Expression>,
    pub constant_values: Vec<(Span, i128)>,
    pub dll_import: bool,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub readonly: bool,
    pub volatile: bool,
    pub link_name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub enum TypeDeclaration {
    Aggregate(usize),
    Enum(usize),
    Source(Span),
}

#[derive(Debug, Clone)]
pub struct TypeReference {
    pub span: Span,
    pub declaration: Option<TypeDeclaration>,
}

#[derive(Debug, Clone, Copy)]
pub enum SourceDeclarationKind {
    Function,
    Struct,
    Union,
    Interface,
    Alias,
    TypeParameter,
    Method,
    Parameter,
}

#[derive(Debug, Clone)]
pub struct SourceDeclaration {
    pub kind: SourceDeclarationKind,
    pub name_span: Span,
    pub header_span: Span,
    pub span: Span,
    pub container: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InterfaceCheck {
    pub name: String,
    pub ty: Type,
    pub span: Span,
    pub requirements: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionType {
    pub calling_convention: CallingConvention,
    pub variadic: bool,
    pub result: Type,
    pub parameters: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallingConvention {
    C,
    Stdcall,
    Fastcall,
    Vectorcall,
}

impl CallingConvention {
    pub fn canonical(self, variadic: bool) -> Self {
        if matches!(self, Self::Stdcall | Self::Fastcall)
            && (variadic || !cfg!(target_arch = "x86"))
        {
            return Self::C;
        }
        return self;
    }

    pub fn c_prefix(self) -> &'static str {
        return match self {
            Self::C if cfg!(windows) => "__cdecl ",
            Self::C => "",
            Self::Stdcall => "__stdcall ",
            Self::Fastcall => "__fastcall ",
            Self::Vectorcall => "__vectorcall ",
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerType {
    pub pointee: Type,
    pub mutable: bool,
    pub volatile: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Qualifiers {
    pub readonly: bool,
    pub volatile: bool,
}

impl Qualifiers {
    pub fn describe(self, ty: Type, name: String) -> String {
        let qualifiers = match (self.readonly, self.volatile) {
            (false, false) => return name,
            (true, false) => "const",
            (false, true) => "volatile",
            (true, true) => "const volatile",
        };
        return if matches!(ty, Type::Pointer(_)) {
            format!("{name} {qualifiers}")
        } else {
            format!("{qualifiers} {name}")
        };
    }
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub scoped: bool,
    pub native_name: Option<String>,
    pub name_span: Span,
    pub header_span: Span,
    pub id: usize,
    pub underlying: IntType,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub value: i128,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayType {
    pub element: Type,
    pub length: usize,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDefinition {
    pub external: bool,
    pub tagged: bool,
    pub name_span: Span,
    pub header_span: Span,
    pub id: usize,
    pub fields: Vec<Field>,
    pub has_destructor: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub readonly: bool,
    pub volatile: bool,
    pub native_name: Option<String>,
    pub declaration_span: Span,
    pub private: bool,
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub specialized: bool,
    pub return_borrows: Option<Vec<(String, Span)>>,
    pub dll_import: bool,
    pub calling_convention: CallingConvention,
    pub name_span: Span,
    pub link_name: Option<String>,
    pub variadic: bool,
    pub requires_unsafe: bool,
    pub external: bool,
    pub name: String,
    pub return_type: Type,
    pub parameters: Vec<Parameter>,
    pub body: Block,
    pub span: Span,
}

impl Function {
    pub fn description(&self) -> String {
        if let Some(local) = self.name.strip_prefix('!') {
            return format!("function '{}'", local.split_once('_').unwrap().1);
        }
        if let Some(generic) = self.name.strip_prefix('$') {
            return format!("function '{}'", generic.split_once('_').unwrap().1);
        }
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

#[derive(Debug, Clone)]
pub struct Parameter {
    pub declaration_span: Span,
    pub synthetic: bool,
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
    StaticLocal {
        name: String,
        name_span: Span,
        global: String,
        ty: Type,
    },
    Switch {
        value: Expression,
        cases: Vec<SwitchCase>,
    },
    Unsafe(Block),
    Local {
        name_span: Span,
        declaration_span: Span,
        name: String,
        ty: Type,
        initializer: Expression,
    },
    Assign {
        name_span: Span,
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
    DoWhile {
        body: Block,
        condition: Expression,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub label: Option<CaseLabel>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum CaseLabel {
    Named { name: String, span: Span },
    Constant { expression: Expression, value: i128 },
}

impl Expression {
    pub fn is_place(&self) -> bool {
        return matches!(
            self.kind,
            ExpressionKind::Variable { .. }
                | ExpressionKind::Field { .. }
                | ExpressionKind::Index { .. }
                | ExpressionKind::Unary {
                    operator: UnaryOperator::Dereference,
                    ..
                }
        );
    }
}

#[derive(Debug, Clone)]
pub enum ExpressionKind {
    Floating {
        spelling: String,
        kind: FloatType,
    },
    Update {
        target: Box<Expression>,
        increment: bool,
        prefix: bool,
    },
    Conditional {
        condition: Box<Expression>,
        then_value: Box<Expression>,
        else_value: Box<Expression>,
    },
    Storage {
        pointer: Box<Expression>,
        value: Option<Box<Expression>>,
    },
    StaticReference {
        name_span: Span,
        id: usize,
        name: String,
    },
    IndirectCall {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
    },
    EnumValue {
        name_span: Span,
        id: usize,
        name: String,
    },
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
        name_span: Span,
        id: usize,
        name: String,
        arguments: Vec<Expression>,
    },
    MethodCall {
        name_span: Span,
        receiver: Box<Expression>,
        name: String,
        arguments: Vec<Expression>,
    },
    Integer(u64),
    TypedInteger {
        value: u64,
        kind: IntType,
    },
    Variable {
        name: String,
        name_span: Span,
    },
    Field {
        name_span: Span,
        base: Box<Expression>,
        name: String,
    },
    StructLiteral {
        id: Option<usize>,
        fields: Vec<FieldInitializer>,
    },
    Call {
        name_span: Span,
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
    pub name_span: Span,
    pub name: String,
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Address,
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
    ShiftLeft,
    ShiftRight,
}

impl BinaryOperator {
    pub fn numeric_symbol(self) -> &'static str {
        return match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            _ => unreachable!("checked numeric operator"),
        };
    }
    pub fn is_shift(self) -> bool {
        return matches!(self, Self::ShiftLeft | Self::ShiftRight);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_conversion_ranks_match_the_native_c_compiler() {
        let types = [
            ("char", IntType::Char),
            ("signed char", IntType::I8),
            ("unsigned char", IntType::U8),
            ("short", IntType::I16),
            ("unsigned short", IntType::U16),
            ("int", IntType::I32),
            ("unsigned int", IntType::U32),
            ("long", IntType::native_long()),
            ("unsigned long", IntType::native_long().unsigned()),
            ("long long", IntType::native_long_long()),
            ("unsigned long long", IntType::native_long_long().unsigned()),
            ("int64_t", IntType::I64),
            ("uint64_t", IntType::U64),
        ];
        let mut source =
            String::from("#include <stdio.h>\n#include <stdint.h>\nint main(void) {\n");
        let mut expected = Vec::new();
        for (left_name, left) in types {
            for (right_name, right) in types {
                source.push_str(&format!("printf(\"%d\\n\", _Generic((({left_name})0 + ({right_name})0), int: 3, unsigned int: -3, long: 4, unsigned long: -4, long long: 5, unsigned long long: -5));\n"));
                let kind = left.common(right);
                expected.push((
                    left_name,
                    right_name,
                    i16::from(kind.rank()) * if kind.signed() { 1 } else { -1 },
                ));
            }
        }
        source.push_str("return 0; }\n");
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/integer-promotion-tests")
            .join(std::process::id().to_string());
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("promotions.c");
        let executable = directory.join(format!("promotions{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&path, source).unwrap();
        crate::native::compile_file(&path, &executable).unwrap();
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(output.status.success());
        let actual: Vec<i16> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| line.parse().unwrap())
            .collect();
        assert_eq!(actual.len(), expected.len());
        for (actual, (left, right, expected)) in actual.into_iter().zip(expected) {
            assert_eq!(
                actual, expected,
                "usual arithmetic conversion for {left} + {right}"
            );
        }
    }
}
