use super::*;

#[derive(Clone)]
pub(super) struct Template {
    start: usize,
    body: usize,
    parameters: Vec<String>,
    span: Span,
}

#[derive(Default)]
pub(super) struct Templates {
    pub(super) structs: HashMap<String, Template>,
    functions: HashMap<String, Template>,
    pub(super) ends: HashMap<usize, usize>,
    pub(super) bindings: HashMap<String, Type>,
    pub(super) current_function: Option<String>,
    struct_instances: HashMap<(String, Vec<Type>), usize>,
    function_instances: HashMap<(String, Vec<Type>), String>,
    pub(super) struct_definitions: Vec<StructDefinition>,
    pub(super) function_definitions: Vec<Function>,
    depth: usize,
}

fn parameters(tokens: &[SpannedToken], mut position: usize) -> Option<(Vec<String>, usize)> {
    if tokens.get(position)?.kind != Token::Less {
        return None;
    }
    position += 1;
    let mut names = Vec::new();
    loop {
        let Token::Identifier(name) = &tokens.get(position)?.kind else {
            return None;
        };
        names.push(name.clone());
        position += 1;
        match tokens.get(position)?.kind {
            Token::Greater => return Some((names, position + 1)),
            Token::Comma => position += 1,
            _ => return None,
        }
    }
}

fn closing(
    tokens: &[SpannedToken],
    start: usize,
    open: Token,
    close: Token,
) -> Result<usize, Diagnostic> {
    let mut depth = 0usize;
    for (position, token) in tokens.iter().enumerate().skip(start) {
        if token.kind == open {
            depth += 1;
        }
        if token.kind == close {
            depth -= 1;
            if depth == 0 {
                return Ok(position);
            }
        }
        if token.kind == Token::EndOfFile {
            break;
        }
    }
    return Err(Diagnostic::new(
        tokens[start].span,
        "unterminated generic declaration",
    ));
}

impl Parser {
    pub(super) fn generic_declaration_ahead(&self) -> bool {
        if self.value_scopes.iter().rev().any(
            |scope| matches!(&self.current().kind, Token::Identifier(name) if scope.contains(name)),
        ) {
            return false;
        }
        let mut depth = 0usize;
        let mut position = self.position + 1;
        while let Some(token) = self.tokens.get(position) {
            match token.kind {
                Token::Less => depth += 1,
                Token::Greater => depth = depth.saturating_sub(1),
                Token::GreaterGreater => depth = depth.saturating_sub(2),
                Token::EndOfFile | Token::Semicolon => return false,
                _ => {}
            }
            position += 1;
            if depth == 0 {
                break;
            }
        }
        while self
            .tokens
            .get(position)
            .is_some_and(|token| matches!(token.kind, Token::Star | Token::Const | Token::Caret))
        {
            position += 1;
        }
        return matches!(
            self.tokens.get(position).map(|token| &token.kind),
            Some(Token::Identifier(_))
        );
    }

