use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{SpannedToken, Token};
use std::collections::{HashMap, HashSet, hash_map::Entry};
mod calling_conventions;
mod constants;
mod file_locals;
mod generics;
mod integers;
mod limits;
mod symbols;

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program, Diagnostic> {
    let mut parser = Parser {
        tokens,
        position: 0,
        struct_names: Vec::new(),
        union_tags: HashSet::new(),
        native_tags: HashMap::new(),
        current_struct: None,
        methods: Vec::new(),
        aliases: HashMap::new(),
        repeated_aliases: Vec::new(),
        resolved_aliases: HashMap::new(),
        resolving_aliases: HashSet::new(),
        value_scopes: vec![HashSet::new()],
        arrays: Vec::new(),
        enum_names: Vec::new(),
        pointers: Vec::new(),
        function_types: Vec::new(),
        generics: generics::Templates::default(),
        pending_greater: None,
        source_declarations: Vec::new(),
        type_references: Vec::new(),
        type_annotations: Vec::new(),
        enum_constants: HashMap::new(),
        enum_representations: HashMap::new(),
        enum_sources: HashMap::new(),
        enumerator_owners: HashMap::new(),
        evaluating_enums: HashSet::new(),
        nesting: 0,
        file_locals: HashMap::new(),
        file_ends: Vec::new(),
    };
    parser.collect_file_locals()?;
    parser.collect_templates()?;
    parser.collect_type_names()?;
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut globals = Vec::new();
    let mut native_headers = Vec::new();

    loop {
        if parser.at(&Token::EndOfFile) {
            if parser.position + 1 == parser.tokens.len() {
                break;
            }
            parser.position += 1;
            continue;
        }
        if parser.skip_template() {
            continue;
        }
        if parser.at(&Token::Extern)
            && matches!(parser.tokens.get(parser.position + 1).map(|token| &token.kind), Some(Token::Identifier(name)) if name == "header")
            && matches!(
                parser
                    .tokens
                    .get(parser.position + 2)
                    .map(|token| &token.kind),
                Some(Token::StringLiteral(_))
            )
        {
            parser.advance();
            parser.advance();
            let token = parser.advance();
            let Token::StringLiteral(bytes) = token.kind else {
                unreachable!();
            };
            let name = String::from_utf8(bytes)
                .map_err(|_| Diagnostic::new(token.span, "native header paths require UTF-8"))?;
            let name = crate::source::header_name(std::path::Path::new(&name))
                .map_err(|error| Diagnostic::new(token.span, error.to_string()))?;
            parser.expect(
                Token::Semicolon,
                "expected ';' after native header reference",
            )?;
            if !native_headers.contains(&name) {
                native_headers.push(name);
            }
        } else if parser.at(&Token::Extern)
            && parser.next_at(&Token::Enum)
            && parser.tokens.get(parser.position + 3).is_some_and(|token| {
                matches!(token.kind, Token::LeftBrace | Token::Colon | Token::Equal)
            })
        {
            let start = parser.advance().span;
            let mut definition = parser.enum_definition(true)?;
            definition.span = start.through(definition.span);
            definition.header_span = start.through(definition.header_span);
            let name = parser.enum_names[definition.id].0.clone();
            if definition.native_name.is_none() {
                definition.native_name = Some(name);
            }
            enums.push(definition);
        } else if parser.consume(&Token::Typedef) {
            parser.typedef_type()?;
            parser.expect(Token::Semicolon, "expected ';' after typedef")?;
        } else if parser.at(&Token::Enum)
            && parser.tokens.get(parser.position + 2).is_some_and(|token| {
                matches!(token.kind, Token::LeftBrace | Token::Colon | Token::Equal)
            })
        {
            enums.push(parser.enum_definition(false)?);
        } else if matches!(parser.current().kind, Token::Struct | Token::Union)
            && parser
                .tokens
                .get(parser.position + 2)
                .is_some_and(|token| matches!(token.kind, Token::Semicolon | Token::Equal))
        {
            let is_union = parser.advance().kind == Token::Union;
            let (name, span) = parser.identifier()?;
            let id = parser.tagged_struct_id(name, span, is_union)?;
            parser.reference_type(span, Type::Struct(id));
            if parser.consume(&Token::Equal) {
                native_tag_name(parser.advance(), if is_union { "union" } else { "struct" })?;
            }
            parser.expect(
                Token::Semicolon,
                "expected ';' after opaque tag declaration",
            )?;
        } else if matches!(parser.current().kind, Token::Struct | Token::Union)
            && parser
                .tokens
                .get(parser.position + 2)
                .is_some_and(|token| token.kind == Token::LeftBrace)
        {
            structs.push(parser.struct_definition()?);
        } else {
            match parser.declaration()? {
                Declaration::Function(function) => functions.push(function),
                Declaration::Global(variable) => globals.push(variable),
            }
        }
    }

    parser.validate_interfaces()?;
    functions.append(&mut parser.methods);
    functions.append(&mut parser.generics.function_definitions);
    structs.append(&mut parser.generics.struct_definitions);
    return Ok(Program {
        native_headers,
        globals,
        source_declarations: parser.source_declarations,
        type_references: parser.type_references,
        type_annotations: parser.type_annotations,
        union_tags: parser.union_tags,
        native_tags: parser.native_tags,
        interface_checks: parser.generics.interface_checks,
        function_types: parser.function_types,
        pointers: parser.pointers,
        enum_names: parser.enum_names,
        enums,
        arrays: parser.arrays,
        functions,
        structs,
        struct_names: parser.struct_names,
    });
}

#[derive(Clone)]
struct Parser {
    file_locals: HashMap<(usize, String), String>,
    file_ends: Vec<usize>,
    nesting: usize,
    enum_constants: HashMap<(usize, String), constants::EnumConstant>,
    enum_representations: HashMap<usize, IntType>,
    enum_sources: HashMap<usize, usize>,
    enumerator_owners: HashMap<String, usize>,
    evaluating_enums: HashSet<usize>,
    source_declarations: Vec<SourceDeclaration>,
    type_references: Vec<TypeReference>,
    type_annotations: Vec<(Span, Type)>,
    generics: generics::Templates,
    pending_greater: Option<SpannedToken>,
    function_types: Vec<FunctionType>,
    pointers: Vec<PointerType>,
    enum_names: Vec<(String, Span)>,
    arrays: Vec<ArrayType>,
    tokens: Vec<SpannedToken>,
    position: usize,
    struct_names: Vec<(String, Span)>,
    union_tags: HashSet<usize>,
    native_tags: HashMap<usize, String>,
    current_struct: Option<usize>,
    methods: Vec<Function>,
    aliases: HashMap<String, Alias>,
    repeated_aliases: Vec<(String, Alias)>,
    resolved_aliases: HashMap<String, Type>,
    resolving_aliases: HashSet<String>,
    value_scopes: Vec<HashSet<String>>,
}

#[derive(Clone, Copy)]
struct Alias {
    position: usize,
    span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunctionDeclaration {
    Definition,
    External,
    Requirement,
}

enum Declaration {
    Function(Function),
    Global(ExternalVariable),
}

fn native_tag_name(token: SpannedToken, kind: &str) -> Result<String, Diagnostic> {
    let Token::StringLiteral(bytes) = token.kind else {
        return Err(Diagnostic::new(
            token.span,
            format!("expected a quoted native {kind} tag"),
        ));
    };
    let name = String::from_utf8(bytes).map_err(|_| {
        Diagnostic::new(
            token.span,
            format!("native {kind} tags require ASCII identifiers"),
        )
    })?;
    let mut bytes = name.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !(valid_c_symbol(&name) || matches!(name.as_str(), "bool" | "true" | "false" | "NULL"))
    {
        return Err(Diagnostic::new(
            token.span,
            format!("native {kind} tags require ASCII identifiers"),
        ));
    }
    return Ok(name);
}

fn valid_c_symbol(symbol: &str) -> bool {
    let mut bytes = symbol.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return false;
    }
    return !matches!(
        symbol,
        "auto"
            | "break"
            | "case"
            | "char"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extern"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "register"
            | "restrict"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "struct"
            | "switch"
            | "typedef"
            | "union"
            | "unsigned"
            | "void"
            | "volatile"
            | "while"
            | "_Alignas"
            | "_Alignof"
            | "_Atomic"
            | "_Bool"
            | "_Complex"
            | "_Generic"
            | "_Imaginary"
            | "_Noreturn"
            | "_Static_assert"
            | "_Thread_local"
            | "bool"
            | "true"
            | "false"
            | "NULL"
    );
}

