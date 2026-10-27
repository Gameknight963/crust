use super::*;

#[derive(Clone)]
pub(super) struct Template {
    pub(super) is_union: bool,
    start: usize,
    body: usize,
    parameters: Vec<String>,
    parameter_spans: Vec<Span>,
    constraints: Vec<(String, String)>,
    span: Span,
}

#[derive(Default, Clone)]
pub(super) struct Templates {
    interfaces: HashMap<String, Interface>,
    checked_interfaces: HashSet<(Type, String)>,
    pub(super) interface_checks: Vec<InterfaceCheck>,
    pub(super) structs: HashMap<String, Template>,
    functions: HashMap<String, Template>,
    pub(super) ends: HashMap<usize, usize>,
    pub(super) bindings: HashMap<String, Type>,
    pub(super) binding_declarations: HashMap<String, Span>,
    pub(super) constraint_references: Vec<(String, Span)>,
    pub(super) current_function: Option<String>,
    struct_instances: HashMap<(String, Vec<Type>), usize>,
    function_instances: HashMap<(String, Vec<Type>), String>,
    pub(super) struct_definitions: Vec<StructDefinition>,
    pub(super) function_definitions: Vec<Function>,
    depth: usize,
}

#[derive(Clone)]
struct Interface {
    name_span: Span,
    body: usize,
    all_static: bool,
}

type TypeParameters = Vec<(String, Vec<String>)>;