    pub(super) fn collect_templates(&mut self) -> Result<(), Diagnostic> {
        let mut position = 0;
        let mut depth = 0usize;
        let mut declaration_start = 0;
        while position < self.tokens.len() {
            let is_struct = depth == 0
                && self.tokens[position].kind == Token::Struct
                && self
                    .tokens
                    .get(position + 2)
                    .is_some_and(|token| token.kind == Token::Less);
            let name_position = position + usize::from(is_struct);
            if depth == 0
                && let Some(SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                }) = self.tokens.get(name_position).cloned()
                && let Some((names, after)) = parameters(&self.tokens, name_position + 1)
                && (is_struct
                    || self
                        .tokens
                        .get(after)
                        .is_some_and(|token| token.kind == Token::LeftParen))
            {
                let start = if is_struct {
                    position
                } else {
                    declaration_start
                };
                if self.tokens[start].kind == Token::Extern {
                    return Err(Diagnostic::new(
                        span,
                        "external declarations cannot be generic",
                    ));
                }
                let body = if is_struct {
                    after
                } else {
                    closing(&self.tokens, after, Token::LeftParen, Token::RightParen)? + 1
                };
                if !self
                    .tokens
                    .get(body)
                    .is_some_and(|token| token.kind == Token::LeftBrace)
                {
                    return Err(Diagnostic::new(
                        span,
                        "a generic declaration requires a body",
                    ));
                }
                let mut end = closing(&self.tokens, body, Token::LeftBrace, Token::RightBrace)? + 1;
                if is_struct {
                    if !self
                        .tokens
                        .get(end)
                        .is_some_and(|token| token.kind == Token::Semicolon)
                    {
                        return Err(Diagnostic::new(span, "expected ';' after generic struct"));
                    }
                    end += 1;
                }
                let mut seen = HashSet::new();
                for parameter in &names {
                    if matches!(parameter.as_str(), "Self" | "this") || !seen.insert(parameter) {
                        return Err(Diagnostic::new(
                            span,
                            "generic parameter names must be distinct and cannot use Self or this",
                        ));
                    }
                }
                let template = Template {
                    start,
                    body,
                    parameters: names,
                    span,
                };
                let registry = if is_struct {
                    &mut self.generics.structs
                } else {
                    &mut self.generics.functions
                };
                if registry.insert(name.clone(), template).is_some() {
                    return Err(Diagnostic::new(
                        span,
                        format!("generic declaration '{name}' is already defined"),
                    ));
                }
                self.generics.ends.insert(start, end);
                position = end;
                declaration_start = end;
                continue;
            }
            match self.tokens[position].kind {
                Token::LeftBrace => depth += 1,
                Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        declaration_start = position + 1;
                    }
                }
                Token::Semicolon if depth == 0 => declaration_start = position + 1,
                _ => {}
            }
            position += 1;
        }
        return Ok(());
    }

    pub(super) fn skip_template(&mut self) -> bool {
        if let Some(end) = self.generics.ends.get(&self.position) {
            self.position = *end;
            return true;
        }
        return false;
    }

    pub(super) fn skip_type_parameters(&mut self) -> Result<(), Diagnostic> {
        let Some((_, end)) = parameters(&self.tokens, self.position) else {
            return Err(Diagnostic::new(
                self.current().span,
                "expected generic parameter names",
            ));
        };
        self.position = end;
        return Ok(());
    }

    pub(super) fn next_generic_function(&self, name: &str) -> bool {
        return self.generics.functions.contains_key(name)
            && self.at(&Token::Less)
            && !self
                .value_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains(name));
    }

    fn greater(&mut self) -> Result<(), Diagnostic> {
        if self.consume(&Token::Greater) {
            return Ok(());
        }
        if self.at(&Token::GreaterGreater) {
            let token = self.advance();
            self.pending_greater = Some(SpannedToken {
                kind: Token::Greater,
                span: Span {
                    start: token.span.start + 1,
                    column: token.span.column + 1,
                    ..token.span
                },
            });
            return Ok(());
        }
        return Err(Diagnostic::new(
            self.current().span,
            "expected '>' after generic arguments",
        ));
    }

    pub(super) fn type_arguments(&mut self) -> Result<Vec<Type>, Diagnostic> {
        self.expect(
            Token::Less,
            "generic types and functions require explicit '<type>' arguments",
        )?;
        let mut arguments = Vec::new();
        loop {
            arguments.push(self.type_name()?);
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        self.greater()?;
        return Ok(arguments);
    }

    fn check_instantiation(
        &self,
        template: &Template,
        arguments: &[Type],
        span: Span,
    ) -> Result<(), Diagnostic> {
        if arguments.len() != template.parameters.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "generic declaration expects {} type argument(s), got {}",
                    template.parameters.len(),
                    arguments.len()
                ),
            ));
        }
        if self.generics.depth >= 48
            || self.generics.struct_instances.len() + self.generics.function_instances.len() >= 512
        {
            return Err(Diagnostic::new(
                span,
                "generic instantiation limit exceeded",
            ));
        }
        return Ok(());
    }

    pub(super) fn instantiate_struct(
        &mut self,
        name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        let key = (name.to_owned(), arguments.clone());
        if let Some(id) = self.generics.struct_instances.get(&key) {
            return Ok(Type::Struct(*id));
        }
        let template = self.generics.structs[name].clone();
        self.check_instantiation(&template, &arguments, span)?;
        let argument_names = arguments
            .iter()
            .map(|ty| self.generic_type_name(*ty))
            .collect::<Vec<_>>()
            .join(", ");
        let mut display_name = format!("{name}<{argument_names}>");
        if display_name.len() > 200 {
            display_name.truncate(197);
            display_name.push_str("...");
        }
        let id = self.struct_names.len();
        self.struct_names.push((display_name, span));
        self.generics.struct_instances.insert(key, id);
        let context = self.enter_template(&template, arguments, template.body);
        let definition = self.struct_body(id, name, template.span);
        self.leave_template(context);
        self.generics.struct_definitions.push(definition?);
        return Ok(Type::Struct(id));
    }

    pub(super) fn instantiate_function(
        &mut self,
        name: &str,
        arguments: Vec<Type>,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let key = (name.to_owned(), arguments.clone());
        if let Some(name) = self.generics.function_instances.get(&key) {
            return Ok(name.clone());
        }
        let template = self.generics.functions[name].clone();
        self.check_instantiation(&template, &arguments, span)?;
        let internal = format!("${}_{name}", self.generics.function_instances.len());
        self.generics
            .function_instances
            .insert(key, internal.clone());
        let context = self.enter_template(&template, arguments, template.start);
        self.generics.current_function = Some(internal.clone());
        let definition = self.function();
        self.leave_template(context);
        self.generics.function_definitions.push(definition?);
        return Ok(internal);
    }

    fn enter_template(
        &mut self,
        template: &Template,
        arguments: Vec<Type>,
        position: usize,
    ) -> Context {
        let bindings = template.parameters.iter().cloned().zip(arguments).collect();
        let context = Context {
            position: self.position,
            pending: self.pending_greater.take(),
            owner: self.current_struct.take(),
            function: self.generics.current_function.take(),
            bindings: std::mem::replace(&mut self.generics.bindings, bindings),
            scopes: std::mem::replace(&mut self.value_scopes, vec![HashSet::new()]),
        };
        self.position = position;
        self.generics.depth += 1;
        return context;
    }

    fn leave_template(&mut self, context: Context) {
        self.position = context.position;
        self.pending_greater = context.pending;
        self.current_struct = context.owner;
        self.generics.current_function = context.function;
        self.generics.bindings = context.bindings;
        self.value_scopes = context.scopes;
        self.generics.depth -= 1;
    }

    fn generic_type_name(&self, ty: Type) -> String {
        return match ty {
            Type::Integer(IntType::I32) => "int".to_owned(),
            Type::Integer(kind) => kind.c_name().to_owned(),
            Type::Bool => "bool".to_owned(),
            Type::Void => "void".to_owned(),
            Type::BoxInt => "BoxInt".to_owned(),
            Type::Struct(id) => self.struct_names[id].0.clone(),
            Type::Enum(id) => self.enum_names[id].0.clone(),
            Type::Pointer(id) => format!(
                "{}{}*",
                if self.pointers[id].mutable {
                    ""
                } else {
                    "const "
                },
                self.generic_type_name(self.pointers[id].pointee)
            ),
            Type::Array(id) => format!(
                "{}[{}]",
                self.generic_type_name(self.arrays[id].element),
                self.arrays[id].length
            ),
            Type::Borrow { pointee, mutable } => format!(
                "{}{}^",
                if mutable { "" } else { "const " },
                self.generic_type_name(pointee.ty())
            ),
            Type::Function(id) => format!("function#{id}"),
        };
    }
}

struct Context {
    position: usize,
    pending: Option<SpannedToken>,
    owner: Option<usize>,
    function: Option<String>,
    bindings: HashMap<String, Type>,
    scopes: Vec<HashSet<String>>,
}