impl Parser {
    // Collect top-level type declarations first so forward uses have the same
    // meaning throughout the file. Type aliases expand before the checker.
    fn collect_type_names(&mut self) -> Result<(), Diagnostic> {
        let mut depth = 0usize;
        let mut position = 0;
        while position < self.tokens.len() {
            if let Some(end) = self.generics.ends.get(&position) {
                position = *end;
                continue;
            }
            let token = self.tokens[position].clone();
            if depth == 0
                && token.kind == Token::Enum
                && self.tokens.get(position + 2).is_some_and(|token| {
                    matches!(token.kind, Token::LeftBrace | Token::Colon | Token::Equal)
                })
                && let Some(SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                }) = self.tokens.get(position + 1).cloned()
            {
                if name == "Self" || name == "this" {
                    return Err(Diagnostic::new(span, "Self and this are reserved names"));
                }
                self.enum_id(name, span);
            }
            if depth == 0
                && matches!(token.kind, Token::Struct | Token::Union)
                && self.tokens.get(position + 2).is_some_and(|token| {
                    matches!(
                        token.kind,
                        Token::LeftBrace | Token::Semicolon | Token::Equal
                    )
                })
                && let Some(SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                }) = self.tokens.get(position + 1).cloned()
            {
                if name == "Self" || name == "this" {
                    return Err(Diagnostic::new(span, "Self and this are reserved names"));
                }
                let id = self.tagged_struct_id(name.clone(), span, token.kind == Token::Union)?;
                if self.tokens[position + 2].kind == Token::Equal {
                    let native = native_tag_name(
                        self.tokens[position + 3].clone(),
                        if token.kind == Token::Union {
                            "union"
                        } else {
                            "struct"
                        },
                    )?;
                    if let Some(previous) = self.native_tags.insert(id, native.clone())
                        && previous != native
                    {
                        return Err(Diagnostic::new(
                            span,
                            format!("conflicting native tags for '{name}'"),
                        ));
                    }
                }
            }
            if depth == 0 && token.kind == Token::Typedef {
                let start = position + 1;
                let mut end = start;
                while let Some(token) = self.tokens.get(end) {
                    if matches!(
                        token.kind,
                        Token::Semicolon | Token::EndOfFile | Token::LeftBrace
                    ) {
                        break;
                    }
                    end += 1;
                }
                if !self
                    .tokens
                    .get(end)
                    .is_some_and(|token| token.kind == Token::Semicolon)
                {
                    return Err(Diagnostic::new(token.span, "expected 'typedef type Name;'"));
                }
                let mut brackets = 0usize;
                let mut outside_brackets = Vec::new();
                for index in start..end {
                    match self.tokens[index].kind {
                        Token::LeftBracket => brackets += 1,
                        Token::RightBracket => brackets = brackets.saturating_sub(1),
                        _ if brackets == 0 => outside_brackets.push(index),
                        _ => {}
                    }
                }
                let paren = outside_brackets
                    .iter()
                    .copied()
                    .find(|&index| self.tokens[index].kind == Token::LeftParen);
                let name_position = if let Some(paren) = paren {
                    let mut declarator = paren + 1;
                    if self.tokens.get(declarator).is_some_and(|token| {
                        matches!(
                            token.kind,
                            Token::Cdecl | Token::Stdcall | Token::Fastcall | Token::Vectorcall
                        )
                    }) {
                        declarator += 1;
                    }
                    if self
                        .tokens
                        .get(declarator)
                        .is_some_and(|token| token.kind == Token::Star)
                    {
                        let mut position = declarator;
                        while self
                            .tokens
                            .get(position)
                            .is_some_and(|token| token.kind == Token::Star)
                        {
                            position += 1;
                        }
                        position
                    } else {
                        paren.saturating_sub(1)
                    }
                } else {
                    outside_brackets
                        .into_iter()
                        .rfind(|&index| matches!(self.tokens[index].kind, Token::Identifier(_)))
                        .unwrap_or(start)
                };
                if name_position <= start {
                    return Err(Diagnostic::new(
                        token.span,
                        "a typedef needs both a type and an alias name",
                    ));
                }
                let SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                } = self.tokens[name_position].clone()
                else {
                    return Err(Diagnostic::new(token.span, "a typedef needs an alias name"));
                };
                if name == "Self" || name == "this" {
                    return Err(Diagnostic::new(span, "Self and this are reserved names"));
                }
                let alias = Alias {
                    position: start,
                    span,
                };
                self.source_declarations.push(SourceDeclaration {
                    kind: SourceDeclarationKind::Alias,
                    name_span: span,
                    header_span: Span {
                        end: self.tokens[end].span.start,
                        ..token.span
                    },
                    span: token.span.through(self.tokens[end].span),
                    container: None,
                });
                match self.aliases.entry(name) {
                    Entry::Occupied(entry) => {
                        self.repeated_aliases.push((entry.key().clone(), alias))
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(alias);
                    }
                }
                position = end + 1;
                continue;
            }
            match token.kind {
                Token::LeftBrace => depth += 1,
                Token::RightBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
            position += 1;
        }
        for (name, span) in &self.enum_names {
            if self.struct_names.iter().any(|entry| entry.0 == *name) {
                return Err(Diagnostic::new(
                    *span,
                    format!("type tag '{name}' is already used by a struct"),
                ));
            }
        }
        for name in self.generics.structs.keys() {
            if let Some((_, span)) = self
                .struct_names
                .iter()
                .chain(&self.enum_names)
                .find(|entry| entry.0 == *name)
            {
                return Err(Diagnostic::new(
                    *span,
                    format!("type tag '{name}' is already used by a generic struct"),
                ));
            }
        }
        self.collect_enum_sources();
        let mut aliases: Vec<_> = self
            .aliases
            .iter()
            .map(|(name, alias)| (alias.position, name.clone()))
            .collect();
        aliases.sort_by_key(|entry| entry.0);
        for (_, name) in aliases {
            self.alias_type(&name)?;
        }
        for (name, alias) in self.repeated_aliases.clone() {
            let (_, ty) = self.read_alias(alias)?;
            if ty != self.resolved_aliases[&name] {
                return Err(Diagnostic::new(
                    alias.span,
                    format!("conflicting typedef declarations for '{name}'"),
                ));
            }
        }
        return Ok(());
    }

    fn alias_type(&mut self, name: &str) -> Result<Type, Diagnostic> {
        if let Some(ty) = self.resolved_aliases.get(name) {
            return Ok(*ty);
        }
        let alias = self.aliases[name];
        if !self.resolving_aliases.insert(name.to_owned()) {
            return Err(Diagnostic::new(
                alias.span,
                format!("cyclic typedef involving '{name}'"),
            ));
        }
        let (parsed_name, ty) = self.read_alias(alias)?;
        if parsed_name != name {
            return Err(Diagnostic::new(alias.span, "expected 'typedef type Name;'"));
        }
        self.resolving_aliases.remove(name);
        self.resolved_aliases.insert(name.to_owned(), ty);
        return Ok(ty);
    }

    fn read_alias(&mut self, alias: Alias) -> Result<(String, Type), Diagnostic> {
        return self.global_constant_context(alias.position, |parser| parser.typedef_type());
    }

    fn typedef_type(&mut self) -> Result<(String, Type), Diagnostic> {
        let span = self.current().span;
        let parsed = self.typedef_type_inner()?;
        limits::ty(self, parsed.1, span)?;
        return Ok(parsed);
    }

    fn typedef_type_inner(&mut self) -> Result<(String, Type), Diagnostic> {
        let mut ty = self.type_name()?;
        let convention = self.calling_convention()?;
        if self.consume(&Token::LeftParen) {
            let inner_convention = self.calling_convention()?;
            let convention = if let Some(inner_convention) = inner_convention {
                if convention.is_some_and(|convention| inner_convention != convention) {
                    return Err(Diagnostic::new(
                        self.current().span,
                        "conflicting calling conventions",
                    ));
                }
                Some(inner_convention)
            } else {
                convention
            };
            let mut stars = 0;
            while self.consume(&Token::Star) {
                stars += 1;
            }
            if stars == 0 {
                return Err(Diagnostic::new(
                    self.current().span,
                    "expected '*' in function pointer typedef",
                ));
            }
            let (name, span) = self.identifier()?;
            self.expect(
                Token::RightParen,
                "expected ')' after function pointer name",
            )?;
            ty = self.function_type_suffix(ty, span, convention.unwrap_or(CallingConvention::C))?;
            for _ in 0..stars {
                ty = self.pointer_type(ty, true);
            }
            self.type_annotations.push((span, ty));
            return Ok((name, ty));
        }
        let (name, span) = self.identifier()?;
        ty = self.array_suffix(ty)?;
        if self.at(&Token::LeftParen) {
            ty = self.function_type_suffix(ty, span, convention.unwrap_or(CallingConvention::C))?;
        } else if convention.is_some() {
            return Err(Diagnostic::new(
                span,
                "a calling convention requires a function type",
            ));
        }
        self.type_annotations.push((span, ty));
        return Ok((name, ty));
    }

    fn pointer_type(&mut self, pointee: Type, mutable: bool) -> Type {
        return self.qualified_pointer_type(pointee, mutable, false);
    }

    fn qualified_pointer_type(&mut self, pointee: Type, mutable: bool, volatile: bool) -> Type {
        let pointer = PointerType {
            pointee,
            mutable,
            volatile,
        };
        let id = if let Some(id) = self
            .pointers
            .iter()
            .position(|existing| *existing == pointer)
        {
            id
        } else {
            let id = self.pointers.len();
            self.pointers.push(pointer);
            id
        };
        return Type::Pointer(id);
    }

    fn function_type_suffix(
        &mut self,
        result: Type,
        span: Span,
        convention: CallingConvention,
    ) -> Result<Type, Diagnostic> {
        self.expect(
            Token::LeftParen,
            "expected '(' before callback parameter types",
        )?;
        let mut parameters = Vec::new();
        let mut variadic = false;
        if self.at(&Token::Void) && self.next_at(&Token::RightParen) {
            self.advance();
        } else if !self.at(&Token::RightParen) {
            loop {
                if self.consume(&Token::Ellipsis) {
                    if parameters.is_empty() {
                        return Err(Diagnostic::new(
                            span,
                            "variadic function types require a fixed parameter",
                        ));
                    }
                    variadic = true;
                    break;
                }
                let mut ty = self.type_name()?;
                if matches!(self.current().kind, Token::Identifier(_)) {
                    self.advance();
                    ty = self.array_suffix(ty)?;
                }
                parameters.push(ty);
                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(
            Token::RightParen,
            "expected ')' after callback parameter types",
        )?;
        let calling_convention = Self::signature_convention(convention, variadic, span)?;
        let id = if let Some(id) = self.function_types.iter().position(|ty| {
            ty.result == result
                && ty.parameters == parameters
                && ty.variadic == variadic
                && ty.calling_convention == calling_convention
        }) {
            id
        } else {
            let id = self.function_types.len();
            self.function_types.push(FunctionType {
                calling_convention,
                variadic,
                result,
                parameters,
                span,
            });
            id
        };
        return Ok(Type::Function(id));
    }

    fn named_type(&mut self, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if let Some(ty) = self.generics.bindings.get(name).copied() {
            self.type_references.push(TypeReference {
                span,
                declaration: self
                    .generics
                    .binding_declarations
                    .get(name)
                    .copied()
                    .map(TypeDeclaration::Source),
            });
            return Ok(ty);
        }
        if name == "Self" {
            let ty = self
                .current_struct
                .map(Type::Struct)
                .ok_or_else(|| Diagnostic::new(span, "Self is only available inside a struct"))?;
            self.reference_type(span, ty);
            return Ok(ty);
        }
        if self.aliases.contains_key(name) {
            self.type_references.push(TypeReference {
                span,
                declaration: Some(TypeDeclaration::Source(self.aliases[name].span)),
            });
            return self.alias_type(name);
        }
        if self.generics.structs.contains_key(name) {
            let arguments = self.type_arguments()?;
            return self.instantiate_struct(name, arguments, span);
        }
        if let Some(kind) = IntType::named(name) {
            self.type_references.push(TypeReference {
                span,
                declaration: None,
            });
            return Ok(Type::Integer(kind));
        }
        if let Some(id) = self.struct_names.iter().position(|entry| entry.0 == name) {
            self.reference_type(span, Type::Struct(id));
            return Ok(Type::Struct(id));
        }
        if let Some(id) = self.enum_names.iter().position(|entry| entry.0 == name) {
            self.reference_type(span, Type::Enum(id));
            return Ok(Type::Enum(id));
        }
        return Err(Diagnostic::new(span, format!("unknown type '{name}'")));
    }

    fn known_type(&self, name: &str) -> bool {
        return name == "Self"
            || self.generics.bindings.contains_key(name)
            || self.generics.structs.contains_key(name)
            || IntType::named(name).is_some()
            || self.aliases.contains_key(name)
            || self.struct_names.iter().any(|entry| entry.0 == name)
            || self.enum_names.iter().any(|entry| entry.0 == name);
    }

    fn type_in_expression(&self) -> bool {
        return match &self.current().kind {
            Token::Int
            | Token::Char
            | Token::Signed
            | Token::Unsigned
            | Token::Short
            | Token::Long
            | Token::Bool
            | Token::Void
            | Token::BoxInt
            | Token::Struct
            | Token::Union
            | Token::Enum
            | Token::Const
            | Token::Volatile => true,
            Token::Identifier(name) => {
                self.known_type(name)
                    && !self
                        .value_scopes
                        .iter()
                        .rev()
                        .any(|scope| scope.contains(name))
            }
            _ => false,
        };
    }

    fn local_type_name(&self) -> bool {
        if matches!(&self.current().kind, Token::Identifier(name) if self.generics.structs.contains_key(name))
            && self.next_at(&Token::Less)
        {
            return self.generic_declaration_ahead();
        }
        if !matches!(&self.current().kind, Token::Identifier(name) if self.known_type(name)) {
            return false;
        }
        if self.next_at(&Token::Const) || self.next_at(&Token::Volatile) {
            return true;
        }
        if matches!(
            self.tokens.get(self.position + 1).map(|token| &token.kind),
            Some(Token::Identifier(_))
        ) {
            return true;
        }
        if self.next_at(&Token::Star) {
            let mut position = self.position + 1;
            while self.tokens.get(position).is_some_and(|token| {
                matches!(
                    token.kind,
                    Token::Star | Token::Const | Token::Volatile | Token::Ampersand
                )
            }) {
                position += 1;
            }
            return matches!(
                self.tokens.get(position).map(|token| &token.kind),
                Some(Token::Identifier(_))
            ) && self
                .tokens
                .get(position + 1)
                .is_some_and(|token| matches!(token.kind, Token::Equal | Token::LeftBracket));
        }
        // A type name may also be a value name. Keep `Count & other` available
        // as a bitwise expression, while recognizing initialized borrow locals.
        return self.next_at(&Token::Ampersand)
            && matches!(
                self.tokens.get(self.position + 2).map(|token| &token.kind),
                Some(Token::Identifier(_))
            )
            && self
                .tokens
                .get(self.position + 3)
                .is_some_and(|token| token.kind == Token::Equal);
    }

    fn struct_id(&mut self, name: String, span: Span) -> usize {
        if let Some(id) = self.struct_names.iter().position(|entry| entry.0 == name) {
            return id;
        }
        let id = self.struct_names.len();
        self.struct_names.push((name, span));
        return id;
    }

    fn tagged_struct_id(
        &mut self,
        name: String,
        span: Span,
        is_union: bool,
    ) -> Result<usize, Diagnostic> {
        if self.enum_names.iter().any(|entry| entry.0 == name) {
            return Err(Diagnostic::new(
                span,
                format!("type tag '{name}' is already used by an enum"),
            ));
        }
        if let Some(id) = self.struct_names.iter().position(|entry| entry.0 == name) {
            if self.union_tags.contains(&id) != is_union {
                return Err(Diagnostic::new(
                    span,
                    format!("tag '{name}' has a different struct or union kind"),
                ));
            }
            return Ok(id);
        }
        let id = self.struct_id(name, span);
        if is_union {
            self.union_tags.insert(id);
        }
        return Ok(id);
    }

    fn enum_id(&mut self, name: String, span: Span) -> usize {
        if let Some(id) = self.enum_names.iter().position(|entry| entry.0 == name) {
            return id;
        }
        let id = self.enum_names.len();
        self.enum_names.push((name, span));
        return id;
    }

    fn enum_definition(&mut self, external: bool) -> Result<EnumDefinition, Diagnostic> {
        let definition = self.enum_definition_until(None)?;
        if !external && definition.native_name.is_some() {
            return Err(Diagnostic::new(
                definition.name_span,
                "a renamed native enum requires extern",
            ));
        }
        return Ok(definition);
    }

    fn enum_definition_until(&mut self, stop: Option<&str>) -> Result<EnumDefinition, Diagnostic> {
        let start = self.advance().span;
        let (name, span) = self.identifier()?;
        let id = self.enum_id(name, span);
        let native_name = if self.consume(&Token::Equal) {
            Some(native_tag_name(self.advance(), "enum")?)
        } else {
            None
        };
        self.evaluating_enums.insert(id);
        let underlying = if self.consume(&Token::Colon) {
            let ty = self.type_name()?;
            let Type::Integer(kind) = ty else {
                return Err(Diagnostic::new(
                    span,
                    "an enum underlying type must be an integer",
                ));
            };
            kind
        } else {
            IntType::I32
        };
        let body_start = self.expect(Token::LeftBrace, "expected '{' before enum values")?;
        self.enum_representations.insert(id, underlying);
        let mut variants = Vec::new();
        let mut next = 0i128;
        while !self.at(&Token::RightBrace) {
            let (name, span) = self.identifier()?;
            let value = if self.consume(&Token::Equal) {
                let expression = self.expression(1)?;
                self.integer_constant(&expression, true)?.value
            } else {
                next
            };
            if !underlying.fits(value) {
                return Err(Diagnostic::new(
                    span,
                    "enum value does not fit its underlying type",
                ));
            }
            self.enum_constants
                .entry((id, name.clone()))
                .or_insert(constants::EnumConstant {
                    value,
                    kind: underlying,
                    span,
                });
            variants.push(EnumVariant { name, value, span });
            next = value + 1;
            if stop.is_some_and(|name| variants.last().unwrap().name == name) {
                break;
            }
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        let end = if stop
            .is_some_and(|name| variants.last().is_some_and(|variant| variant.name == name))
        {
            self.tokens[self.position - 1].span
        } else {
            self.expect(Token::RightBrace, "expected '}' after enum values")?;
            self.expect(Token::Semicolon, "expected ';' after enum definition")?
        };
        self.evaluating_enums.remove(&id);
        return Ok(EnumDefinition {
            native_name,
            name_span: span,
            header_span: Span {
                end: body_start.start,
                ..start
            },
            id,
            underlying,
            variants,
            span: start.through(end),
        });
    }

    fn struct_definition(&mut self) -> Result<StructDefinition, Diagnostic> {
        let token = self.advance();
        let start = token.span;
        let (name, span) = self.identifier()?;
        let id = self.tagged_struct_id(name.clone(), span, token.kind == Token::Union)?;
        return self.struct_body(id, &name, start, span);
    }

    fn struct_body(
        &mut self,
        id: usize,
        source_name: &str,
        start: Span,
        name_span: Span,
    ) -> Result<StructDefinition, Diagnostic> {
        let previous_owner = self.current_struct;
        self.current_struct = Some(id);
        let body_start = self.expect(Token::LeftBrace, "expected '{' after the struct name")?;
        let mut fields = Vec::new();
        let mut has_destructor = false;
        while !self.at(&Token::RightBrace) {
            let member_start = self.current().span;
            if self.at(&Token::EndOfFile) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "expected '}' after the struct fields",
                ));
            }
            if self.at(&Token::Tilde) {
                let start = self.advance().span;
                let (name, name_span) = self.identifier()?;
                if name != source_name {
                    return Err(Diagnostic::new(
                        name_span,
                        "the destructor name must match its enclosing struct",
                    ));
                }
                self.reference_type(name_span, Type::Struct(id));
                if has_destructor {
                    return Err(Diagnostic::new(
                        start,
                        "a struct can only define one destructor",
                    ));
                }
                self.expect(Token::LeftParen, "expected '(' after the destructor name")?;
                self.expect(
                    Token::RightParen,
                    "destructors cannot have explicit parameters",
                )?;
                if self.at(&Token::Const) {
                    return Err(Diagnostic::new(
                        self.current().span,
                        "destructors always have an exclusive receiver and cannot be const",
                    ));
                }
                let body = self.block()?;
                self.methods.push(Function {
                    dll_import: false,
                    calling_convention: CallingConvention::C,
                    name_span,
                    link_name: None,
                    variadic: false,
                    requires_unsafe: false,
                    external: false,
                    name: destructor_name(id),
                    return_type: Type::Void,
                    parameters: vec![Parameter {
                        declaration_span: name_span,
                        synthetic: true,
                        name: String::from("this"),
                        ty: Type::Borrow {
                            pointee: Pointee::Struct(id),
                            mutable: true,
                            volatile: false,
                        },
                        span: name_span,
                    }],
                    span: start.through(body.span),
                    body,
                });
                has_destructor = true;
                continue;
            }
            let private = self.consume(&Token::Private);
            let unsafe_prefix = self.consume(&Token::Unsafe);
            let is_static = self.consume(&Token::Static);
            let requires_unsafe = self.consume(&Token::Unsafe) || unsafe_prefix;
            let ty = self.type_name()?;
            let convention = self.calling_convention()?;
            let (name, span) = self.identifier()?;
            if self.at(&Token::LeftParen) {
                if private {
                    return Err(Diagnostic::new(span, "private currently applies to fields"));
                }
                let key = if is_static {
                    static_method_name(id, &name)
                } else {
                    method_name(id, &name)
                };
                let mut method = self.function_body(
                    ty,
                    (key, span),
                    member_start,
                    Some(id),
                    is_static,
                    FunctionDeclaration::Definition,
                )?;
                method.requires_unsafe = requires_unsafe;
                method.calling_convention = Self::signature_convention(
                    convention.unwrap_or(CallingConvention::C),
                    method.variadic,
                    span,
                )?;
                self.methods.push(method);
                continue;
            }
            if convention.is_some() {
                return Err(Diagnostic::new(
                    span,
                    "a calling convention requires a method",
                ));
            }
            if is_static {
                return Err(Diagnostic::new(span, "static currently applies to methods"));
            }
            if requires_unsafe {
                return Err(Diagnostic::new(
                    span,
                    "unsafe applies to functions and methods",
                ));
            }
            let ty = self.array_suffix(ty)?;
            let declaration_span = Span {
                end: self.current().span.start,
                ..member_start
            };
            self.expect(Token::Semicolon, "expected ';' after the field")?;
            fields.push(Field {
                declaration_span,
                name,
                ty,
                span,
                private,
            });
        }
        self.advance();
        self.current_struct = previous_owner;
        let end = self.expect(Token::Semicolon, "expected ';' after the struct definition")?;
        return Ok(StructDefinition {
            name_span,
            header_span: Span {
                end: body_start.start,
                ..start
            },
            id,
            fields,
            has_destructor,
            span: start.through(end),
        });
    }

    fn function(&mut self) -> Result<Function, Diagnostic> {
        let span = self.current().span;
        return match self.declaration()? {
            Declaration::Function(function) => Ok(function),
            Declaration::Global(_) => Err(Diagnostic::new(span, "expected a function declaration")),
        };
    }

    fn declaration(&mut self) -> Result<Declaration, Diagnostic> {
        let start = self.current().span;
        let file_local = self.consume(&Token::Static);
        let unsafe_prefix = self.consume(&Token::Unsafe);
        let import_prefix = self.dll_import_attribute()?;
        let external = self.consume(&Token::Extern);
        if file_local && external {
            return Err(Diagnostic::new(
                start,
                "static functions cannot have extern linkage",
            ));
        }
        let dll_import = self.dll_import_attribute()? || import_prefix;
        if dll_import && !external {
            return Err(Diagnostic::new(
                start,
                "__declspec(dllimport) requires an extern declaration",
            ));
        }
        let unsafe_suffix = self.consume(&Token::Unsafe);
        let requires_unsafe = unsafe_suffix || unsafe_prefix || external;
        let (return_type, qualifiers) = self.qualified_type_name(external)?;
        let convention = self.calling_convention()?;
        let (mut name, name_span) = self.identifier()?;
        if file_local {
            name = self.file_function_name(&name, name_span);
        }
        if external && !self.at(&Token::LeftParen) && !self.at(&Token::Less) {
            if convention.is_some() || unsafe_prefix || unsafe_suffix {
                return Err(Diagnostic::new(
                    start,
                    "calling conventions and unsafe modifiers apply to functions",
                ));
            }
            let ty = self.array_suffix(return_type)?;
            if self.has_function_template(&name) {
                return Err(Diagnostic::new(
                    name_span,
                    "external variable name conflicts with a generic function",
                ));
            }
            let link_name = self.external_link_name(&name)?;
            let end = self.expect(
                Token::Semicolon,
                "expected ';' after external variable declaration",
            )?;
            return Ok(Declaration::Global(ExternalVariable {
                dll_import,
                name,
                name_span,
                ty,
                readonly: qualifiers.readonly,
                volatile: qualifiers.volatile,
                link_name,
                span: start.through(end),
            }));
        }
        if qualifiers.readonly || qualifiers.volatile {
            return Err(Diagnostic::new(
                start,
                "qualified function results require a pointer or borrow type",
            ));
        }
        if self.at(&Token::Less) {
            self.skip_type_parameters()?;
            name =
                self.generics.current_function.clone().ok_or_else(|| {
                    Diagnostic::new(start, "generic function needs type arguments")
                })?;
        }
        let declaration = if external {
            FunctionDeclaration::External
        } else {
            FunctionDeclaration::Definition
        };
        let mut function = self.function_body(
            return_type,
            (name, name_span),
            start,
            None,
            false,
            declaration,
        )?;
        function.requires_unsafe = requires_unsafe;
        if file_local && function.link_name.is_some() {
            return Err(Diagnostic::new(
                start,
                "static functions cannot export a C symbol",
            ));
        }
        function.dll_import = dll_import;
        if dll_import && !function.external {
            return Err(Diagnostic::new(
                start,
                "a DLL import cannot have a function body",
            ));
        }
        function.calling_convention = Self::signature_convention(
            convention.unwrap_or(CallingConvention::C),
            function.variadic,
            start,
        )?;
        return Ok(Declaration::Function(function));
    }

    fn external_link_name(&mut self, name: &str) -> Result<String, Diagnostic> {
        if !self.consume(&Token::Equal) {
            if !valid_c_symbol(name) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "C symbol name is not usable as a C identifier",
                ));
            }
            return Ok(name.to_owned());
        }
        let token = self.advance();
        let Token::StringLiteral(bytes) = token.kind else {
            return Err(Diagnostic::new(
                token.span,
                "expected a quoted C symbol name",
            ));
        };
        let symbol = String::from_utf8(bytes)
            .map_err(|_| Diagnostic::new(token.span, "C symbol names require ASCII identifiers"))?;
        if !valid_c_symbol(&symbol) {
            return Err(Diagnostic::new(
                token.span,
                "C symbol name is not usable as a C identifier",
            ));
        }
        return Ok(symbol);
    }

    fn function_body(
        &mut self,
        return_type: Type,
        name: (String, Span),
        start: Span,
        owner: Option<usize>,
        is_static: bool,
        declaration: FunctionDeclaration,
    ) -> Result<Function, Diagnostic> {
        let (name, name_span) = name;
        let external = declaration == FunctionDeclaration::External;
        let prototype = declaration != FunctionDeclaration::Definition;
        let mut link_name = external.then(|| name.clone());
        self.expect(Token::LeftParen, "expected '(' after the function name")?;
        let mut parameters = Vec::new();
        let mut variadic = false;

        if self.at(&Token::Void) && self.next_at(&Token::RightParen) {
            self.advance();
        } else if !self.at(&Token::RightParen) {
            loop {
                if self.consume(&Token::Ellipsis) {
                    if parameters.is_empty() || !external {
                        return Err(Diagnostic::new(
                            start,
                            "variadic declarations require extern and a fixed parameter",
                        ));
                    }
                    variadic = true;
                    break;
                }
                let parameter_start = self.current().span;
                let ty = self.type_name()?;
                if ty == Type::Void {
                    return Err(Diagnostic::new(
                        self.current().span,
                        "a named parameter cannot have type void",
                    ));
                }
                let (name, span, synthetic) = if prototype
                    && matches!(self.current().kind, Token::Comma | Token::RightParen)
                {
                    (
                        format!("arg_{}", parameters.len()),
                        self.current().span,
                        true,
                    )
                } else {
                    let (name, span) = self.identifier()?;
                    (name, span, false)
                };
                let ty = self.array_suffix(ty)?;
                parameters.push(Parameter {
                    name,
                    ty,
                    span,
                    synthetic,
                    declaration_span: Span {
                        end: self.current().span.start,
                        ..parameter_start
                    },
                });
                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }

        self.expect(Token::RightParen, "expected ')' after the parameters")?;
        let shared = self.consume(&Token::Const);
        if is_static && shared {
            return Err(Diagnostic::new(
                start,
                "a static method has no receiver to qualify as const",
            ));
        }
        if let Some(id) = owner.filter(|_| !is_static) {
            if parameters
                .first()
                .is_some_and(|parameter| parameter.name == "this")
            {
                if shared || parameters[0].ty != Type::Struct(id) {
                    return Err(Diagnostic::new(
                        start,
                        "an explicit receiver must be 'Self this' and cannot have a const method qualifier",
                    ));
                }
            } else {
                parameters.insert(
                    0,
                    Parameter {
                        declaration_span: name_span,
                        synthetic: true,
                        name: String::from("this"),
                        ty: Type::Borrow {
                            pointee: Pointee::Struct(id),
                            mutable: !shared,
                            volatile: false,
                        },
                        span: name_span,
                    },
                );
            }
        } else if shared {
            return Err(Diagnostic::new(
                start,
                "only instance methods can have a const method qualifier",
            ));
        }
        self.value_scopes.push(
            parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        if external {
            link_name = Some(self.external_link_name(&name)?);
        }
        let defined_export = external && self.at(&Token::LeftBrace);
        if defined_export && (variadic || parameters.iter().any(|parameter| parameter.synthetic)) {
            return Err(Diagnostic::new(
                start,
                "C exports require named parameters and a fixed parameter list",
            ));
        }
        let body = if prototype && !defined_export {
            let span = self.expect(Token::Semicolon, "expected ';' after function declaration")?;
            Block {
                statements: Vec::new(),
                span,
            }
        } else {
            self.block()?
        };
        self.value_scopes.pop();
        let span = start.through(body.span);
        return Ok(Function {
            dll_import: false,
            calling_convention: CallingConvention::C,
            name_span,
            link_name,
            variadic,
            requires_unsafe: external,
            external: external && !defined_export,
            name,
            return_type,
            parameters,
            body,
            span,
        });
    }

    fn type_name(&mut self) -> Result<Type, Diagnostic> {
        return self.qualified_type_name(false).map(|(ty, _)| ty);
    }

    fn qualified_type_name(&mut self, allow_const: bool) -> Result<(Type, Qualifiers), Diagnostic> {
        let span = self.current().span;
        let parsed = self.nested(|parser| parser.qualified_type_name_inner(allow_const))?;
        limits::ty(self, parsed.0, span)?;
        return Ok(parsed);
    }

    fn qualified_type_name_inner(
        &mut self,
        allow_const: bool,
    ) -> Result<(Type, Qualifiers), Diagnostic> {
        let start = self.current().span;
        let mut qualifiers = self.qualifiers();
        let token = self.advance();
        let base = match token.kind {
            Token::Int
            | Token::Char
            | Token::Signed
            | Token::Unsigned
            | Token::Short
            | Token::Long => self.integer_type(token.clone(), &mut qualifiers)?,
            Token::Bool => Type::Bool,
            Token::Void => Type::Void,
            Token::BoxInt => Type::BoxInt,
            Token::Struct | Token::Union => {
                let (name, span) = self.identifier()?;
                if self.generics.structs.contains_key(&name) {
                    if self.generics.structs[&name].is_union != (token.kind == Token::Union) {
                        return Err(Diagnostic::new(
                            span,
                            "generic tag has a different struct or union kind",
                        ));
                    }
                    let arguments = self.type_arguments()?;
                    self.instantiate_struct(&name, arguments, span)?
                } else {
                    let ty = Type::Struct(self.tagged_struct_id(
                        name,
                        span,
                        token.kind == Token::Union,
                    )?);
                    self.reference_type(span, ty);
                    ty
                }
            }
            Token::Enum => {
                let (name, span) = self.identifier()?;
                let ty = Type::Enum(self.enum_id(name, span));
                self.reference_type(span, ty);
                ty
            }
            Token::Identifier(ref name) => self.named_type(name, token.span)?,
            _ => {
                return Err(Diagnostic::new(
                    token.span,
                    "expected a type: 'int', 'bool', 'void', 'BoxInt', or a type name",
                ));
            }
        };
        let trailing = self.qualifiers();
        qualifiers.readonly |= trailing.readonly;
        qualifiers.volatile |= trailing.volatile;
        let mut base = self.array_suffix(base)?;
        while self.consume(&Token::Star) {
            if base.is_borrow() {
                return Err(Diagnostic::new(
                    token.span,
                    "raw pointers cannot point to borrow bindings",
                ));
            }
            if (qualifiers.readonly || qualifiers.volatile) && matches!(base, Type::Function(_)) {
                return Err(Diagnostic::new(
                    token.span,
                    "function types cannot be const- or volatile-qualified",
                ));
            }
            base = self.qualified_pointer_type(base, !qualifiers.readonly, qualifiers.volatile);
            qualifiers = self.qualifiers();
        }
        let base = self.array_suffix(base)?;
        let ty = if self.consume(&Token::Ampersand) {
            let ty = base
                .borrowed_qualified(!qualifiers.readonly, qualifiers.volatile)
                .ok_or_else(|| Diagnostic::new(token.span, "void cannot be borrowed"))?;
            qualifiers = Qualifiers::default();
            ty
        } else {
            if (qualifiers.readonly || qualifiers.volatile) && !allow_const {
                return Err(Diagnostic::new(
                    token.span,
                    "const and volatile qualify pointer targets, borrows, and external objects",
                ));
            }
            base
        };
        let span = Span {
            end: self
                .pending_greater
                .as_ref()
                .map(|token| token.span.start)
                .unwrap_or(self.tokens[self.position - 1].span.end),
            ..start
        };
        self.type_annotations.push((span, ty));
        self.type_references.push(TypeReference {
            span,
            declaration: None,
        });
        return Ok((ty, qualifiers));
    }

    fn qualifiers(&mut self) -> Qualifiers {
        let mut qualifiers = Qualifiers::default();
        loop {
            if self.consume(&Token::Const) {
                qualifiers.readonly = true;
            } else if self.consume(&Token::Volatile) {
                qualifiers.volatile = true;
            } else {
                break;
            }
        }
        return qualifiers;
    }

    fn array_suffix(&mut self, mut element: Type) -> Result<Type, Diagnostic> {
        let mut dimensions = Vec::new();
        while self.consume(&Token::LeftBracket) {
            let expression = self.expression(1)?;
            let value = self.integer_constant(&expression, true)?.value;
            let length = usize::try_from(value)
                .ok()
                .filter(|length| *length > 0)
                .ok_or_else(|| {
                    Diagnostic::new(
                        expression.span,
                        "array length must fit size_t and be positive",
                    )
                })?;
            self.expect(Token::RightBracket, "expected ']' after array length")?;
            dimensions.push((length, expression.span));
        }
        for (length, span) in dimensions.into_iter().rev() {
            if element == Type::Void || element.is_borrow() {
                return Err(Diagnostic::new(span, "array elements must be values"));
            }
            let id = if let Some(id) = self
                .arrays
                .iter()
                .position(|array| array.element == element && array.length == length)
            {
                id
            } else {
                let id = self.arrays.len();
                self.arrays.push(ArrayType {
                    element,
                    length,
                    span,
                });
                id
            };
            element = Type::Array(id);
        }
        limits::ty(self, element, self.current().span)?;
        return Ok(element);
    }

    fn block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.expect(Token::LeftBrace, "expected '{' to start a block")?;
        self.value_scopes.push(HashSet::new());
        let mut statements = Vec::new();
        while !self.at(&Token::RightBrace) {
            if self.at(&Token::EndOfFile) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "expected '}' before end of file",
                ));
            }
            statements.push(self.statement()?);
        }
        let end = self.advance().span;
        self.value_scopes.pop();
        return Ok(Block {
            statements,
            span: start.through(end),
        });
    }

    fn statement(&mut self) -> Result<Statement, Diagnostic> {
        return self.nested(Self::statement_inner);
    }

    fn control_body(&mut self) -> Result<Block, Diagnostic> {
        if self.at(&Token::LeftBrace) {
            return self.block();
        }
        let statement = self.statement()?;
        if matches!(statement.kind, StatementKind::Local { .. }) {
            return Err(Diagnostic::new(
                statement.span,
                "a local declaration in a control-flow body requires braces",
            ));
        }
        return Ok(Block {
            span: statement.span,
            statements: vec![statement],
        });
    }

    fn statement_inner(&mut self) -> Result<Statement, Diagnostic> {
        let start = self.current().span;

        if self.consume(&Token::Unsafe) {
            let block = self.block()?;
            return Ok(Statement {
                span: start.through(block.span),
                kind: StatementKind::Unsafe(block),
            });
        }

        if self.at(&Token::LeftBrace) {
            let block = self.block()?;
            return Ok(Statement {
                span: block.span,
                kind: StatementKind::Block(block),
            });
        }

        if self.consume(&Token::If) {
            self.expect(Token::LeftParen, "expected '(' after 'if'")?;
            let condition = self.expression(1)?;
            self.expect(Token::RightParen, "expected ')' after the condition")?;
            let then_block = self.control_body()?;
            let else_block = if self.consume(&Token::Else) {
                Some(self.control_body()?)
            } else {
                None
            };
            let end = else_block
                .as_ref()
                .map_or(then_block.span, |block| block.span);
            return Ok(Statement {
                span: start.through(end),
                kind: StatementKind::If {
                    condition,
                    then_block,
                    else_block,
                },
            });
        }

        if self.consume(&Token::Do) {
            let body = self.control_body()?;
            self.expect(Token::While, "expected 'while' after the do body")?;
            self.expect(Token::LeftParen, "expected '(' after 'while'")?;
            let condition = self.expression(1)?;
            self.expect(Token::RightParen, "expected ')' after the condition")?;
            let end = self.expect(
                Token::Semicolon,
                "expected ';' after the do-while condition",
            )?;
            return Ok(Statement {
                span: start.through(end),
                kind: StatementKind::DoWhile { body, condition },
            });
        }

        if self.consume(&Token::While) {
            self.expect(Token::LeftParen, "expected '(' after 'while'")?;
            let condition = self.expression(1)?;
            self.expect(Token::RightParen, "expected ')' after the condition")?;
            let body = self.control_body()?;
            return Ok(Statement {
                span: start.through(body.span),
                kind: StatementKind::While {
                    condition,
                    body,
                    step: None,
                },
            });
        }

        if self.consume(&Token::For) {
            self.expect(Token::LeftParen, "expected '(' after 'for'")?;
            self.value_scopes.push(HashSet::new());
            let initializer = self.statement()?;
            if !matches!(
                initializer.kind,
                StatementKind::Local { .. }
                    | StatementKind::Assign { .. }
                    | StatementKind::Store { .. }
                    | StatementKind::CompoundAssign { .. }
                    | StatementKind::Expression(_)
                    | StatementKind::Empty
            ) {
                return Err(Diagnostic::new(
                    initializer.span,
                    "a for initializer must be a declaration, assignment, or expression",
                ));
            }
            let condition = if self.at(&Token::Semicolon) {
                Expression {
                    span: self.current().span,
                    kind: ExpressionKind::Integer(1),
                }
            } else {
                self.expression(1)?
            };
            self.expect(Token::Semicolon, "expected ';' after the for condition")?;
            let step_start = self.current().span;
            let step = if self.at(&Token::RightParen) {
                StatementKind::Empty
            } else {
                self.assignment_or_expression()?
            };
            let step_end = self.expect(Token::RightParen, "expected ')' after the for step")?;
            let body = self.control_body()?;
            self.value_scopes.pop();
            let span = start.through(body.span);
            let loop_statement = Statement {
                span,
                kind: StatementKind::While {
                    condition,
                    body,
                    step: Some(Box::new(Statement {
                        span: step_start.through(step_end),
                        kind: step,
                    })),
                },
            };
            return Ok(Statement {
                span,
                kind: StatementKind::Block(Block {
                    span,
                    statements: vec![initializer, loop_statement],
                }),
            });
        }

        let kind;
        if self.at(&Token::Int)
            || self.at(&Token::Void)
            || matches!(
                self.current().kind,
                Token::Char | Token::Signed | Token::Unsigned | Token::Short | Token::Long
            )
            || self.at(&Token::Bool)
            || self.at(&Token::BoxInt)
            || self.at(&Token::Const)
            || self.at(&Token::Volatile)
            || self.at(&Token::Struct)
            || self.at(&Token::Union)
            || self.at(&Token::Enum)
            || self.local_type_name()
        {
            let ty = self.type_name()?;
            if ty == Type::Void {
                return Err(Diagnostic::new(
                    start,
                    "a local variable cannot have type void",
                ));
            }
            let (name, name_span) = self.identifier()?;
            let ty = self.array_suffix(ty)?;
            let declaration_span = Span {
                end: self.current().span.start,
                ..start
            };
            self.value_scopes.last_mut().unwrap().insert(name.clone());
            self.expect(
                Token::Equal,
                "local variables need an initializer: 'type name = expression;'",
            )?;
            let initializer = self.initializer()?;
            kind = StatementKind::Local {
                name_span,
                declaration_span,
                name,
                ty,
                initializer,
            };
        } else if self.consume(&Token::Break) {
            kind = StatementKind::Break;
        } else if self.consume(&Token::Continue) {
            kind = StatementKind::Continue;
        } else if self.consume(&Token::Return) {
            let value = if self.at(&Token::Semicolon) {
                None
            } else {
                Some(self.expression(1)?)
            };
            kind = StatementKind::Return(value);
        } else if self.at(&Token::Semicolon) {
            kind = StatementKind::Empty;
        } else {
            kind = self.assignment_or_expression()?;
        }

        let end = self.expect(Token::Semicolon, "expected ';' after the statement")?;
        return Ok(Statement {
            kind,
            span: start.through(end),
        });
    }

    fn assignment_or_expression(&mut self) -> Result<StatementKind, Diagnostic> {
        let target = self.expression(1)?;
        if self.consume(&Token::Equal) {
            let value = self.initializer()?;
            if let ExpressionKind::Variable { name, name_span } = target.kind {
                return Ok(StatementKind::Assign {
                    name,
                    name_span,
                    value,
                });
            }
            return Ok(StatementKind::Store { target, value });
        }
        let operator = match self.current().kind {
            Token::PlusEqual => Some(BinaryOperator::Add),
            Token::MinusEqual => Some(BinaryOperator::Subtract),
            Token::StarEqual => Some(BinaryOperator::Multiply),
            Token::SlashEqual => Some(BinaryOperator::Divide),
            Token::PercentEqual => Some(BinaryOperator::Remainder),
            Token::AmpersandEqual => Some(BinaryOperator::BitAnd),
            Token::PipeEqual => Some(BinaryOperator::BitOr),
            Token::CaretEqual => Some(BinaryOperator::BitXor),
            Token::LessLessEqual => Some(BinaryOperator::ShiftLeft),
            Token::GreaterGreaterEqual => Some(BinaryOperator::ShiftRight),
            _ => None,
        };
        if let Some(operator) = operator {
            self.advance();
            let value = self.expression(1)?;
            return Ok(StatementKind::CompoundAssign {
                target,
                operator,
                value,
            });
        }
        return Ok(StatementKind::Expression(target));
    }

    // Higher precedence binds more tightly. The recursive call with +1 makes
    // operators at the same precedence left-associative: a - b - c = (a - b) - c.
    fn expression(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        return self.nested(|parser| parser.expression_inner(minimum));
    }

    fn expression_inner(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        let mut left = self.unary()?;
        while let Some((operator, precedence)) = Self::binary_operator(&self.current().kind) {
            if precedence < minimum {
                break;
            }
            self.advance();
            let right = self.expression(precedence + 1)?;
            let span = left.span.through(right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
            limits::expression(&left)?;
        }
        if minimum <= 1 && self.consume(&Token::Question) {
            let then_value = self.expression(1)?;
            self.expect(Token::Colon, "expected ':' in conditional expression")?;
            let else_value = self.expression(1)?;
            let span = left.span.through(else_value.span);
            left = Expression {
                span,
                kind: ExpressionKind::Conditional {
                    condition: Box::new(left),
                    then_value: Box::new(then_value),
                    else_value: Box::new(else_value),
                },
            };
            limits::expression(&left)?;
        }
        return Ok(left);
    }

    fn unary(&mut self) -> Result<Expression, Diagnostic> {
        let expression = self.nested(Self::unary_inner)?;
        limits::expression(&expression)?;
        return Ok(expression);
    }

    fn unary_inner(&mut self) -> Result<Expression, Diagnostic> {
        let start = self.current().span;
        if matches!(self.current().kind, Token::PlusPlus | Token::MinusMinus) {
            let increment = self.advance().kind == Token::PlusPlus;
            let target = self.unary()?;
            return Ok(Expression {
                span: start.through(target.span),
                kind: ExpressionKind::Update {
                    target: Box::new(target),
                    increment,
                    prefix: true,
                },
            });
        }
        if self.consume(&Token::Alignof) {
            self.expect(Token::LeftParen, "expected '(' after alignof")?;
            let ty = self.type_name()?;
            let end = self.expect(Token::RightParen, "expected ')' after the type")?;
            return Ok(Expression {
                span: start.through(end),
                kind: ExpressionKind::Alignof(ty),
            });
        }
        if self.consume(&Token::Sizeof) {
            let saved = self.position;
            if self.consume(&Token::LeftParen) && self.type_in_expression() {
                let ty = self.type_name()?;
                let end = self.expect(Token::RightParen, "expected ')' after the type")?;
                return Ok(Expression {
                    span: start.through(end),
                    kind: ExpressionKind::SizeofType(ty),
                });
            }
            self.position = saved;
            let operand = self.unary()?;
            return Ok(Expression {
                span: start.through(operand.span),
                kind: ExpressionKind::SizeofValue(Box::new(operand)),
            });
        }
        if self.at(&Token::LeftParen) {
            let saved = self.position;
            self.advance();
            if self.type_in_expression() {
                let ty = self.type_name()?;
                self.expect(Token::RightParen, "expected ')' after the cast type")?;
                if !self.at(&Token::LeftBrace) {
                    let operand = self.unary()?;
                    return Ok(Expression {
                        span: start.through(operand.span),
                        kind: ExpressionKind::Cast {
                            ty,
                            operand: Box::new(operand),
                        },
                    });
                }
            }
            self.position = saved;
        }
        let operator = match self.current().kind {
            Token::Ampersand => Some(UnaryOperator::Address),
            Token::Plus => Some(UnaryOperator::Plus),
            Token::Minus => Some(UnaryOperator::Negate),
            Token::Bang => Some(UnaryOperator::Not),
            Token::Tilde => Some(UnaryOperator::BitNot),
            Token::Star => Some(UnaryOperator::Dereference),
            _ => None,
        };
        if let Some(mut operator) = operator {
            let start = self.advance().span;
            if operator == UnaryOperator::Address && self.consume(&Token::Const) {
                operator = UnaryOperator::Borrow { mutable: false };
            }
            let operand = self.unary()?;
            return Ok(Expression {
                span: start.through(operand.span),
                kind: ExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                },
            });
        }
        return self.postfix();
    }

    fn postfix(&mut self) -> Result<Expression, Diagnostic> {
        let mut expression = self.primary()?;
        loop {
            limits::expression(&expression)?;
            if matches!(self.current().kind, Token::PlusPlus | Token::MinusMinus) {
                let token = self.advance();
                expression = Expression {
                    span: expression.span.through(token.span),
                    kind: ExpressionKind::Update {
                        target: Box::new(expression),
                        increment: token.kind == Token::PlusPlus,
                        prefix: false,
                    },
                };
                continue;
            }
            if self.consume(&Token::LeftParen) {
                let (arguments, end) = self.arguments()?;
                expression = Expression {
                    span: expression.span.through(end),
                    kind: ExpressionKind::IndirectCall {
                        callee: Box::new(expression),
                        arguments,
                    },
                };
                continue;
            }
            if self.consume(&Token::LeftBracket) {
                let index = self.expression(1)?;
                let end = self.expect(Token::RightBracket, "expected ']' after index")?;
                expression = Expression {
                    span: expression.span.through(end),
                    kind: ExpressionKind::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    },
                };
                continue;
            }
            let arrow = self.consume(&Token::Arrow);
            if !arrow && !self.consume(&Token::Dot) {
                break;
            }
            if self.at(&Token::Tilde) {
                return Err(Diagnostic::new(
                    self.current().span,
                    "destructors cannot be called explicitly",
                ));
            }
            let (name, span) = self.identifier()?;
            if arrow {
                expression = Expression {
                    span: expression.span,
                    kind: ExpressionKind::Unary {
                        operator: UnaryOperator::Dereference,
                        operand: Box::new(expression),
                    },
                };
            }
            if self.consume(&Token::LeftParen) {
                let (arguments, end) = self.arguments()?;
                expression = Expression {
                    span: expression.span.through(end),
                    kind: ExpressionKind::MethodCall {
                        name_span: span,
                        receiver: Box::new(expression),
                        name,
                        arguments,
                    },
                };
                continue;
            }
            expression = Expression {
                span: expression.span.through(span),
                kind: ExpressionKind::Field {
                    name_span: span,
                    base: Box::new(expression),
                    name,
                },
            };
        }
        return Ok(expression);
    }

    fn initializer(&mut self) -> Result<Expression, Diagnostic> {
        return self.nested(Self::initializer_inner);
    }

    fn initializer_inner(&mut self) -> Result<Expression, Diagnostic> {
        if self.at(&Token::LeftBrace) {
            if !self.next_at(&Token::Dot) && !self.next_at(&Token::RightBrace) {
                let start = self.advance().span;
                let mut elements = Vec::new();
                while !self.at(&Token::RightBrace) {
                    elements.push(self.initializer()?);
                    if !self.consume(&Token::Comma) {
                        break;
                    }
                }
                let end = self.expect(Token::RightBrace, "expected '}' after array initializer")?;
                return Ok(Expression {
                    span: start.through(end),
                    kind: ExpressionKind::ArrayLiteral { id: None, elements },
                });
            }
            return self.struct_literal(None);
        }
        return self.expression(1);
    }

    fn arguments(&mut self) -> Result<(Vec<Expression>, Span), Diagnostic> {
        let mut arguments = Vec::new();
        if !self.at(&Token::RightParen) {
            loop {
                arguments.push(self.expression(1)?);
                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }
        let end = self.expect(Token::RightParen, "expected ')' after the arguments")?;
        return Ok((arguments, end));
    }

    fn struct_literal(&mut self, id: Option<usize>) -> Result<Expression, Diagnostic> {
        let start = self.expect(
            Token::LeftBrace,
            "expected '{' to start a struct initializer",
        )?;
        let mut fields = Vec::new();
        while !self.at(&Token::RightBrace) {
            self.expect(
                Token::Dot,
                "struct initializers need named fields: '.field = value'",
            )?;
            let (name, span) = self.identifier()?;
            self.expect(Token::Equal, "expected '=' after the field name")?;
            let value = self.initializer()?;
            fields.push(FieldInitializer {
                name_span: span,
                name,
                span: span.through(value.span),
                value,
            });
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        let end = self.expect(
            Token::RightBrace,
            "expected '}' after the struct initializer",
        )?;
        return Ok(Expression {
            span: start.through(end),
            kind: ExpressionKind::StructLiteral { id, fields },
        });
    }

    fn primary(&mut self) -> Result<Expression, Diagnostic> {
        let token = self.advance();
        match token.kind {
            Token::Init | Token::Destroy => {
                let initialize = token.kind == Token::Init;
                self.expect(Token::LeftParen, "expected '(' after storage operation")?;
                let pointer = Box::new(self.expression(0)?);
                let value = if initialize {
                    self.expect(Token::Comma, "init requires a pointer and value")?;
                    Some(Box::new(self.initializer()?))
                } else {
                    None
                };
                let end = self.expect(Token::RightParen, "expected ')' after storage operation")?;
                return Ok(Expression {
                    span: token.span.through(end),
                    kind: ExpressionKind::Storage { pointer, value },
                });
            }
            Token::StringLiteral(mut bytes) => {
                let mut span = token.span;
                while let Token::StringLiteral(next) = &self.current().kind {
                    bytes.extend_from_slice(next);
                    span = span.through(self.advance().span);
                }
                let element = Type::Integer(IntType::U8);
                let length = bytes.len() + 1;
                let id = if let Some(id) = self
                    .arrays
                    .iter()
                    .position(|array| array.element == element && array.length == length)
                {
                    id
                } else {
                    let id = self.arrays.len();
                    self.arrays.push(ArrayType {
                        element,
                        length,
                        span,
                    });
                    id
                };
                return Ok(Expression {
                    span,
                    kind: ExpressionKind::StringLiteral { id, bytes },
                });
            }
            Token::True | Token::False => {
                return Ok(Expression {
                    span: token.span,
                    kind: ExpressionKind::Boolean(token.kind == Token::True),
                });
            }
            Token::IntegerLiteral(value) => {
                return Ok(Expression {
                    kind: ExpressionKind::Integer(value),
                    span: token.span,
                });
            }
            Token::SuffixedIntegerLiteral {
                value,
                suffix,
                decimal,
            } => {
                let kind = integers::literal_type(value, suffix, decimal, token.span)?;
                return Ok(Expression {
                    kind: ExpressionKind::TypedInteger { value, kind },
                    span: token.span,
                });
            }
            Token::Identifier(name) => {
                let mut generic_type = None;
                let name = if self
                    .value_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains(&name))
                {
                    name
                } else {
                    self.file_function_name(&name, token.span)
                };
                if self.next_generic_function(&name) {
                    let arguments = self.type_arguments()?;
                    if self.generics.structs.contains_key(&name) && self.at(&Token::ColonColon) {
                        generic_type = Some(self.instantiate_struct(&name, arguments, token.span)?);
                    } else {
                        let span = token.span.through(self.tokens[self.position - 1].span);
                        let name = self.instantiate_function(&name, arguments, token.span)?;
                        if self.consume(&Token::LeftParen) {
                            let (arguments, end) = self.arguments()?;
                            return Ok(Expression {
                                span: token.span.through(end),
                                kind: ExpressionKind::Call {
                                    name,
                                    name_span: token.span,
                                    arguments,
                                },
                            });
                        }
                        return Ok(Expression {
                            span,
                            kind: ExpressionKind::Variable {
                                name,
                                name_span: token.span,
                            },
                        });
                    }
                }
                if generic_type.is_none()
                    && self.generics.structs.contains_key(&name)
                    && self.at(&Token::Less)
                {
                    generic_type = Some(self.named_type(&name, token.span)?);
                }
                if self.consume(&Token::ColonColon) {
                    let qualifier_span = Span {
                        end: self.tokens[self.position - 1].span.start,
                        ..token.span
                    };
                    let ty = if let Some(ty) = generic_type {
                        ty
                    } else {
                        self.named_type(&name, token.span)?
                    };
                    self.type_annotations.push((qualifier_span, ty));
                    if let Type::Enum(id) = ty {
                        let (name, end) = self.identifier()?;
                        return Ok(Expression {
                            span: token.span.through(end),
                            kind: ExpressionKind::EnumValue {
                                id,
                                name,
                                name_span: end,
                            },
                        });
                    }
                    let Type::Struct(id) = ty else {
                        return Err(Diagnostic::new(
                            token.span,
                            "static calls require a struct type",
                        ));
                    };
                    let (name, name_span) = self.identifier()?;
                    if !self.consume(&Token::LeftParen) {
                        return Ok(Expression {
                            span: token.span.through(self.tokens[self.position - 1].span),
                            kind: ExpressionKind::StaticReference {
                                id,
                                name,
                                name_span,
                            },
                        });
                    }
                    let (arguments, end) = self.arguments()?;
                    return Ok(Expression {
                        span: token.span.through(end),
                        kind: ExpressionKind::StaticCall {
                            name_span,
                            id,
                            name,
                            arguments,
                        },
                    });
                }
                if !self.consume(&Token::LeftParen) {
                    return Ok(Expression {
                        kind: ExpressionKind::Variable {
                            name,
                            name_span: token.span,
                        },
                        span: token.span,
                    });
                }
                let (arguments, end) = self.arguments()?;
                return Ok(Expression {
                    span: token.span.through(end),
                    kind: ExpressionKind::Call {
                        name,
                        name_span: token.span,
                        arguments,
                    },
                });
            }
            Token::LeftParen => {
                if self.type_in_expression() {
                    let ty = self.type_name()?;
                    self.expect(
                        Token::RightParen,
                        "expected ')' after the compound literal type",
                    )?;
                    let mut literal = match ty {
                        Type::Struct(id) => self.struct_literal(Some(id))?,
                        Type::Array(id) => {
                            let mut literal = self.initializer()?;
                            if let ExpressionKind::ArrayLiteral { id: literal_id, .. } =
                                &mut literal.kind
                            {
                                *literal_id = Some(id);
                            } else {
                                return Err(Diagnostic::new(
                                    literal.span,
                                    "expected array elements inside braces",
                                ));
                            }
                            literal
                        }
                        _ => {
                            return Err(Diagnostic::new(
                                token.span,
                                "a compound literal needs a struct or array type",
                            ));
                        }
                    };
                    literal.span = token.span.through(literal.span);
                    return Ok(literal);
                }
                let mut expression = self.expression(1)?;
                let end = self.expect(Token::RightParen, "expected ')' after the expression")?;
                expression.span = token.span.through(end);
                return Ok(expression);
            }
            _ => {
                return Err(Diagnostic::new(
                    token.span,
                    format!("expected an expression, found {:?}", token.kind),
                ));
            }
        }
    }

    fn binary_operator(token: &Token) -> Option<(BinaryOperator, u8)> {
        return match token {
            Token::PipePipe => Some((BinaryOperator::Or, 1)),
            Token::AmpersandAmpersand => Some((BinaryOperator::And, 2)),
            Token::Pipe => Some((BinaryOperator::BitOr, 3)),
            Token::Caret => Some((BinaryOperator::BitXor, 4)),
            Token::Ampersand => Some((BinaryOperator::BitAnd, 5)),
            Token::EqualEqual => Some((BinaryOperator::Equal, 6)),
            Token::BangEqual => Some((BinaryOperator::NotEqual, 6)),
            Token::Less => Some((BinaryOperator::Less, 7)),
            Token::LessEqual => Some((BinaryOperator::LessEqual, 7)),
            Token::Greater => Some((BinaryOperator::Greater, 7)),
            Token::GreaterEqual => Some((BinaryOperator::GreaterEqual, 7)),
            Token::LessLess => Some((BinaryOperator::ShiftLeft, 8)),
            Token::GreaterGreater => Some((BinaryOperator::ShiftRight, 8)),
            Token::Plus => Some((BinaryOperator::Add, 9)),
            Token::Minus => Some((BinaryOperator::Subtract, 9)),
            Token::Star => Some((BinaryOperator::Multiply, 10)),
            Token::Slash => Some((BinaryOperator::Divide, 10)),
            Token::Percent => Some((BinaryOperator::Remainder, 10)),
            _ => None,
        };
    }

    fn identifier(&mut self) -> Result<(String, Span), Diagnostic> {
        let token = self.advance();
        return match token.kind {
            Token::Identifier(name) => Ok((name, token.span)),
            _ => Err(Diagnostic::new(token.span, "expected an identifier")),
        };
    }

    fn current(&self) -> &SpannedToken {
        if let Some(token) = &self.pending_greater {
            return token;
        }
        return &self.tokens[self.position];
    }

    fn at(&self, token: &Token) -> bool {
        return &self.current().kind == token;
    }

    fn next_at(&self, token: &Token) -> bool {
        return self
            .tokens
            .get(self.position + usize::from(self.pending_greater.is_none()))
            .is_some_and(|next| &next.kind == token);
    }

    fn advance(&mut self) -> SpannedToken {
        if let Some(token) = self.pending_greater.take() {
            return token;
        }
        let token = self.current().clone();
        if token.kind != Token::EndOfFile {
            self.position += 1;
        }
        return token;
    }

    fn consume(&mut self, token: &Token) -> bool {
        if self.at(token) {
            self.advance();
            return true;
        }
        return false;
    }

    fn expect(&mut self, token: Token, message: &str) -> Result<Span, Diagnostic> {
        if self.at(&token) {
            return Ok(self.advance().span);
        }
        return Err(Diagnostic::new(self.current().span, message));
    }
}