fn parameters(tokens: &[SpannedToken], mut position: usize) -> Option<(TypeParameters, usize)> {
    if tokens.get(position)?.kind != Token::Less {
        return None;
    }
    position += 1;
    let mut names = Vec::new();
    loop {
        let Token::Identifier(name) = &tokens.get(position)?.kind else {
            return None;
        };
        let name = name.clone();
        position += 1;
        let mut constraints = Vec::new();
        if tokens.get(position)?.kind == Token::Colon {
            position += 1;
            loop {
                let Token::Identifier(constraint) = &tokens.get(position)?.kind else {
                    return None;
                };
                constraints.push(constraint.clone());
                position += 1;
                if tokens.get(position)?.kind != Token::Plus {
                    break;
                }
                position += 1;
            }
        }
        names.push((name, constraints));
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
    pub(super) fn has_function_template(&self, name: &str) -> bool {
        return self.generics.functions.contains_key(name);
    }
    pub(super) fn generic_declaration_ahead(&self) -> bool {
        if self.value_scopes.iter().rev().any(
            |scope| matches!(&self.current().kind, Token::Identifier(name) if scope.contains(name)),
        ) {
            return false;
        }
        let mut depth = 0usize;
        let mut brackets = 0usize;
        let mut position = self.position + 1;
        while let Some(token) = self.tokens.get(position) {
            match token.kind {
                Token::LeftBracket => brackets += 1,
                Token::RightBracket => brackets = brackets.saturating_sub(1),
                Token::Less if brackets == 0 => depth += 1,
                Token::Greater if brackets == 0 => depth = depth.saturating_sub(1),
                Token::GreaterGreater if brackets == 0 => depth = depth.saturating_sub(2),
                Token::EndOfFile | Token::Semicolon => return false,
                _ => {}
            }
            position += 1;
            if depth == 0 {
                break;
            }
        }
        loop {
            match self.tokens.get(position).map(|token| &token.kind) {
                Some(Token::Star | Token::Const | Token::Volatile | Token::Caret) => position += 1,
                Some(Token::LeftBracket) => {
                    let mut brackets = 1usize;
                    position += 1;
                    while brackets != 0 {
                        match self.tokens.get(position).map(|token| &token.kind) {
                            Some(Token::LeftBracket) => brackets += 1,
                            Some(Token::RightBracket) => brackets -= 1,
                            None | Some(Token::EndOfFile | Token::Semicolon) => return false,
                            _ => {}
                        }
                        position += 1;
                    }
                }
                _ => break,
            }
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
            let all_static = self.tokens[position].kind == Token::Static;
            let interface_position = position + usize::from(all_static);
            if depth == 0
                && self
                    .tokens
                    .get(interface_position)
                    .is_some_and(|token| token.kind == Token::Interface)
            {
                let Some(SpannedToken {
                    kind: Token::Identifier(name),
                    span,
                }) = self.tokens.get(interface_position + 1)
                else {
                    return Err(Diagnostic::new(
                        self.tokens[interface_position].span,
                        "expected an interface name",
                    ));
                };
                let body = interface_position + 2;
                if !self
                    .tokens
                    .get(body)
                    .is_some_and(|token| token.kind == Token::LeftBrace)
                {
                    return Err(Diagnostic::new(
                        *span,
                        "expected '{' after the interface name",
                    ));
                }
                let end = closing(&self.tokens, body, Token::LeftBrace, Token::RightBrace)? + 1;
                if !self
                    .tokens
                    .get(end)
                    .is_some_and(|token| token.kind == Token::Semicolon)
                {
                    return Err(Diagnostic::new(
                        *span,
                        "expected ';' after interface declaration",
                    ));
                }
                if self
                    .generics
                    .interfaces
                    .insert(
                        name.clone(),
                        Interface {
                            body,
                            all_static,
                            name_span: *span,
                        },
                    )
                    .is_some()
                {
                    return Err(Diagnostic::new(
                        *span,
                        format!("interface '{name}' is already defined"),
                    ));
                }
                self.source_declarations.push(SourceDeclaration {
                    kind: SourceDeclarationKind::Interface,
                    name_span: *span,
                    header_span: Span {
                        end: self.tokens[body].span.start,
                        ..self.tokens[position].span
                    },
                    span: self.tokens[position].span.through(self.tokens[end].span),
                    container: None,
                });
                self.generics.ends.insert(position, end + 1);
                position = end + 1;
                declaration_start = position;
                continue;
            }
            let is_struct = depth == 0
                && matches!(self.tokens[position].kind, Token::Struct | Token::Union)
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
                for (parameter, _) in &names {
                    if matches!(parameter.as_str(), "Self" | "this") || !seen.insert(parameter) {
                        return Err(Diagnostic::new(
                            span,
                            "generic parameter names must be distinct and cannot use Self or this",
                        ));
                    }
                }
                let parameter_spans =
                    self.generic_parameter_declarations(name_position + 2, after, &name);
                self.source_declarations.push(SourceDeclaration {
                    kind: if is_struct {
                        if self.tokens[position].kind == Token::Union {
                            SourceDeclarationKind::Union
                        } else {
                            SourceDeclarationKind::Struct
                        }
                    } else {
                        SourceDeclarationKind::Function
                    },
                    name_span: span,
                    header_span: Span {
                        end: self.tokens[body].span.start,
                        ..self.tokens[start].span
                    },
                    span: self.tokens[start].span.through(self.tokens[end - 1].span),
                    container: None,
                });
                let template = Template {
                    parameter_spans,
                    is_union: is_struct && self.tokens[position].kind == Token::Union,
                    start,
                    body,
                    constraints: names
                        .iter()
                        .flat_map(|(parameter, constraints)| {
                            constraints
                                .iter()
                                .map(move |constraint| (parameter.clone(), constraint.clone()))
                        })
                        .collect(),
                    parameters: names.into_iter().map(|(name, _)| name).collect(),
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
                Token::Semicolon | Token::EndOfFile if depth == 0 => {
                    declaration_start = position + 1
                }
                _ => {}
            }
            position += 1;
        }
        for template in self
            .generics
            .structs
            .values()
            .chain(self.generics.functions.values())
        {
            for (_, name) in &template.constraints {
                if !self.generics.interfaces.contains_key(name) {
                    return Err(Diagnostic::new(
                        template.span,
                        format!("unknown interface '{name}'"),
                    ));
                }
            }
        }
        for (name, span) in &self.generics.constraint_references {
            self.type_references.push(TypeReference {
                span: *span,
                declaration: Some(TypeDeclaration::Source(
                    self.generics.interfaces[name].name_span,
                )),
            });
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
        self.type_references.push(TypeReference {
            span,
            declaration: Some(TypeDeclaration::Source(self.generics.structs[name].span)),
        });
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
        if template.is_union {
            self.union_tags.insert(id);
        }
        self.generics.struct_instances.insert(key, id);
        let context = self.enter_template(&template, arguments.clone(), template.body);
        let definition =
            self.struct_body(id, name, self.tokens[template.start].span, template.span);
        self.leave_template(context);
        self.generics.struct_definitions.push(definition?);
        self.instantiate_constraints(&template, &arguments, span)?;
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
        let context = self.enter_template(&template, arguments.clone(), template.start);
        self.generics.current_function = Some(internal.clone());
        let definition = self.function();
        self.leave_template(context);
        self.generics.function_definitions.push(definition?);
        self.instantiate_constraints(&template, &arguments, span)?;
        return Ok(internal);
    }

    fn instantiate_constraints(
        &mut self,
        template: &Template,
        arguments: &[Type],
        span: Span,
    ) -> Result<(), Diagnostic> {
        for (parameter, name) in &template.constraints {
            let index = template
                .parameters
                .iter()
                .position(|item| item == parameter)
                .unwrap();
            let ty = arguments[index];
            if !self.generics.checked_interfaces.insert((ty, name.clone())) {
                continue;
            }
            let Type::Struct(id) = ty else {
                return Err(Diagnostic::new(
                    span,
                    format!("interface '{name}' requires a struct type"),
                ));
            };
            let interface = self.generics.interfaces[name].clone();
            let signature_template = Template {
                parameter_spans: vec![interface.name_span],
                is_union: false,
                start: interface.body,
                body: interface.body,
                parameters: vec!["Self".to_owned()],
                constraints: Vec::new(),
                span,
            };
            let context = self.enter_template(&signature_template, vec![ty], interface.body + 1);
            self.current_struct = Some(id);
            let requirements = self.interface_requirements(id, interface.all_static);
            self.leave_template(context);
            self.generics.interface_checks.push(InterfaceCheck {
                name: name.clone(),
                ty,
                span,
                requirements: requirements?,
            });
        }
        return Ok(());
    }

    fn interface_requirements(
        &mut self,
        id: usize,
        all_static: bool,
    ) -> Result<Vec<Function>, Diagnostic> {
        let mut requirements = Vec::new();
        let mut names = HashSet::new();
        while !self.at(&Token::RightBrace) {
            let start = self.current().span;
            let unsafe_prefix = self.consume(&Token::Unsafe);
            let is_static = self.consume(&Token::Static) || all_static;
            let requires_unsafe = self.consume(&Token::Unsafe) || unsafe_prefix;
            let result = self.type_name()?;
            let convention = self.calling_convention()?.unwrap_or(CallingConvention::C);
            let (name, span) = self.identifier()?;
            if !names.insert(name.clone()) {
                return Err(Diagnostic::new(
                    span,
                    "interface member is already declared",
                ));
            }
            let key = if is_static {
                static_method_name(id, &name)
            } else {
                method_name(id, &name)
            };
            let mut function = self.function_body(
                result,
                (key, span),
                start,
                Some(id),
                is_static,
                FunctionDeclaration::Requirement,
            )?;
            function.requires_unsafe = requires_unsafe;
            function.calling_convention =
                Self::signature_convention(convention, function.variadic, span)?;
            if function.variadic {
                return Err(Diagnostic::new(
                    start,
                    "interface members require fixed parameter lists",
                ));
            }
            let mut parameters = HashSet::new();
            for parameter in &function.parameters {
                if !parameters.insert(&parameter.name) || (is_static && parameter.name == "this") {
                    return Err(Diagnostic::new(
                        parameter.span,
                        "invalid or duplicate interface parameter name",
                    ));
                }
            }
            requirements.push(function);
        }
        return Ok(requirements);
    }

    pub(super) fn validate_interfaces(&mut self) -> Result<(), Diagnostic> {
        if self.generics.interfaces.is_empty() {
            return Ok(());
        }
        // Resolve signatures in an isolated parser. Abstract Self may instantiate
        // other generic signatures, whose temporary types must not enter output.
        let mut validation = self.clone();
        let id = validation.struct_names.len();
        validation
            .struct_names
            .push(("interface Self".to_owned(), self.current().span));
        let mut interfaces: Vec<_> = validation
            .generics
            .interfaces
            .iter()
            .map(|(name, interface)| (name.clone(), interface.clone()))
            .collect();
        interfaces.sort_by_key(|(_, interface)| interface.body);
        for (name, interface) in interfaces {
            validation.position = interface.body + 1;
            validation.pending_greater = None;
            validation.current_struct = Some(id);
            validation.generics.bindings = HashMap::from([("Self".to_owned(), Type::Struct(id))]);
            validation.generics.binding_declarations =
                HashMap::from([("Self".to_owned(), interface.name_span)]);
            let references_start = validation.type_references.len();
            let annotations_start = validation.type_annotations.len();
            let members = validation.interface_requirements(id, interface.all_static)?;
            self.type_references.extend(
                validation.type_references[references_start..]
                    .iter()
                    .cloned(),
            );
            self.type_annotations.extend(
                validation.type_annotations[annotations_start..]
                    .iter()
                    .filter(|(_, ty)| {
                        matches!(
                            ty,
                            Type::Integer(_) | Type::Bool | Type::Void | Type::BoxInt
                        )
                    })
                    .copied(),
            );
            for member in members {
                self.source_declarations.push(SourceDeclaration {
                    kind: SourceDeclarationKind::Method,
                    name_span: member.name_span,
                    header_span: Span {
                        end: member.body.span.start,
                        ..member.span
                    },
                    span: member.span,
                    container: Some(name.clone()),
                });
                for parameter in member
                    .parameters
                    .into_iter()
                    .filter(|parameter| !parameter.synthetic)
                {
                    self.source_declarations.push(SourceDeclaration {
                        kind: SourceDeclarationKind::Parameter,
                        name_span: parameter.span,
                        header_span: parameter.declaration_span,
                        span: parameter.declaration_span,
                        container: Some(name.clone()),
                    });
                }
            }
        }
        return Ok(());
    }

    fn enter_template(
        &mut self,
        template: &Template,
        arguments: Vec<Type>,
        position: usize,
    ) -> Context {
        let bindings = template.parameters.iter().cloned().zip(arguments).collect();
        let binding_declarations = template
            .parameters
            .iter()
            .cloned()
            .zip(template.parameter_spans.iter().copied())
            .collect();
        let context = Context {
            position: self.position,
            pending: self.pending_greater.take(),
            owner: self.current_struct.take(),
            function: self.generics.current_function.take(),
            bindings: std::mem::replace(&mut self.generics.bindings, bindings),
            binding_declarations: std::mem::replace(
                &mut self.generics.binding_declarations,
                binding_declarations,
            ),
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
        self.generics.binding_declarations = context.binding_declarations;
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
            Type::Pointer(id) => {
                let pointer = self.pointers[id];
                format!(
                    "{}*",
                    Qualifiers {
                        readonly: !pointer.mutable,
                        volatile: pointer.volatile
                    }
                    .describe(pointer.pointee, self.generic_type_name(pointer.pointee))
                )
            }
            Type::Array(id) => format!(
                "{}[{}]",
                self.generic_type_name(self.arrays[id].element),
                self.arrays[id].length
            ),
            Type::Borrow {
                pointee,
                mutable,
                volatile,
            } => format!(
                "{}&",
                Qualifiers {
                    readonly: !mutable,
                    volatile
                }
                .describe(pointee.ty(), self.generic_type_name(pointee.ty()))
            ),
            Type::Function(id) => format!("function#{id}"),
        };
    }
}

struct Context {
    binding_declarations: HashMap<String, Span>,
    position: usize,
    pending: Option<SpannedToken>,
    owner: Option<usize>,
    function: Option<String>,
    bindings: HashMap<String, Type>,
    scopes: Vec<HashSet<String>>,
}
