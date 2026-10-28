use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::types::Types;
mod captures;
mod drop_borrows;
mod storage;

fn return_borrow_sources(
    function: &Function,
    types: &Types,
) -> Result<Option<Vec<usize>>, Diagnostic> {
    let aggregate =
        !function.return_type.is_borrow() && types.carries_borrows(function.return_type);
    let mutable = matches!(function.return_type, Type::Borrow { mutable: true, .. });
    if !function.return_type.is_borrow() && !aggregate {
        if function.return_borrows.is_some() {
            return Err(Diagnostic::new(
                function.span,
                "borrows applies to a borrowed return",
            ));
        }
        return Ok(None);
    }
    let sources = if let Some(names) = &function.return_borrows {
        if names.is_empty() {
            return Err(Diagnostic::new(
                function.span,
                "borrows requires at least one source",
            ));
        }
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for (name, span) in names {
            let index = function
                .parameters
                .iter()
                .position(|parameter| parameter.name == *name && types.carries_borrows(parameter.ty))
                .ok_or_else(|| {
                    Diagnostic::new(*span, "a borrow source must name a borrowed parameter or a value containing references")
                })?;
            if !seen.insert(index) {
                return Err(Diagnostic::new(*span, "duplicate borrow source"));
            }
            sources.push(index);
        }
        sources
    } else {
        let sources = function
            .parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| types.carries_borrows(parameter.ty))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if sources.len() != 1 && !(aggregate && sources.is_empty()) {
            return Err(Diagnostic::new(
                function.span,
                "borrowed return lifetime contracts require one input or an explicit borrows(...) clause",
            ));
        }
        sources
    };
    if mutable
        && sources.iter().any(|index| {
            !matches!(
                function.parameters[*index].ty,
                Type::Borrow { mutable: true, .. }
            ) && !types.contains_mutable_borrows(function.parameters[*index].ty)
        })
    {
        return Err(Diagnostic::new(
            function.span,
            "a mutable borrowed return requires mutable source parameters",
        ));
    }
    return Ok(Some(sources));
}

fn native_main_parameters(parameters: &[Type], types: &Types) -> bool {
    let [Type::Integer(IntType::I32), Type::Pointer(outer)] = parameters else {
        return false;
    };
    let outer = types.pointers[*outer];
    let Type::Pointer(inner) = outer.pointee else {
        return false;
    };
    let inner = types.pointers[inner];
    return outer.mutable
        && inner.mutable
        && !outer.volatile
        && !inner.volatile
        && inner.pointee == Type::Integer(IntType::Char);
}

// Code generation accepts only a program that has passed this checker.
pub(crate) struct CheckedProgram {
    analysis: AnalyzedProgram,
}

impl CheckedProgram {
    pub fn into_analysis(self) -> AnalyzedProgram {
        return self.analysis;
    }
}

impl std::ops::Deref for CheckedProgram {
    type Target = AnalyzedProgram;

    fn deref(&self) -> &Self::Target {
        return &self.analysis;
    }
}

pub(crate) struct AnalyzedProgram {
    pub kind: crate::ProgramKind,
    pub program: Program,
    pub types: Types,
    pub analyses: HashMap<String, FunctionAnalysis>,
}

#[derive(Default)]
pub(crate) struct FunctionAnalysis {
    // Reference-producing calls and bindings retain pointer storage in C.
    // Value contexts read their referents without consuming an owner.
    pub reference_reads: HashMap<Span, Type>,
    pub place_volatile: HashMap<Span, bool>,
    pub calls: HashMap<Span, CallSite>,
    pub receiver_types: HashMap<Span, Type>,
    pub symbol_uses: HashMap<Span, Span>,
    pub function_values: HashMap<Span, String>,
    pub expression_types: HashMap<Span, Type>,
    // Lowered calls retain receiver-first evaluation and ordinary call rules.
    pub method_calls: HashMap<Span, Expression>,
    pub conversions: HashMap<Span, Type>,
    pub constants: HashMap<Span, i128>,
}

pub(crate) struct CallSite {
    pub callee: Span,
    pub target: CallTarget,
    pub arguments: Vec<Span>,
}

pub(crate) enum CallTarget {
    Function(String),
    Callback(Type),
}

#[derive(Clone, PartialEq, Eq)]
struct Signature {
    calling_convention: CallingConvention,
    link_name: Option<String>,
    variadic: bool,
    requires_unsafe: bool,
    external: bool,
    return_type: Type,
    parameters: Vec<Type>,
    return_borrow: Option<Vec<usize>>,
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
    storage: Option<usize>,
    volatile: bool,
    external: bool,
    mutable: bool,
    declaration: Option<Span>,
    ty: Type,
    state: State,
    reference: Option<usize>,
}

type Scopes = Vec<HashMap<String, Variable>>;

struct Place {
    volatile: bool,
    ty: Type,
    mutable: bool,
    root: (usize, String),
    fields: Vec<String>,
    via: Option<usize>,
}

#[derive(Clone)]
struct Loan {
    id: usize,
    root: (usize, String),
    fields: Vec<String>,
    mutable: bool,
    parents: Vec<usize>,
    alternatives: Vec<((usize, String), Vec<String>)>,
    scope: Option<usize>,
}

struct LoopContext {
    is_switch: bool,
    scope_count: usize,
    breaks: Vec<Scopes>,
    continues: Vec<(Scopes, Span)>,
}

pub(crate) fn check(program: Program) -> Result<CheckedProgram, Diagnostic> {
    return check_with_kind(program, crate::ProgramKind::Executable);
}

pub(crate) fn check_with_kind(
    program: Program,
    kind: crate::ProgramKind,
) -> Result<CheckedProgram, Diagnostic> {
    return Ok(CheckedProgram {
        analysis: analyze(program, false, kind)?,
    });
}

pub(crate) fn analyze_completion_with_kind(
    program: Program,
    kind: crate::ProgramKind,
) -> Result<AnalyzedProgram, Diagnostic> {
    return analyze(program, true, kind);
}

fn analyze(
    mut program: Program,
    completion: bool,
    kind: crate::ProgramKind,
) -> Result<AnalyzedProgram, Diagnostic> {
    if program.native_headers.is_empty()
        && let Some(definition) = program
            .enums
            .iter()
            .find(|definition| definition.native_name.is_some())
    {
        return Err(Diagnostic::new(
            definition.span,
            "native enums require an extern header declaration",
        ));
    }
    let imported: HashSet<_> = program
        .functions
        .iter()
        .filter(|function| function.dll_import)
        .filter_map(|function| function.link_name.clone())
        .chain(
            program
                .globals
                .iter()
                .filter(|global| global.dll_import)
                .map(|global| global.link_name.clone()),
        )
        .collect();
    for function in &mut program.functions {
        function.dll_import = function
            .link_name
            .as_ref()
            .is_some_and(|name| imported.contains(name));
    }
    for global in &mut program.globals {
        global.dll_import = imported.contains(&global.link_name);
    }
    let mut types = Types::build(&program)?;
    let mut analyses = HashMap::new();
    let mut signatures = HashMap::new();
    let mut native_signatures = HashMap::new();
    let mut native_definitions = HashSet::new();
    let mut function_declarations = HashMap::new();
    for function in &program.functions {
        if !function.external && function.dll_import {
            return Err(Diagnostic::new(
                function.span,
                "a DLL import cannot have a function body",
            ));
        }
        function_declarations
            .entry(function.name.clone())
            .or_insert(function.name_span);
    }
    let mut globals: HashMap<String, Variable> = HashMap::new();
    let mut global_symbols = HashMap::new();
    let mut native_globals = HashMap::new();
    let mut defined_globals = HashSet::new();
    for (storage, global) in program.globals.iter().enumerate() {
        if !global.external
            && global.initializer.is_none()
            && storage::has_private_fields(global.ty, &types)
        {
            return Err(Diagnostic::new(
                global.span,
                "static storage containing private fields requires an explicit initializer",
            ));
        }
        if !global.external && !defined_globals.insert(global.name.clone()) {
            return Err(Diagnostic::new(
                global.name_span,
                "global variable has multiple definitions",
            ));
        }
        if global.ty == Type::Void
            || !types.is_sized(global.ty)
            || types.owns(global.ty)
            || types.carries_borrows(global.ty)
            || matches!(global.ty, Type::Function(_))
            || (global.external && matches!(global.ty, Type::Array(_)))
        {
            return Err(Diagnostic::new(
                global.span,
                "external variables require complete copyable C values or raw pointers",
            ));
        }
        if matches!(
            global.name.as_str(),
            "Self" | "this" | "print" | "move" | "box_int" | "slice"
        ) || types.enumerator(&global.name).is_some()
            || function_declarations.contains_key(&global.name)
        {
            return Err(Diagnostic::new(
                global.name_span,
                "external variable name is already defined or reserved",
            ));
        }
        if global.link_name == "main"
            || program
                .functions
                .iter()
                .any(|function| function.link_name.as_ref() == Some(&global.link_name))
        {
            return Err(Diagnostic::new(
                global.span,
                "a C symbol cannot name both a function and an external variable",
            ));
        }
        let signature = (global.ty, global.readonly, global.volatile);
        if let Some(previous) = native_globals.insert(&global.link_name, signature)
            && previous != signature
        {
            return Err(Diagnostic::new(
                global.span,
                "conflicting declarations for a C variable symbol",
            ));
        }
        if let Some(previous) = global_symbols.insert(global.name.clone(), global.link_name.clone())
            && (previous != global.link_name
                || globals[&global.name].ty != global.ty
                || globals[&global.name].mutable == global.readonly
                || globals[&global.name].volatile != global.volatile)
        {
            return Err(Diagnostic::new(
                global.span,
                "conflicting external variable declarations",
            ));
        }
        globals.entry(global.name.clone()).or_insert(Variable {
            storage: Some(storage),
            volatile: global.volatile,
            external: global.external,
            mutable: !global.readonly,
            declaration: Some(global.name_span),
            ty: global.ty,
            state: State::Available,
            reference: None,
        });
    }
    for (name, return_type, parameters) in [
        ("print", Type::Void, vec![Type::Integer(IntType::I32)]),
        ("box_int", Type::BoxInt, vec![Type::Integer(IntType::I32)]),
        ("move", Type::BoxInt, vec![Type::BoxInt]),
        (
            "slice",
            Type::Void,
            vec![Type::Void, Type::Integer(IntType::named("size_t").unwrap())],
        ),
    ] {
        signatures.insert(
            name.to_owned(),
            Signature {
                calling_convention: CallingConvention::C,
                link_name: None,
                variadic: false,
                requires_unsafe: false,
                external: false,
                return_type,
                parameters,
                return_borrow: None,
            },
        );
    }

    // Signatures are known before bodies, allowing forward calls and recursion.
    for function in &program.functions {
        if (function.return_type != Type::Void && !types.is_sized(function.return_type))
            || function
                .parameters
                .iter()
                .any(|parameter| !types.is_sized(parameter.ty))
        {
            return Err(Diagnostic::new(
                function.span,
                "function values require complete object types. Use pointers for opaque structs",
            ));
        }
        if function.link_name.is_some()
            && (types.carries_borrows(function.return_type)
                || types.owns(function.return_type)
                || matches!(function.return_type, Type::Array(_))
                || function.parameters.iter().any(|parameter| {
                    types.carries_borrows(parameter.ty)
                        || types.owns(parameter.ty)
                        || matches!(parameter.ty, Type::Array(_))
                }))
        {
            return Err(Diagnostic::new(
                function.span,
                "external functions require C values and raw pointers; pass arrays by pointer",
            ));
        }
        if types.enumerator(&function.name).is_some() {
            return Err(Diagnostic::new(
                function.span,
                "function name conflicts with an enum value",
            ));
        }
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
        let return_borrow = return_borrow_sources(function, &types)?;
        let signature = Signature {
            calling_convention: function.calling_convention,
            link_name: function.link_name.clone(),
            variadic: function.variadic,
            requires_unsafe: function.requires_unsafe,
            external: function.external,
            return_type: function.return_type,
            return_borrow,
            parameters: function
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect(),
        };
        if let Some(symbol) = &function.link_name {
            if !function.external && symbol == "main" {
                return Err(Diagnostic::new(
                    function.span,
                    "C export name 'main' is reserved for the program entry point",
                ));
            }
            let mut native_signature = signature.clone();
            native_signature.external = true;
            if let Some(previous) = native_signatures.get(symbol)
                && previous != &native_signature
            {
                return Err(Diagnostic::new(
                    function.span,
                    format!("conflicting declarations for C symbol '{symbol}'"),
                ));
            }
            native_signatures.insert(symbol.clone(), native_signature);
            if !function.external && !native_definitions.insert(symbol.clone()) {
                return Err(Diagnostic::new(
                    function.span,
                    format!("C symbol '{symbol}' has more than one definition"),
                ));
            }
        }
        if let Some(previous) = signatures.get(&function.name) {
            if previous.link_name.is_some() && signature.link_name.is_some() {
                let mut compatible = previous.clone();
                compatible.external = signature.external;
                if compatible == signature {
                    if previous.external && !signature.external {
                        signatures.insert(function.name.clone(), signature);
                    }
                    continue;
                }
                return Err(Diagnostic::new(
                    function.span,
                    format!("conflicting C declarations for '{}'", function.name),
                ));
            }
            return Err(Diagnostic::new(
                function.span,
                format!("{} is already defined or reserved", function.description()),
            ));
        }
        signatures.insert(function.name.clone(), signature);
    }

    for constraint in &program.interface_checks {
        for requirement in &constraint.requirements {
            let expected_sources = return_borrow_sources(requirement, &types)?;
            let expected: Vec<_> = requirement
                .parameters
                .iter()
                .map(|parameter| parameter.ty)
                .collect();
            let satisfied = signatures.get(&requirement.name).is_some_and(|signature| {
                signature.return_type == requirement.return_type
                    && signature.variadic == requirement.variadic
                    && signature.calling_convention == requirement.calling_convention
                    && signature.parameters == expected
                    && signature.return_borrow.as_ref().is_none_or(|sources| {
                        expected_sources.as_ref().is_some_and(|expected| {
                            sources.iter().all(|index| expected.contains(index))
                        })
                    })
                    && (!signature.requires_unsafe || requirement.requires_unsafe)
            });
            if !satisfied {
                return Err(Diagnostic::new(
                    constraint.span,
                    format!(
                        "type '{}' does not satisfy interface '{}': missing or incompatible {}",
                        types.describe(constraint.ty),
                        constraint.name,
                        requirement.description()
                    ),
                ));
            }
        }
    }

    if kind == crate::ProgramKind::Library {
        if let Some(function) = program
            .functions
            .iter()
            .find(|function| function.name == "main")
        {
            return Err(Diagnostic::new(
                function.span,
                "library targets cannot define main",
            ));
        }
    } else {
        match signatures.get("main") {
            Some(signature)
                if signature.return_type == Type::Integer(IntType::I32)
                    && !signature.external
                    && !signature.requires_unsafe
                    && signature.calling_convention == CallingConvention::C
                    && (signature.parameters.is_empty()
                        || native_main_parameters(&signature.parameters, &types)) => {}
            Some(_) => {
                let function = program
                    .functions
                    .iter()
                    .find(|function| function.name == "main")
                    .unwrap();
                return Err(Diagnostic::new(
                    function.span,
                    "entry point must be int main(), int main(void), or int main(int argc, char** argv)",
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
                    "program needs an int main() entry point",
                ));
            }
        }
    }

    for function in &program.functions {
        if function.external {
            continue;
        }
        let mut analysis = FunctionAnalysis::default();
        let mut checker = FunctionChecker {
            reference_reads: &mut analysis.reference_reads,
            place_volatile: &mut analysis.place_volatile,
            calls: &mut analysis.calls,
            completion,
            receiver_types: &mut analysis.receiver_types,
            function_declarations: &function_declarations,
            symbol_uses: &mut analysis.symbol_uses,
            signatures: &signatures,
            global_symbols: &global_symbols,
            global_storage: &program.globals,
            scopes: vec![globals.clone(), HashMap::new()],
            return_type: function.return_type,
            return_borrow_root: signatures[&function.name]
                .return_borrow
                .as_ref()
                .map(|indices| {
                    indices
                        .iter()
                        .map(|index| (1, function.parameters[*index].name.clone()))
                        .collect()
                }),
            loans: Vec::new(),
            next_loan: 0,
            references: HashMap::new(),
            capture_bindings: HashMap::new(),
            capture_expressions: HashMap::new(),
            capture_return_roots: signatures[&function.name]
                .return_borrow
                .as_ref()
                .into_iter()
                .flatten()
                .filter(|index| function.parameters[**index].ty.is_borrow())
                .map(|index| (1, function.parameters[*index].name.clone()))
                .collect(),
            types: &mut types,
            expression_types: &mut analysis.expression_types,
            method_calls: &mut analysis.method_calls,
            loops: Vec::new(),
            conversions: &mut analysis.conversions,
            constants: &mut analysis.constants,
            unevaluated: false,
            unsafe_depth: usize::from(function.requires_unsafe),
            current_struct: if let Some(method) = function
                .name
                .strip_prefix('@')
                .or_else(|| function.name.strip_prefix('#'))
            {
                Some(method.split_once('_').unwrap().0.parse().unwrap())
            } else {
                function
                    .name
                    .strip_prefix('~')
                    .map(|id| id.parse().unwrap())
            },
            function_values: &mut analysis.function_values,
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
                if parameter.synthetic {
                    None
                } else {
                    Some(parameter.span)
                },
            )?;
            checker.initialize_captures(parameter)?;
        }

        if let Some(sources) = &function.return_borrows {
            for (name, span) in sources {
                if let Some(parameter) = function
                    .parameters
                    .iter()
                    .find(|parameter| parameter.name == *name)
                {
                    checker.symbol_uses.insert(*span, parameter.span);
                }
            }
        }
        let returns = match checker.statements(&function.body.statements) {
            Ok(returns) => returns,
            Err(_) if completion => false,
            Err(error) => return Err(error),
        };
        if !completion && function.return_type != Type::Void && !returns {
            return Err(Diagnostic::new(
                function.span,
                format!(
                    "{} must return a value on every path",
                    function.description()
                ),
            ));
        }
        if !completion {
            checker.check_scope_drops(1, function.body.span)?;
        }
        analyses.insert(function.name.clone(), analysis);
    }

    for global in &program.globals {
        if global.initializer.is_some() {
            storage::validate(global, &analyses[&format!("?init_{}", global.name)], &types)?;
        }
    }
    return Ok(AnalyzedProgram {
        kind,
        program,
        types,
        analyses,
    });
}

struct FunctionChecker<'program> {
    reference_reads: &'program mut HashMap<Span, Type>,
    place_volatile: &'program mut HashMap<Span, bool>,
    calls: &'program mut HashMap<Span, CallSite>,
    completion: bool,
    receiver_types: &'program mut HashMap<Span, Type>,
    global_symbols: &'program HashMap<String, String>,
    global_storage: &'program [ExternalVariable],
    function_declarations: &'program HashMap<String, Span>,
    symbol_uses: &'program mut HashMap<Span, Span>,
    function_values: &'program mut HashMap<Span, String>,
    signatures: &'program HashMap<String, Signature>,
    scopes: Scopes,
    return_type: Type,
    return_borrow_root: Option<Vec<(usize, String)>>,
    // Argument loans last through the call. Local references retain their
    // loans until the binding's block ends, including their reborrow ancestry.
    loans: Vec<Loan>,
    next_loan: usize,
    references: HashMap<Span, usize>,
    capture_bindings: HashMap<captures::Root, captures::Captures>,
    capture_expressions: HashMap<Span, captures::Captures>,
    capture_return_roots: HashSet<captures::Root>,
    types: &'program mut Types,
    expression_types: &'program mut HashMap<Span, Type>,
    method_calls: &'program mut HashMap<Span, Expression>,
    loops: Vec<LoopContext>,
    conversions: &'program mut HashMap<Span, Type>,
    constants: &'program mut HashMap<Span, i128>,
    unevaluated: bool,
    unsafe_depth: usize,
    current_struct: Option<usize>,
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
        let scope = self.scopes.len() - 1;
        self.check_scope_drops(scope, block.span)?;
        self.loans.retain(|loan| loan.scope != Some(scope));
        self.scopes.pop();
        self.capture_bindings.retain(|root, _| root.0 != scope);
        return Ok(returns);
    }

    fn statement(&mut self, statement: &Statement) -> Result<bool, Diagnostic> {
        let result = self.statement_inner(statement);
        self.release_temporaries();
        return result;
    }

    fn release_temporaries(&mut self) {
        self.loans.retain(|loan| loan.scope.is_some());
    }

    fn adopt_reference(&mut self, id: usize, scope: usize) {
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let loan = self
                .loans
                .iter_mut()
                .find(|loan| loan.id == id)
                .expect("reference has a live loan");
            if loan.scope.is_some() {
                continue;
            }
            loan.scope = Some(scope);
            pending.extend(&loan.parents);
        }
    }

    fn statement_inner(&mut self, statement: &Statement) -> Result<bool, Diagnostic> {
        match &statement.kind {
            StatementKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let result = self.block(block);
                self.unsafe_depth -= 1;
                return result;
            }
            StatementKind::StaticLocal {
                name,
                name_span,
                global,
                ty,
            } => {
                let index = self
                    .global_storage
                    .iter()
                    .position(|storage| storage.name == *global)
                    .unwrap();
                let storage = &self.global_storage[index];
                self.declare(name, *ty, State::Available, *name_span, Some(*name_span))?;
                let variable = self.scopes.last_mut().unwrap().get_mut(name).unwrap();
                variable.storage = Some(index);
                variable.mutable = !storage.readonly;
                variable.volatile = storage.volatile;
            }
            StatementKind::Local {
                name_span,
                name,
                ty,
                initializer,
                ..
            } => {
                if name == "this" {
                    return Err(Diagnostic::new(
                        statement.span,
                        "this is reserved for the instance receiver",
                    ));
                }
                self.declare(
                    name,
                    *ty,
                    State::Uninitialized,
                    *name_span,
                    Some(*name_span),
                )?;
                self.require_type(initializer, *ty)?;
                if ty.is_borrow() {
                    let id = self.references[&initializer.span];
                    let scope = self.scopes.len() - 1;
                    self.adopt_reference(id, scope);
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .get_mut(name)
                        .unwrap()
                        .reference = Some(id);
                }
                self.set_state(name, State::Available);
                let place = Place {
                    ty: *ty,
                    root: self.root(name),
                    fields: Vec::new(),
                    mutable: true,
                    volatile: false,
                    via: None,
                };
                self.bind_captures(&place, initializer)?;
            }
            StatementKind::Assign {
                name,
                name_span,
                value,
            } => {
                // Assignment may reinitialize an owner that has been moved.
                let target = self.lookup(name, statement.span)?;
                if !target.mutable {
                    return Err(Diagnostic::new(
                        *name_span,
                        "cannot assign to a const external variable",
                    ));
                }
                self.record_binding_symbol(*name_span, target);
                if target.ty.is_borrow() {
                    let target = Expression {
                        span: *name_span,
                        kind: ExpressionKind::Variable {
                            name: name.clone(),
                            name_span: *name_span,
                        },
                    };
                    let place = self.checked_place(&target)?;
                    if !place.mutable {
                        return Err(Diagnostic::new(
                            *name_span,
                            "cannot assign through a shared borrow",
                        ));
                    }
                    if self.types.carries_borrows(place.ty) {
                        return Err(Diagnostic::new(
                            *name_span,
                            "cannot replace a value containing references through a borrowed destination",
                        ));
                    }
                    self.require_type(value, place.ty)?;
                    self.access(&place.root, &place.fields, true, place.via, *name_span)?;
                    return Ok(false);
                }
                self.require_type(value, target.ty)?;
                let place = Place {
                    ty: target.ty,
                    root: self.root(name),
                    fields: Vec::new(),
                    mutable: true,
                    volatile: false,
                    via: None,
                };
                self.access(&place.root, &[], true, None, statement.span)?;
                self.check_capture_move(&place, statement.span)?;
                self.bind_captures(&place, value)?;
                self.set_state(name, State::Available);
            }
            StatementKind::CompoundAssign {
                target,
                value,
                operator,
            } => {
                let place = self.checked_place(target)?;
                if !place.mutable
                    || !matches!(
                        place.ty,
                        Type::Integer(_) | Type::Float(_) | Type::Pointer(_)
                    )
                {
                    return Err(Diagnostic::new(
                        target.span,
                        "compound assignment requires a mutable integer place",
                    ));
                }
                let previous_loans = self.loans.len();
                // Reads such as n += n are allowed. Prevent mutation or moves
                // that could invalidate a captured destination during the RHS.
                self.borrow(
                    place.root.clone(),
                    place.fields.clone(),
                    false,
                    place.via,
                    target.span,
                )?;
                let actual = self.expression(value)?;
                if matches!(place.ty, Type::Pointer(_)) {
                    if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                        return Err(Diagnostic::new(
                            statement.span,
                            "pointer compound assignments support += and -=",
                        ));
                    }
                    self.pointer_offset(place.ty, value, actual)?;
                } else if matches!(place.ty, Type::Float(_)) || matches!(actual, Type::Float(_)) {
                    if !matches!(
                        operator,
                        BinaryOperator::Add
                            | BinaryOperator::Subtract
                            | BinaryOperator::Multiply
                            | BinaryOperator::Divide
                    ) {
                        return Err(Diagnostic::new(
                            statement.span,
                            "this compound operator requires integers",
                        ));
                    }
                    let common = place.ty.common_numeric(actual).ok_or_else(|| {
                        Diagnostic::new(value.span, "compound assignment requires numeric values")
                    })?;
                    self.convert(value, actual, common)?;
                } else if operator.is_shift() {
                    self.integer(place.ty, target.span)?;
                    let right = self.integer(actual, value.span)?.promoted();
                    self.convert(value, actual, Type::Integer(right))?;
                } else {
                    let common = self
                        .integer(place.ty, target.span)?
                        .common(self.integer(actual, value.span)?);
                    self.convert(value, actual, Type::Integer(common))?;
                }
                self.loans.truncate(previous_loans);
                self.access(&place.root, &place.fields, true, place.via, target.span)?;
            }
            StatementKind::Store { target, value } => {
                let target_type = self.receiver_type(target)?;
                self.require_type(value, target_type)?;
                let place = self.checked_place(target)?;
                if !place.mutable || place.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        target.span,
                        "cannot assign through a shared borrow or rebind a borrow",
                    ));
                }
                if place.volatile && self.types.owns(place.ty) {
                    return Err(Diagnostic::new(
                        target.span,
                        "volatile storage operations require copyable values",
                    ));
                }
                self.access(&place.root, &place.fields, true, place.via, target.span)?;
                if self.types.carries_borrows(place.ty) {
                    if place.via.is_some()
                        || self.lookup(&place.root.1, target.span)?.ty.is_borrow()
                    {
                        return Err(Diagnostic::new(
                            target.span,
                            "cannot replace a value containing references through a borrowed destination",
                        ));
                    }
                    self.check_capture_move(&place, target.span)?;
                    self.bind_captures(&place, value)?;
                }
            }
            StatementKind::Expression(expression) => {
                let ty = self.expression(expression)?;
                if ty.is_borrow() {
                    return Err(Diagnostic::new(
                        expression.span,
                        "a borrow must initialize a local or be passed directly to a function",
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
                    (ty, Some(expression)) => {
                        self.require_type(expression, ty)?;
                        if !ty.is_borrow() && self.types.carries_borrows(ty) {
                            self.check_capture_return(expression)?;
                        }
                        if ty.is_borrow() && self.return_borrow_root.is_some() {
                            let id = self.references[&expression.span];
                            let loan = self
                                .loans
                                .iter()
                                .find(|loan| loan.id == id)
                                .expect("returned reference has a loan");
                            if !self.capture_return_roots.contains(&loan.root)
                                || loan
                                    .alternatives
                                    .iter()
                                    .any(|(root, _)| !self.capture_return_roots.contains(root))
                            {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "returned borrow must originate from the function's borrowed parameter",
                                ));
                            }
                        }
                    }
                    (_, None) => {
                        return Err(Diagnostic::new(
                            statement.span,
                            "a non-void function must return a value",
                        ));
                    }
                }
                self.check_return_drops(value.as_ref(), statement.span)?;
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
                self.condition(condition)?;
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
                if let Some(context) = self.loops.iter().rev().find(|context| {
                    matches!(statement.kind, StatementKind::Break) || !context.is_switch
                }) {
                    self.check_scope_drops(context.scope_count, statement.span)?;
                }
                let Some(context) = self.loops.iter_mut().rev().find(|context| {
                    matches!(statement.kind, StatementKind::Break) || !context.is_switch
                }) else {
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
                self.condition(condition)?;
                if !same_owner_states(&before, &self.scopes, self.types) {
                    return Err(Diagnostic::new(
                        condition.span,
                        "a loop condition cannot consume an outer owner",
                    ));
                }
                let after_condition = self.scopes.clone();
                self.loops.push(LoopContext {
                    is_switch: false,
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
            StatementKind::DoWhile { body, condition } => {
                let before = self.scopes.clone();
                self.loops.push(LoopContext {
                    is_switch: false,
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
                    // Unreachable conditions still need checked expression metadata.
                    self.scopes = before.clone();
                    self.condition(condition)?;
                    if !same_owner_states(&before, &self.scopes, self.types) {
                        return Err(Diagnostic::new(
                            condition.span,
                            "a loop condition cannot consume an outer owner",
                        ));
                    }
                }
                let mut exits = context.breaks;
                for (state, span) in context.continues {
                    self.scopes = state;
                    let before_condition = self.scopes.clone();
                    self.condition(condition)?;
                    if !same_owner_states(&before_condition, &self.scopes, self.types) {
                        return Err(Diagnostic::new(
                            condition.span,
                            "a loop condition cannot consume an outer owner",
                        ));
                    }
                    if !same_owner_states(&before, &self.scopes, self.types) {
                        return Err(Diagnostic::new(
                            span,
                            "a loop must restore moved outer owners before the next iteration",
                        ));
                    }
                    exits.push(self.scopes.clone());
                }
                let Some(mut after) = exits.pop() else {
                    self.scopes = before;
                    return Ok(true);
                };
                for state in exits {
                    after = merge(&after, &state);
                }
                self.scopes = after;
            }
            StatementKind::Empty => {}
            StatementKind::Switch { value, cases } => {
                let selector_type = self.probe_receiver_type(value)?;
                let tagged = match selector_type.referent() {
                    Type::Struct(id) if self.types.structs[id].tagged => Some(id),
                    _ => None,
                };
                let numeric = if tagged.is_none() {
                    let actual = self.expression(value)?;
                    let kind = self
                        .types
                        .integer_representation(actual)
                        .ok_or_else(|| {
                            Diagnostic::new(
                                value.span,
                                "switch requires an integer, enum, or tagged union",
                            )
                        })?
                        .promoted();
                    self.conversions.insert(value.span, Type::Integer(kind));
                    self.release_temporaries();
                    Some(kind)
                } else {
                    let place = self.checked_place(value)?;
                    self.access(&place.root, &place.fields, false, place.via, value.span)?;
                    None
                };
                let before = self.scopes.clone();
                let mut seen = HashSet::new();
                let mut default = false;
                self.loops.push(LoopContext {
                    is_switch: true,
                    scope_count: before.len(),
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                let mut fallthrough = None;
                for case in cases {
                    if let Some(label) = &case.label {
                        if let Some(id) = tagged {
                            let CaseLabel::Named { name, span } = label else {
                                return Err(Diagnostic::new(
                                    case.body.span,
                                    "a tagged-union case names a payload",
                                ));
                            };
                            if !self.types.structs[id]
                                .fields
                                .iter()
                                .any(|field| field.name == *name)
                                || !seen.insert(name.clone())
                            {
                                return Err(Diagnostic::new(
                                    *span,
                                    "unknown or duplicate tagged-union case",
                                ));
                            }
                            self.record_field_symbol(Type::Struct(id), name, *span);
                        } else {
                            let (expression, constant) = match label {
                                CaseLabel::Constant { expression, value } => {
                                    (expression.clone(), *value)
                                }
                                CaseLabel::Named { name, span } => {
                                    if self
                                        .scopes
                                        .iter()
                                        .rev()
                                        .any(|scope| scope.contains_key(name))
                                    {
                                        return Err(Diagnostic::new(
                                            *span,
                                            "a local value cannot be used as a case constant",
                                        ));
                                    }
                                    let expression = Expression {
                                        kind: ExpressionKind::Variable {
                                            name: name.clone(),
                                            name_span: *span,
                                        },
                                        span: *span,
                                    };
                                    let constant = self
                                        .types
                                        .enumerator(name)
                                        .map(|(_, variant)| variant.value)
                                        .ok_or_else(|| {
                                            Diagnostic::new(
                                                *span,
                                                "case requires an integer constant expression",
                                            )
                                        })?;
                                    (expression, constant)
                                }
                            };
                            let actual = self.expression(&expression)?;
                            if actual.integer().is_none() && !matches!(actual, Type::Enum(_)) {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "case requires an integer or enum constant",
                                ));
                            }
                            if let (Type::Enum(a), Type::Enum(b)) =
                                (selector_type.referent(), actual)
                                && a != b
                            {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "case belongs to a different enum type",
                                ));
                            }
                            if matches!(label, CaseLabel::Named { .. })
                                && !matches!(actual, Type::Enum(_))
                            {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "a local value cannot be used as a case constant",
                                ));
                            }
                            let constant = numeric.unwrap().wrap_constant(constant);
                            if !seen.insert(constant.to_string()) {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "duplicate case value after integer conversion",
                                ));
                            }
                        }
                    } else if default {
                        return Err(Diagnostic::new(case.body.span, "duplicate default case"));
                    } else {
                        default = true;
                    }
                    self.scopes = fallthrough
                        .take()
                        .map_or_else(|| before.clone(), |state| merge(&before, &state));
                    if !self.block(&case.body)? {
                        fallthrough = Some(self.scopes.clone());
                    }
                }
                let mut exits = self.loops.pop().unwrap().breaks;
                if let Some(state) = fallthrough {
                    exits.push(state);
                }
                if !default
                    && tagged.is_none_or(|id| seen.len() != self.types.structs[id].fields.len())
                {
                    exits.push(before.clone());
                }
                let Some(mut after) = exits.pop() else {
                    self.scopes = before;
                    return Ok(true);
                };
                for state in exits {
                    after = merge(&after, &state);
                }
                self.scopes = after;
            }
        }
        return Ok(false);
    }

    fn expression(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        let ty = self.expression_inner(expression)?;
        if let Type::Borrow { pointee, .. } = ty
            && !matches!(
                expression.kind,
                ExpressionKind::Unary {
                    operator: UnaryOperator::Address | UnaryOperator::Borrow { .. },
                    ..
                }
            )
        {
            if matches!(pointee, Pointee::Slice(_)) {
                return Err(Diagnostic::new(
                    expression.span,
                    "slices are accessed through references",
                ));
            }
            let place = self.reference_place(expression, ty)?;
            self.access(
                &place.root,
                &place.fields,
                false,
                place.via,
                expression.span,
            )?;
            if self.types.owns(pointee.ty()) && !self.unevaluated {
                return Err(Diagnostic::new(
                    expression.span,
                    "a borrowed owner cannot be copied or moved out",
                ));
            }
            self.reference_reads.insert(expression.span, ty);
            self.expression_types.insert(expression.span, pointee.ty());
            return Ok(pointee.ty());
        }
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn reference_expression(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        let ty = self.expression_inner(expression)?;
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn reference_place(&self, expression: &Expression, ty: Type) -> Result<Place, Diagnostic> {
        if let ExpressionKind::Variable { .. } = expression.kind {
            return self.place(expression);
        }
        let Type::Borrow {
            pointee,
            mutable,
            volatile,
        } = ty
        else {
            unreachable!()
        };
        let id = self.references.get(&expression.span).copied();
        let loan = id.and_then(|id| self.loans.iter().find(|loan| loan.id == id));
        assert!(
            self.unevaluated || loan.is_some(),
            "reference has a live loan"
        );
        return Ok(Place {
            ty: pointee.ty(),
            mutable,
            volatile,
            root: loan
                .map(|loan| loan.root.clone())
                .unwrap_or((self.scopes.len(), format!("@{}", expression.span.start))),
            fields: loan.map(|loan| loan.fields.clone()).unwrap_or_default(),
            via: id,
        });
    }

    fn expression_inner(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        if let Some((value, explicit_kind)) = integer_literal(expression) {
            let kind = if let Some(kind) = explicit_kind {
                kind
            } else if IntType::I32.fits(value) {
                IntType::I32
            } else if IntType::I64.fits(value) {
                IntType::I64
            } else if IntType::U64.fits(value) {
                IntType::U64
            } else {
                return Err(Diagnostic::new(
                    expression.span,
                    "integer literal exceeds the 64-bit range",
                ));
            };
            self.constants.insert(expression.span, value);
            return Ok(Type::Integer(kind));
        }
        match &expression.kind {
            ExpressionKind::Update { target, .. } => {
                let place = self.checked_place(target)?;
                if !place.mutable
                    || !matches!(
                        place.ty,
                        Type::Integer(_) | Type::Float(_) | Type::Pointer(_)
                    )
                {
                    return Err(Diagnostic::new(
                        target.span,
                        "increment and decrement require a mutable integer or raw pointer place",
                    ));
                }
                if let Type::Pointer(id) = place.ty {
                    self.require_unsafe(expression.span, "raw pointer increment or decrement")?;
                    if !self.types.is_sized(self.types.pointers[id].pointee) {
                        return Err(Diagnostic::new(
                            target.span,
                            "pointer increment and decrement require a complete object type",
                        ));
                    }
                }
                self.access(&place.root, &place.fields, true, place.via, target.span)?;
                return Ok(place.ty);
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                let ty = self.conditional(condition, then_value, else_value, None)?;
                let mut captures = self
                    .capture_expressions
                    .get(&then_value.span)
                    .cloned()
                    .unwrap_or_default();
                captures.extend(
                    self.capture_expressions
                        .get(&else_value.span)
                        .cloned()
                        .unwrap_or_default(),
                );
                self.remember_captures(expression.span, captures);
                return Ok(ty);
            }
            ExpressionKind::Storage { pointer, value } => {
                self.require_unsafe(expression.span, "initializing or destroying raw storage")?;
                let ty = self.expression(pointer)?;
                let Type::Pointer(id) = ty else {
                    return Err(Diagnostic::new(
                        pointer.span,
                        "storage operations require a mutable raw pointer",
                    ));
                };
                let destination = self.types.pointers[id];
                if !destination.mutable || !self.types.is_sized(destination.pointee) {
                    return Err(Diagnostic::new(
                        pointer.span,
                        "storage operations require a mutable pointer to a complete object type",
                    ));
                }
                if destination.volatile && self.types.owns(destination.pointee) {
                    return Err(Diagnostic::new(
                        pointer.span,
                        "volatile storage operations require copyable values",
                    ));
                }
                if let Some(value) = value {
                    self.require_type(value, destination.pointee)?;
                }
                return Ok(Type::Void);
            }
            ExpressionKind::IndirectCall { callee, arguments } => {
                let previous_loans = self.loans.len();
                let ty = self.expression(callee)?;
                let signature = self.types.callback(ty).cloned().ok_or_else(|| {
                    Diagnostic::new(callee.span, "indirect calls require a function pointer")
                })?;
                self.record_call(
                    expression.span,
                    callee.span,
                    CallTarget::Callback(ty),
                    arguments,
                );
                if arguments.len() < signature.parameters.len()
                    || (!signature.variadic && arguments.len() != signature.parameters.len())
                {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "callback expects {}{} argument(s), got {}",
                            if signature.variadic { "at least " } else { "" },
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }
                self.require_unsafe(expression.span, "calling a function pointer")?;
                for (argument, expected) in arguments.iter().zip(&signature.parameters) {
                    self.require_type(argument, *expected)?;
                }
                self.variadic_arguments(&arguments[signature.parameters.len()..])?;
                self.loans.truncate(previous_loans);
                return Ok(signature.result);
            }
            ExpressionKind::StaticReference {
                id,
                name,
                name_span,
            } => {
                let ty = self.static_reference(*id, name, expression.span)?;
                self.record_function_symbol(&static_method_name(*id, name), *name_span);
                return Ok(ty);
            }
            ExpressionKind::EnumValue {
                id,
                name,
                name_span,
            } => {
                let variant = self.types.enums[*id]
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            expression.span,
                            format!(
                                "enum '{}' has no value '{name}'",
                                self.types.enum_names[*id]
                            ),
                        )
                    })?;
                self.constants.insert(expression.span, variant.value);
                self.symbol_uses.insert(*name_span, variant.span);
                return Ok(Type::Enum(*id));
            }
            ExpressionKind::StringLiteral { id, .. } => return Ok(Type::Array(*id)),
            ExpressionKind::Cast { ty, operand } => {
                if *ty == Type::Void {
                    let previous_loans = self.loans.len();
                    self.expression(operand)?;
                    self.loans.truncate(previous_loans);
                    return Ok(Type::Void);
                }
                if matches!(ty, Type::Pointer(_)) {
                    self.require_unsafe(expression.span, "raw pointer casts")?;
                    if let ExpressionKind::Unary {
                        operator: UnaryOperator::Address,
                        operand: place,
                    } = &operand.kind
                        && !self.function_address(place)
                    {
                        let actual = self.raw_address(operand, place)?;
                        self.expression_types.insert(operand.span, actual);
                        self.conversions.insert(operand.span, *ty);
                        return Ok(*ty);
                    }
                    let actual = self.expression(operand)?;
                    if !matches!(actual, Type::Pointer(_) | Type::Borrow { .. })
                        && !matches!(actual, Type::Integer(kind) if kind.bits() as u32 == usize::BITS)
                        && constant_integer(operand) != Some(0)
                    {
                        return Err(Diagnostic::new(
                            operand.span,
                            "raw pointer casts require a pointer, borrow, pointer-sized integer, or zero",
                        ));
                    }
                    self.conversions.insert(operand.span, *ty);
                    return Ok(*ty);
                }
                if self.types.integer_representation(*ty).is_none() && !matches!(ty, Type::Float(_))
                {
                    return Err(Diagnostic::new(
                        expression.span,
                        "casts require a numeric type, enum, or void",
                    ));
                }
                if constant_integer(operand).is_some() && !matches!(ty, Type::Float(_)) {
                    let representation =
                        Type::Integer(self.types.integer_representation(*ty).unwrap());
                    if *ty == Type::Bool {
                        self.require_type(operand, *ty)?;
                    } else {
                        self.require_type(operand, representation)?;
                        self.conversions.insert(operand.span, *ty);
                    }
                } else {
                    let actual = self.expression(operand)?;
                    if matches!(actual, Type::Pointer(_)) {
                        self.require_unsafe(expression.span, "raw pointer casts")?;
                        if *ty != Type::Bool
                            && !matches!(ty, Type::Integer(kind) if kind.bits() as u32 == usize::BITS)
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "a raw pointer can be cast to bool or a pointer-sized integer",
                            ));
                        }
                        self.conversions.insert(operand.span, *ty);
                        return Ok(*ty);
                    }
                    if self.types.integer_representation(actual).is_none()
                        && !matches!(actual, Type::Float(_))
                    {
                        return Err(Diagnostic::new(
                            operand.span,
                            "casts require a numeric or enum operand",
                        ));
                    }
                    self.conversions.insert(operand.span, *ty);
                }
                return Ok(*ty);
            }
            ExpressionKind::SizeofType(ty) | ExpressionKind::Alignof(ty) => {
                if *ty == Type::Void {
                    return Err(Diagnostic::new(
                        expression.span,
                        "void has no object size or alignment",
                    ));
                }
                if !self.types.is_sized(*ty) {
                    return Err(Diagnostic::new(
                        expression.span,
                        "size and alignment queries require a complete object type",
                    ));
                }
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::SizeofValue(operand) => {
                let previous = self.unevaluated;
                self.unevaluated = true;
                let result = self.expression(operand);
                self.unevaluated = previous;
                if result? == Type::Void {
                    return Err(Diagnostic::new(expression.span, "void has no object size"));
                }
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::Boolean(_) => return Ok(Type::Bool),
            ExpressionKind::Floating { kind, .. } => return Ok(Type::Float(*kind)),
            ExpressionKind::StaticCall {
                name_span,
                id,
                name,
                arguments,
            } => {
                let key = static_method_name(*id, name);
                self.record_call(
                    expression.span,
                    *name_span,
                    CallTarget::Function(key.clone()),
                    arguments,
                );
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
                        name_span: *name_span,
                        name: key,
                        arguments: arguments.clone(),
                    },
                };
                let ty = self.reference_expression(&call)?;
                self.method_calls.insert(expression.span, call);
                return Ok(ty);
            }
            ExpressionKind::MethodCall {
                name_span,
                receiver,
                name,
                arguments,
            } => {
                let ty = self.receiver_type(receiver)?;
                let Type::Struct(id) = ty else {
                    return Err(Diagnostic::new(
                        receiver.span,
                        "a method receiver must be a struct value or reference; use '->' through a raw pointer",
                    ));
                };
                if self.types.structs[id]
                    .fields
                    .iter()
                    .any(|field| field.name == *name)
                {
                    let field_type = self.field_type(ty, name, expression.span)?;
                    if self.types.callback(field_type).is_some() {
                        let call = Expression {
                            span: expression.span,
                            kind: ExpressionKind::IndirectCall {
                                callee: Box::new(Expression {
                                    span: receiver.span.through(*name_span),
                                    kind: ExpressionKind::Field {
                                        name_span: *name_span,
                                        base: receiver.clone(),
                                        name: name.clone(),
                                    },
                                }),
                                arguments: arguments.clone(),
                            },
                        };
                        let result = self.reference_expression(&call)?;
                        self.method_calls.insert(expression.span, call);
                        return Ok(result);
                    }
                }
                let key = method_name(id, name);
                self.record_call(
                    expression.span,
                    *name_span,
                    CallTarget::Function(key.clone()),
                    arguments,
                );
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
                        name_span: *name_span,
                        name: key,
                        arguments: lowered_arguments,
                    },
                };
                let result = self.reference_expression(&call)?;
                self.method_calls.insert(expression.span, call);
                return Ok(result);
            }
            ExpressionKind::StructLiteral { .. } => {
                return self.struct_literal(expression, None);
            }
            ExpressionKind::ArrayLiteral { .. } => {
                return self.array_literal(expression, None);
            }
            ExpressionKind::Field { .. } | ExpressionKind::Index { .. } => {
                if self.probe_receiver_type(expression)?.is_borrow() {
                    let raw = self.check_place(expression)?;
                    return self.stored_reference(expression, &raw);
                }
                let place = self.checked_place(expression)?;
                self.access(
                    &place.root,
                    &place.fields,
                    false,
                    place.via,
                    expression.span,
                )?;
                self.capture_value(expression.span, &place);
                if self.types.owns(place.ty) && !self.unevaluated {
                    return Err(Diagnostic::new(
                        expression.span,
                        "an owning field cannot be copied; moving individual fields is not supported yet",
                    ));
                }
                return Ok(place.ty);
            }
            ExpressionKind::Integer(_) | ExpressionKind::TypedInteger { .. } => {
                unreachable!("literal handled above")
            }
            ExpressionKind::Variable { name, name_span } => {
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && let Some((id, variant)) = self.types.enumerator(name)
                {
                    self.constants.insert(expression.span, variant.value);
                    self.symbol_uses.insert(*name_span, variant.span);
                    return Ok(Type::Enum(id));
                }
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    self.record_function_symbol(name, *name_span);
                    return self.function_reference(name, expression.span);
                }
                let variable = self.available(name, expression.span)?;
                self.record_binding_symbol(*name_span, variable);
                if self.types.owns(variable.ty) && !self.unevaluated {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!("owner '{name}' cannot be copied; use move({name}) to transfer it"),
                    ));
                }
                match variable.ty {
                    Type::Borrow { .. } => {
                        let place = self.place(expression)?;
                        self.access(
                            &place.root,
                            &place.fields,
                            false,
                            place.via,
                            expression.span,
                        )?;
                    }
                    _ => self.access(&self.root(name), &[], false, None, expression.span)?,
                }
                let place = self.place(expression)?;
                self.capture_value(expression.span, &place);
                return Ok(variable.ty);
            }
            ExpressionKind::Call {
                name,
                name_span,
                arguments,
            } => {
                if self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                {
                    let call = Expression {
                        span: expression.span,
                        kind: ExpressionKind::IndirectCall {
                            callee: Box::new(Expression {
                                span: *name_span,
                                kind: ExpressionKind::Variable {
                                    name: name.clone(),
                                    name_span: *name_span,
                                },
                            }),
                            arguments: arguments.clone(),
                        },
                    };
                    let variable = self.lookup(name, expression.span)?;
                    if self.types.callback(variable.ty.referent()).is_some() {
                        let result = self.reference_expression(&call)?;
                        self.method_calls.insert(expression.span, call);
                        return Ok(result);
                    }
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
                self.record_function_symbol(name, *name_span);
                self.record_call(
                    expression.span,
                    *name_span,
                    CallTarget::Function(name.clone()),
                    arguments,
                );
                if arguments.len() < signature.parameters.len()
                    || (!signature.variadic && arguments.len() != signature.parameters.len())
                {
                    return Err(Diagnostic::new(
                        expression.span,
                        format!(
                            "function '{name}' expects {}{} argument(s), got {}",
                            if signature.variadic { "at least " } else { "" },
                            signature.parameters.len(),
                            arguments.len()
                        ),
                    ));
                }

                if signature.external {
                    self.require_unsafe(expression.span, "calling an external C function")?;
                } else if signature.requires_unsafe {
                    self.require_unsafe(expression.span, "calling an unsafe function")?;
                }

                if name == "slice" {
                    self.require_unsafe(expression.span, "constructing a slice from raw storage")?;
                    let mut place = self.checked_place(&arguments[0])?;
                    let Type::Pointer(id) = place.ty else {
                        return Err(Diagnostic::new(
                            arguments[0].span,
                            "slice requires a raw pointer and length",
                        ));
                    };
                    let pointer = self.types.pointers[id];
                    if pointer.volatile || !self.types.is_sized(pointer.pointee) {
                        return Err(Diagnostic::new(
                            arguments[0].span,
                            "slice requires a pointer to a complete non-volatile object type",
                        ));
                    }
                    self.access(
                        &place.root,
                        &place.fields,
                        false,
                        place.via,
                        arguments[0].span,
                    )?;
                    self.expression_types.insert(arguments[0].span, place.ty);
                    if let ExpressionKind::Variable { name, .. } = &arguments[0].kind {
                        let original = self.lookup(name, arguments[0].span)?.ty;
                        if original.is_borrow() {
                            self.reference_reads.insert(arguments[0].span, original);
                        }
                    }
                    let ty = self.types.slice(
                        pointer.pointee,
                        pointer.mutable && place.mutable,
                        expression.span,
                    );
                    place.ty = ty.referent();
                    place.volatile = false;
                    self.borrow_place(
                        expression,
                        arguments[0].span,
                        place,
                        matches!(ty, Type::Borrow { mutable: true, .. }),
                    )?;
                    self.require_type(
                        &arguments[1],
                        Type::Integer(IntType::named("size_t").unwrap()),
                    )?;
                    return Ok(ty);
                }
                if name == "move" {
                    if let ExpressionKind::Variable {
                        name: owner,
                        name_span,
                    } = &arguments[0].kind
                    {
                        let variable = self.available(owner, arguments[0].span)?;
                        self.record_binding_symbol(*name_span, variable);
                        if !self.types.owns(variable.ty) {
                            if variable.ty.is_borrow() {
                                return Err(Diagnostic::new(
                                    arguments[0].span,
                                    "borrow bindings use reborrows rather than move",
                                ));
                            }
                            let ty = self.expression(&arguments[0])?;
                            let captures = self
                                .capture_expressions
                                .get(&arguments[0].span)
                                .cloned()
                                .unwrap_or_default();
                            self.remember_captures(expression.span, captures);
                            return Ok(ty);
                        }
                        self.access(&self.root(owner), &[], true, None, arguments[0].span)?;
                        let place = self.place(&arguments[0])?;
                        self.check_capture_move(&place, arguments[0].span)?;
                        self.capture_value(expression.span, &place);
                        self.set_state(owner, State::Moved);
                        return Ok(variable.ty);
                    }
                    if let ExpressionKind::Field { base, .. } = &arguments[0].kind
                        && let ExpressionKind::Variable { name, .. } = &base.kind
                    {
                        let owner = self.available(name, base.span)?;
                        if let Type::Struct(id) = owner.ty
                            && self.types.structs[id].tagged
                        {
                            if self.types.structs[id].has_destructor {
                                return Err(Diagnostic::new(
                                    expression.span,
                                    "payload extraction requires a tagged union without a custom destructor",
                                ));
                            }
                            let place = self.checked_place(&arguments[0])?;
                            self.access(&place.root, &[], true, place.via, arguments[0].span)?;
                            self.expression_types.insert(arguments[0].span, place.ty);
                            self.check_capture_move(&place, arguments[0].span)?;
                            self.capture_value(expression.span, &place);
                            self.set_state(name, State::Moved);
                            return Ok(place.ty);
                        }
                    }
                    let pointer = match &arguments[0].kind {
                        ExpressionKind::Unary {
                            operator: UnaryOperator::Dereference,
                            operand,
                        } => Some(operand.as_ref()),
                        ExpressionKind::Index { base, .. } => Some(base.as_ref()),
                        _ => None,
                    };
                    if let Some(pointer) = pointer
                        && matches!(self.receiver_type(pointer)?, Type::Pointer(_))
                    {
                        self.require_unsafe(expression.span, "moving a value out of raw storage")?;
                        let place = self.checked_place(&arguments[0])?;
                        if !self.types.is_sized(place.ty) {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "raw storage moves require a complete object type",
                            ));
                        }
                        let owning = self.types.owns(place.ty);
                        if owning && place.volatile {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "volatile storage operations require copyable values",
                            ));
                        }
                        if owning && !place.mutable {
                            return Err(Diagnostic::new(
                                arguments[0].span,
                                "moving an owner requires mutable raw storage",
                            ));
                        }
                        self.access(
                            &place.root,
                            &place.fields,
                            owning,
                            place.via,
                            arguments[0].span,
                        )?;
                        self.expression_types.insert(arguments[0].span, place.ty);
                        return Ok(place.ty);
                    }
                    return Err(Diagnostic::new(
                        arguments[0].span,
                        "move requires the name of a whole variable or a whole value in raw storage; moving individual fields is not supported yet",
                    ));
                }

                if name == "print" {
                    let previous_loans = self.loans.len();
                    let borrowed_text =
                        self.probe_receiver_type(&arguments[0])
                            .ok()
                            .is_some_and(|ty| {
                                ty.is_borrow()
                                    && (self.types.byte_array_length(ty).is_some()
                                        || self.types.is_byte_slice(ty))
                            });
                    let actual = if borrowed_text {
                        // Preserve bounded, volatile reads instead of copying
                        // the entire array before printing it.
                        self.reference_expression(&arguments[0])?
                    } else {
                        self.expression(&arguments[0])?
                    };
                    if self.types.byte_array_length(actual).is_none()
                        && !self.types.is_byte_slice(actual)
                        && !matches!(actual, Type::Float(_))
                    {
                        self.integer(actual, arguments[0].span)?;
                    }
                    self.loans.truncate(previous_loans);
                    return Ok(Type::Void);
                }

                // This ordering is also the language's evaluation order.
                let previous_loans = self.loans.len();
                for (argument, expected) in arguments.iter().zip(&signature.parameters) {
                    self.require_type(argument, *expected)?;
                }
                self.variadic_arguments(&arguments[signature.parameters.len()..])?;
                if let Some(sources) = signature
                    .return_borrow
                    .as_ref()
                    .filter(|_| !self.unevaluated)
                {
                    self.returned_reference(
                        expression.span,
                        arguments,
                        sources,
                        previous_loans,
                        signature.return_type,
                    )?;
                } else {
                    self.loans.truncate(previous_loans);
                }
                return Ok(signature.return_type);
            }
            ExpressionKind::Unary { operator, operand } => {
                if *operator == UnaryOperator::Address {
                    if let ExpressionKind::StaticReference {
                        id,
                        name,
                        name_span,
                    } = &operand.kind
                    {
                        self.record_function_symbol(&static_method_name(*id, name), *name_span);
                        return self.static_reference(*id, name, expression.span);
                    }
                    if let ExpressionKind::Variable { name, name_span } = &operand.kind
                        && !self
                            .scopes
                            .iter()
                            .rev()
                            .any(|scope| scope.contains_key(name))
                        && self.signatures.contains_key(name)
                    {
                        self.record_function_symbol(name, *name_span);
                        return self.function_reference(name, expression.span);
                    }
                    let place = self.checked_place(operand)?;
                    let mutable = place.mutable;
                    return self.borrow_place(expression, operand.span, place, mutable);
                }
                if let UnaryOperator::Borrow { mutable } = operator {
                    return self.borrow_address(expression, operand, *mutable);
                }
                if *operator == UnaryOperator::Dereference {
                    let place = self.checked_place(expression)?;
                    if matches!(place.ty, Type::Function(_)) {
                        self.access(
                            &place.root,
                            &place.fields,
                            false,
                            place.via,
                            expression.span,
                        )?;
                        return Ok(self.types.pointer_to(place.ty, true));
                    }
                    if !self.types.is_sized(place.ty) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "an incomplete object cannot be read as a value",
                        ));
                    }
                    self.access(
                        &place.root,
                        &place.fields,
                        false,
                        place.via,
                        expression.span,
                    )?;
                    if self.types.owns(place.ty) && !self.unevaluated {
                        return Err(Diagnostic::new(
                            expression.span,
                            "a borrowed owner cannot be copied or moved out",
                        ));
                    }
                    return Ok(place.ty);
                }
                let actual = self.expression(operand)?;
                if *operator == UnaryOperator::Not && matches!(actual, Type::Pointer(_)) {
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(actual, Type::Float(_)) {
                    if *operator == UnaryOperator::Not {
                        return Ok(Type::Integer(IntType::I32));
                    }
                    if matches!(operator, UnaryOperator::Plus | UnaryOperator::Negate) {
                        return Ok(actual);
                    }
                    return Err(Diagnostic::new(
                        expression.span,
                        "this operator requires an integer",
                    ));
                }
                let kind = self.integer(actual, operand.span)?.promoted();
                if *operator == UnaryOperator::Not {
                    return Ok(Type::Integer(IntType::I32));
                }
                self.convert(operand, actual, Type::Integer(kind))?;
                return Ok(Type::Integer(kind));
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let before_left_loans = self.loans.len();
                let left_type = self.expression(left)?;
                if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
                    self.scalar_condition(left_type, left.span)?;
                    self.loans.truncate(before_left_loans);
                    let before_right = self.scopes.clone();
                    let right_type = self.expression(right)?;
                    self.scalar_condition(right_type, right.span)?;
                    self.scopes = merge(&before_right, &self.scopes);
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(left_type, Type::Pointer(_)) {
                    let right_type = self.expression(right)?;
                    if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                        && right_type.integer().is_some()
                    {
                        self.pointer_offset(left_type, right, right_type)?;
                        return Ok(left_type);
                    }
                    if *operator == BinaryOperator::Subtract
                        && matches!(right_type, Type::Pointer(_))
                    {
                        self.require_unsafe(expression.span, "raw pointer subtraction")?;
                        let Type::Pointer(left_id) = left_type else {
                            unreachable!()
                        };
                        let Type::Pointer(right_id) = right_type else {
                            unreachable!()
                        };
                        if !self.types.is_sized(self.types.pointers[left_id].pointee)
                            || self.types.pointers[left_id].pointee
                                != self.types.pointers[right_id].pointee
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "pointer subtraction requires matching object types",
                            ));
                        }
                        return Ok(Type::Integer(IntType::named("ptrdiff_t").unwrap()));
                    }
                    if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "raw pointers currently support equality comparisons",
                        ));
                    }
                    let common = if let (Type::Pointer(left_id), Type::Pointer(right_id)) =
                        (left_type, right_type)
                    {
                        let a = self.types.pointers[left_id];
                        let b = self.types.pointers[right_id];
                        if a.pointee != b.pointee
                            && a.pointee != Type::Void
                            && b.pointee != Type::Void
                        {
                            return Err(Diagnostic::new(
                                expression.span,
                                "pointer comparisons require compatible pointee types",
                            ));
                        }
                        self.types.qualified_pointer_to(
                            if a.pointee == Type::Void || b.pointee == Type::Void {
                                Type::Void
                            } else {
                                a.pointee
                            },
                            a.mutable && b.mutable,
                            a.volatile || b.volatile,
                        )
                    } else {
                        left_type
                    };
                    self.convert(left, left_type, common)?;
                    self.convert(right, right_type, common)?;
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(left_type, Type::Enum(_)) {
                    let right_type = self.expression(right)?;
                    if left_type != right_type
                        || !matches!(
                            operator,
                            BinaryOperator::Equal
                                | BinaryOperator::NotEqual
                                | BinaryOperator::Less
                                | BinaryOperator::LessEqual
                                | BinaryOperator::Greater
                                | BinaryOperator::GreaterEqual
                        )
                    {
                        return Err(Diagnostic::new(
                            expression.span,
                            "enum comparisons require values of the same enum type; arithmetic requires an explicit integer cast",
                        ));
                    }
                    return Ok(Type::Integer(IntType::I32));
                }
                let right_type = self.expression(right)?;
                if matches!(left_type, Type::Float(_)) || matches!(right_type, Type::Float(_)) {
                    let common = left_type.common_numeric(right_type).ok_or_else(|| {
                        Diagnostic::new(
                            expression.span,
                            "floating arithmetic requires numeric operands",
                        )
                    })?;
                    if !matches!(
                        operator,
                        BinaryOperator::Add
                            | BinaryOperator::Subtract
                            | BinaryOperator::Multiply
                            | BinaryOperator::Divide
                            | BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual
                    ) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "this operator requires integers",
                        ));
                    }
                    self.convert(left, left_type, common)?;
                    self.convert(right, right_type, common)?;
                    return Ok(
                        if matches!(
                            operator,
                            BinaryOperator::Add
                                | BinaryOperator::Subtract
                                | BinaryOperator::Multiply
                                | BinaryOperator::Divide
                        ) {
                            common
                        } else {
                            Type::Integer(IntType::I32)
                        },
                    );
                }
                let left_kind = self.integer(left_type, left.span)?;
                if matches!(right_type, Type::Pointer(_)) {
                    if *operator == BinaryOperator::Add {
                        self.pointer_offset(right_type, left, left_type)?;
                        return Ok(right_type);
                    }
                    if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                        return Err(Diagnostic::new(
                            expression.span,
                            "raw pointers currently support equality comparisons",
                        ));
                    }
                    self.convert(left, left_type, right_type)?;
                    return Ok(Type::Integer(IntType::I32));
                }
                let right_kind = self.integer(right_type, right.span)?;
                if operator.is_shift() {
                    let promoted = left_kind.promoted();
                    self.convert(left, left_type, Type::Integer(promoted))?;
                    self.convert(right, right_type, Type::Integer(right_kind.promoted()))?;
                    return Ok(Type::Integer(promoted));
                }
                let common = left_kind.common(right_kind);
                self.convert(left, left_type, Type::Integer(common))?;
                self.convert(right, right_type, Type::Integer(common))?;
                return Ok(Type::Integer(
                    if matches!(
                        operator,
                        BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual
                    ) {
                        IntType::I32
                    } else {
                        common
                    },
                ));
            }
        }
    }

    fn require_type(&mut self, expression: &Expression, expected: Type) -> Result<(), Diagnostic> {
        if matches!(expected, Type::Pointer(_))
            && let ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } = &expression.kind
        {
            self.conditional(condition, then_value, else_value, Some(expected))?;
            self.expression_types.insert(expression.span, expected);
            return Ok(());
        }
        if let ExpressionKind::Unary {
            operator: UnaryOperator::Address,
            operand,
        } = &expression.kind
        {
            if matches!(expected, Type::Pointer(_)) && !self.function_address(operand) {
                let actual = self.raw_address(expression, operand)?;
                self.expression_types.insert(expression.span, actual);
                return self.convert(expression, actual, expected);
            }
            if let Type::Borrow { mutable, .. } = expected {
                let actual = self.borrow_address(expression, operand, mutable)?;
                self.expression_types.insert(expression.span, actual);
                return self.convert(expression, actual, expected);
            }
        }
        if let Type::Borrow { mutable, .. } = expected {
            if let ExpressionKind::Variable { name, .. } = &expression.kind
                && self.lookup(name, expression.span)?.ty.is_borrow()
            {
                let actual = self.borrow_address(expression, expression, mutable)?;
                self.expression_types.insert(expression.span, actual);
                return self.convert(expression, actual, expected);
            }
            if matches!(expression.kind, ExpressionKind::Field { .. })
                && self.probe_receiver_type(expression)?.is_borrow()
            {
                let actual = self.borrow_address(expression, expression, mutable)?;
                self.expression_types.insert(expression.span, actual);
                return self.convert(expression, actual, expected);
            }
            let actual = self.reference_expression(expression)?;
            return self.convert(expression, actual, expected);
        }
        if let ExpressionKind::StringLiteral { bytes, .. } = &expression.kind
            && let Type::Array(_) = expected
        {
            let length = self.types.byte_array_length(expected).ok_or_else(|| {
                Diagnostic::new(
                    expression.span,
                    "string literals require a char or uint8_t array",
                )
            })?;
            if bytes.len() >= length {
                return Err(Diagnostic::new(
                    expression.span,
                    "string literal and its terminating zero do not fit the array",
                ));
            }
            self.expression_types.insert(expression.span, expected);
            return Ok(());
        }
        if let Type::Integer(kind) = expected
            && let Some((value, explicit_kind)) = integer_literal(expression)
        {
            if !kind.fits(value) {
                return Err(Diagnostic::new(
                    expression.span,
                    format!(
                        "integer literal must fit {}-bit {}",
                        kind.bits(),
                        if kind.signed() {
                            "signed integer"
                        } else {
                            "unsigned integer"
                        }
                    ),
                ));
            }
            if explicit_kind.is_some() {
                let actual = self.expression(expression)?;
                return self.convert(expression, actual, expected);
            }
            self.constants.insert(expression.span, value);
            self.expression_types.insert(expression.span, expected);
            return Ok(());
        }
        let actual = if matches!(expression.kind, ExpressionKind::ArrayLiteral { .. }) {
            self.array_literal(expression, Some(expected))?
        } else if matches!(
            expression.kind,
            ExpressionKind::StructLiteral { id: None, .. }
        ) {
            self.struct_literal(expression, Some(expected))?
        } else {
            self.expression(expression)?
        };
        return self.convert(expression, actual, expected);
    }

    fn function_address(&self, operand: &Expression) -> bool {
        return match &operand.kind {
            ExpressionKind::StaticReference { .. } => true,
            ExpressionKind::Variable { name, .. } => {
                !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
            }
            _ => false,
        };
    }

    fn raw_address(
        &mut self,
        expression: &Expression,
        operand: &Expression,
    ) -> Result<Type, Diagnostic> {
        let storage = if let ExpressionKind::Variable { name, name_span } = &operand.kind {
            self.scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(name).copied())
                .filter(|variable| variable.storage.is_some() && !variable.external)
                .map(|variable| (name.clone(), *name_span, variable))
        } else {
            None
        };
        let place = if let Some((name, span, variable)) = storage {
            self.record_binding_symbol(span, variable);
            self.receiver_types.insert(operand.span, variable.ty);
            Place {
                ty: variable.ty,
                root: self.root(&name),
                fields: Vec::new(),
                mutable: variable.mutable,
                volatile: variable.volatile,
                via: None,
            }
        } else {
            self.checked_place(operand)?
        };
        self.access(
            &place.root,
            &place.fields,
            false,
            place.via,
            expression.span,
        )?;
        return Ok(self
            .types
            .qualified_pointer_to(place.ty, place.mutable, place.volatile));
    }

    fn borrow_address(
        &mut self,
        expression: &Expression,
        operand: &Expression,
        mutable: bool,
    ) -> Result<Type, Diagnostic> {
        let place = self.checked_place(operand)?;
        return self.borrow_place(expression, operand.span, place, mutable);
    }

    fn borrow_place(
        &mut self,
        expression: &Expression,
        operand_span: Span,
        place: Place,
        mutable: bool,
    ) -> Result<Type, Diagnostic> {
        if mutable && !place.mutable {
            return Err(Diagnostic::new(
                operand_span,
                "cannot create a mutable borrow through a shared borrow",
            ));
        }
        let mut ty = place
            .ty
            .borrowed_qualified(mutable, place.volatile)
            .ok_or_else(|| Diagnostic::new(operand_span, "invalid borrow type"))?;
        if let Type::Array(id) = place.ty
            && self.types.arrays[id].length == 0
        {
            ty = Type::Borrow {
                pointee: Pointee::Slice(id),
                mutable,
                volatile: place.volatile,
            };
        }
        let capture_type = place.ty;
        let captures = self.captures_at(&place);
        let id = self.borrow(
            place.root,
            place.fields,
            mutable,
            place.via,
            expression.span,
        )?;
        let id = if !captures.is_empty() && !self.unevaluated {
            let mut parents = vec![id];
            parents.extend(captures.iter().map(|capture| capture.id()));
            for capture in captures {
                let loan = self
                    .loans
                    .iter()
                    .find(|loan| loan.id == capture.id())
                    .unwrap();
                for (root, fields) in std::iter::once((&loan.root, &loan.fields)).chain(
                    loan.alternatives
                        .iter()
                        .map(|(root, fields)| (root, fields)),
                ) {
                    self.access_through(
                        root,
                        fields,
                        mutable && loan.mutable,
                        &parents,
                        expression.span,
                    )?;
                }
            }
            self.capture_loan(&parents, mutable)
        } else {
            id
        };
        self.references.insert(expression.span, id);
        if self.types.contains_borrows(capture_type) {
            let mut fields = Vec::new();
            captures::reference_fields(
                self.types,
                capture_type,
                Vec::new(),
                &mut fields,
                expression.span,
            )?;
            self.remember_captures(
                expression.span,
                fields
                    .into_iter()
                    .map(|(path, _)| captures::Capture::at(path, id))
                    .collect(),
            );
        }
        return Ok(ty);
    }

    fn conditional(
        &mut self,
        condition: &Expression,
        then_value: &Expression,
        else_value: &Expression,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        let condition_loans = self.loans.len();
        let condition_type = self.expression(condition)?;
        self.scalar_condition(condition_type, condition.span)?;
        self.loans.truncate(condition_loans);
        let before = self.scopes.clone();
        let loan_count = self.loans.len();
        let then_type = self.conditional_branch(then_value, expected)?;
        let then_scopes = self.scopes.clone();
        let then_loans = self.loans.split_off(loan_count);
        self.scopes = before;
        let else_type = self.conditional_branch(else_value, expected)?;
        self.scopes = merge(&then_scopes, &self.scopes);
        self.loans.extend(then_loans);
        if let Some(ty) = expected {
            return Ok(ty);
        }
        let ty = self.conditional_type(then_value, then_type, else_value, else_type)?;
        self.convert(then_value, then_type, ty)?;
        self.convert(else_value, else_type, ty)?;
        return Ok(ty);
    }

    fn conditional_branch(
        &mut self,
        expression: &Expression,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        if let Some(ty) = expected {
            self.require_type(expression, ty)?;
            return Ok(ty);
        }
        return self.expression(expression);
    }

    fn conditional_type(
        &mut self,
        then_value: &Expression,
        then_type: Type,
        else_value: &Expression,
        else_type: Type,
    ) -> Result<Type, Diagnostic> {
        if then_type.is_borrow() || else_type.is_borrow() {
            return Err(Diagnostic::new(
                then_value.span.through(else_value.span),
                "conditional expressions require value results, not borrowed references",
            ));
        }
        if let Some(common) = then_type.common_numeric(else_type) {
            return Ok(common);
        }
        if then_type == else_type {
            return Ok(then_type);
        }
        if let (Type::Pointer(a), Type::Pointer(b)) = (then_type, else_type) {
            let a = self.types.pointers[a];
            let b = self.types.pointers[b];
            if a.pointee == b.pointee || a.pointee == Type::Void || b.pointee == Type::Void {
                let pointee = if a.pointee == Type::Void || b.pointee == Type::Void {
                    Type::Void
                } else {
                    a.pointee
                };
                return Ok(self.types.qualified_pointer_to(
                    pointee,
                    a.mutable && b.mutable,
                    a.volatile || b.volatile,
                ));
            }
        }
        if matches!(then_type, Type::Pointer(_)) && constant_integer(else_value) == Some(0) {
            return Ok(then_type);
        }
        if matches!(else_type, Type::Pointer(_)) && constant_integer(then_value) == Some(0) {
            return Ok(else_type);
        }
        return Err(Diagnostic::new(
            then_value.span.through(else_value.span),
            "conditional branches require compatible value types",
        ));
    }

    fn function_reference(&mut self, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if matches!(name, "print" | "box_int" | "move" | "slice") {
            return Err(Diagnostic::new(
                span,
                "compiler builtins do not have callback addresses",
            ));
        }
        let signature = self.signatures[name].clone();
        let function_type = self.types.function_type(
            signature.return_type,
            signature.parameters,
            signature.variadic,
            signature.calling_convention,
            span,
        )?;
        self.function_values.insert(span, name.to_owned());
        return Ok(self.types.pointer_to(function_type, true));
    }

    fn record_binding_symbol(&mut self, span: Span, variable: Variable) {
        if let Some(declaration) = variable.declaration {
            self.symbol_uses.insert(span, declaration);
        }
    }

    fn record_function_symbol(&mut self, name: &str, span: Span) {
        if let Some(declaration) = self.function_declarations.get(name) {
            self.symbol_uses.insert(span, *declaration);
        }
    }

    fn record_call(
        &mut self,
        span: Span,
        callee: Span,
        target: CallTarget,
        arguments: &[Expression],
    ) {
        self.calls.entry(span).or_insert_with(|| CallSite {
            callee,
            target,
            arguments: arguments.iter().map(|argument| argument.span).collect(),
        });
    }

    fn record_field_symbol(&mut self, ty: Type, name: &str, span: Span) {
        if let Type::Struct(id) = ty
            && let Some(field) = self.types.structs[id]
                .fields
                .iter()
                .find(|field| field.name == name)
        {
            self.symbol_uses.insert(span, field.span);
        }
    }

    fn variadic_arguments(&mut self, arguments: &[Expression]) -> Result<(), Diagnostic> {
        for argument in arguments {
            let ty = if let ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } = &argument.kind
                && !self.function_address(operand)
            {
                let ty = self.raw_address(argument, operand)?;
                self.expression_types.insert(argument.span, ty);
                ty
            } else {
                self.expression(argument)?
            };
            if ty.is_borrow()
                || self.types.owns(ty)
                || !self.types.is_sized(ty)
                || matches!(ty, Type::Array(_))
            {
                return Err(Diagnostic::new(
                    argument.span,
                    "variadic arguments require C values or raw pointers",
                ));
            }
            if let Some(kind) = self.types.integer_representation(ty) {
                let promoted = Type::Integer(kind.promoted());
                if promoted != ty {
                    self.conversions.insert(argument.span, promoted);
                }
            }
            if ty == Type::Float(FloatType::Float) {
                self.conversions
                    .insert(argument.span, Type::Float(FloatType::Double));
            }
        }
        return Ok(());
    }

    fn static_reference(&mut self, id: usize, name: &str, span: Span) -> Result<Type, Diagnostic> {
        let key = static_method_name(id, name);
        if !self.signatures.contains_key(&key) {
            return Err(Diagnostic::new(
                span,
                format!("unknown static method '{name}'"),
            ));
        }
        return self.function_reference(&key, span);
    }

    fn convert(
        &mut self,
        expression: &Expression,
        actual: Type,
        expected: Type,
    ) -> Result<(), Diagnostic> {
        if actual == expected {
            return Ok(());
        }
        if let (
            Type::Borrow {
                pointee: source,
                mutable: source_mutable,
                volatile: source_volatile,
            },
            Type::Borrow {
                pointee: target,
                mutable: target_mutable,
                volatile: target_volatile,
            },
        ) = (actual, expected)
            && (source == target
                || matches!((source, target), (Pointee::Array(a), Pointee::Slice(b))
                if self.types.arrays[a].element == self.types.arrays[b].element))
            && (source_mutable || !target_mutable)
            && source_volatile == target_volatile
        {
            if !self.unevaluated {
                let id = self.references[&expression.span];
                self.loans
                    .iter_mut()
                    .find(|loan| loan.id == id)
                    .expect("reference has a live loan")
                    .mutable = target_mutable;
            }
            self.conversions.insert(expression.span, expected);
            return Ok(());
        }
        if let Type::Pointer(target) = expected {
            let compatible = if let Type::Pointer(source) = actual {
                let source = self.types.pointers[source];
                let target = self.types.pointers[target];
                (source.pointee == target.pointee
                    || (target.pointee == Type::Void
                        && !matches!(source.pointee, Type::Function(_))))
                    && (source.mutable || !target.mutable)
                    && (!source.volatile || target.volatile)
            } else {
                constant_integer(expression) == Some(0)
            };
            if compatible {
                self.conversions.insert(expression.span, expected);
                return Ok(());
            }
        }
        if actual.numeric() && expected.numeric() {
            self.conversions.insert(expression.span, expected);
            return Ok(());
        }
        return Err(Diagnostic::new(
            expression.span,
            format!(
                "expected an {} value, found {}",
                self.types.describe(expected),
                self.types.describe(actual)
            ),
        ));
    }

    fn integer(&self, ty: Type, span: Span) -> Result<IntType, Diagnostic> {
        return ty.integer().ok_or_else(|| {
            Diagnostic::new(
                span,
                format!(
                    "expected an integer or bool value, found {}",
                    self.types.describe(ty)
                ),
            )
        });
    }

    fn condition(&mut self, expression: &Expression) -> Result<(), Diagnostic> {
        let ty = self.expression(expression)?;
        self.scalar_condition(ty, expression.span)?;
        self.release_temporaries();
        return Ok(());
    }

    fn scalar_condition(&self, ty: Type, span: Span) -> Result<(), Diagnostic> {
        if !matches!(ty, Type::Pointer(_) | Type::Float(_)) {
            self.integer(ty, span)?;
        }
        return Ok(());
    }

    fn require_unsafe(&self, span: Span, operation: &str) -> Result<(), Diagnostic> {
        if self.unsafe_depth == 0 && !self.unevaluated {
            return Err(Diagnostic::new(
                span,
                format!("{operation} requires an unsafe block"),
            ));
        }
        return Ok(());
    }

    fn pointer_offset(
        &mut self,
        pointer: Type,
        offset: &Expression,
        offset_type: Type,
    ) -> Result<(), Diagnostic> {
        self.require_unsafe(offset.span, "raw pointer arithmetic or indexing")?;
        let Type::Pointer(id) = pointer else {
            unreachable!()
        };
        if !self.types.is_sized(self.types.pointers[id].pointee) {
            return Err(Diagnostic::new(
                offset.span,
                "pointer arithmetic and indexing require a complete object type",
            ));
        }
        self.integer(offset_type, offset.span)?;
        return self.convert(
            offset,
            offset_type,
            Type::Integer(IntType::named("ptrdiff_t").unwrap()),
        );
    }

    // Resolve the receiver's type without evaluating it. In particular, probing
    // move(value).method() must not move value before checking the actual call.
    fn receiver_type(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        let ty = self.probe_receiver_type(expression)?.referent();
        self.receiver_types.insert(expression.span, ty);
        return Ok(ty);
    }

    fn probe_receiver_type(&mut self, expression: &Expression) -> Result<Type, Diagnostic> {
        if let Some((value, explicit_kind)) = integer_literal(expression) {
            return Ok(Type::Integer(if let Some(kind) = explicit_kind {
                kind
            } else if IntType::I32.fits(value) {
                IntType::I32
            } else if IntType::I64.fits(value) {
                IntType::I64
            } else {
                IntType::U64
            }));
        }
        match &expression.kind {
            ExpressionKind::Update { target, .. } => return self.receiver_type(target),
            ExpressionKind::Conditional {
                then_value,
                else_value,
                ..
            } => {
                let then_type = self.receiver_type(then_value)?;
                let else_type = self.receiver_type(else_value)?;
                return self.conditional_type(then_value, then_type, else_value, else_type);
            }
            ExpressionKind::Storage { .. } => return Ok(Type::Void),
            ExpressionKind::StaticReference { id, name, .. } => {
                return self.static_reference(*id, name, expression.span);
            }
            ExpressionKind::IndirectCall { callee, .. } => {
                let ty = self.receiver_type(callee)?;
                let function = if let Type::Function(id) = ty {
                    Some(&self.types.function_types[id])
                } else {
                    self.types.callback(ty)
                };
                return function.map(|function| function.result).ok_or_else(|| {
                    Diagnostic::new(callee.span, "indirect calls require a function pointer")
                });
            }
            ExpressionKind::Boolean(_) => return Ok(Type::Bool),
            ExpressionKind::Floating { kind, .. } => return Ok(Type::Float(*kind)),
            ExpressionKind::SizeofType(_)
            | ExpressionKind::SizeofValue(_)
            | ExpressionKind::Alignof(_) => {
                return Ok(Type::Integer(IntType::named("size_t").unwrap()));
            }
            ExpressionKind::StringLiteral { id, .. } => return Ok(Type::Array(*id)),
            ExpressionKind::Unary {
                operator: UnaryOperator::Address,
                operand,
            } => {
                if let ExpressionKind::StaticReference { id, name, .. } = &operand.kind {
                    return self.static_reference(*id, name, expression.span);
                }
                if let ExpressionKind::Variable { name, .. } = &operand.kind
                    && !self
                        .scopes
                        .iter()
                        .rev()
                        .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    return self.function_reference(name, expression.span);
                }
                let pointee = self.receiver_type(operand)?;
                let mutable = self.probe_mutability(operand)?;
                let volatile = self.probe_volatile(operand)?;
                return Ok(self.types.qualified_pointer_to(pointee, mutable, volatile));
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let a = self.receiver_type(left)?;
                let b = self.receiver_type(right)?;
                if matches!(
                    operator,
                    BinaryOperator::Equal
                        | BinaryOperator::NotEqual
                        | BinaryOperator::Less
                        | BinaryOperator::LessEqual
                        | BinaryOperator::Greater
                        | BinaryOperator::GreaterEqual
                        | BinaryOperator::And
                        | BinaryOperator::Or
                ) {
                    return Ok(Type::Integer(IntType::I32));
                }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if matches!(a, Type::Pointer(_)) {
                        return Ok(
                            if *operator == BinaryOperator::Subtract
                                && matches!(b, Type::Pointer(_))
                            {
                                Type::Integer(IntType::named("ptrdiff_t").unwrap())
                            } else {
                                a
                            },
                        );
                    }
                    if *operator == BinaryOperator::Add && matches!(b, Type::Pointer(_)) {
                        return Ok(b);
                    }
                }
                if operator.is_shift() {
                    self.integer(b, right.span)?;
                    return Ok(Type::Integer(self.integer(a, left.span)?.promoted()));
                }
                return a.common_numeric(b).ok_or_else(|| {
                    Diagnostic::new(expression.span, "arithmetic requires numeric operands")
                });
            }
            ExpressionKind::Cast { ty, .. } => return Ok(*ty),
            ExpressionKind::Variable { name, .. } => {
                if !self
                    .scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains_key(name))
                    && self.signatures.contains_key(name)
                {
                    return self.function_reference(name, expression.span);
                }
                return Ok(self.available(name, expression.span)?.ty);
            }
            ExpressionKind::Field { base, name, .. } => {
                let ty = self.receiver_type(base)?;
                return self.field_type(ty, name, expression.span);
            }
            ExpressionKind::Index { base, .. } => {
                return match self.receiver_type(base)? {
                    Type::Array(id) => Ok(self.types.arrays[id].element),
                    Type::Pointer(id) => Ok(self.types.pointers[id].pointee),
                    _ => Err(Diagnostic::new(
                        base.span,
                        "indexing requires an array or raw pointer",
                    )),
                };
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return match self.receiver_type(operand)? {
                    Type::BoxInt => Ok(Type::Integer(IntType::I32)),
                    Type::Pointer(id) => Ok(self.types.pointers[id].pointee),
                    _ => Err(Diagnostic::new(
                        expression.span,
                        "dereference needs a BoxInt or a borrow",
                    )),
                };
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Borrow { mutable },
                operand,
            } => {
                let pointee = self.receiver_type(operand)?;
                let volatile = self.probe_volatile(operand)?;
                return pointee
                    .borrowed_qualified(*mutable, volatile)
                    .ok_or_else(|| Diagnostic::new(expression.span, "invalid borrow type"));
            }
            ExpressionKind::Unary { operator, operand } => {
                if *operator == UnaryOperator::Not {
                    return Ok(Type::Integer(IntType::I32));
                }
                let ty = self.receiver_type(operand)?;
                if matches!(ty, Type::Float(_))
                    && matches!(operator, UnaryOperator::Plus | UnaryOperator::Negate)
                {
                    return Ok(ty);
                }
                return Ok(Type::Integer(self.integer(ty, operand.span)?.promoted()));
            }
            ExpressionKind::StaticCall { id, name, .. } => {
                return self
                    .signatures
                    .get(&static_method_name(*id, name))
                    .map(|signature| signature.return_type)
                    .ok_or_else(|| {
                        Diagnostic::new(expression.span, format!("unknown static method '{name}'"))
                    });
            }
            ExpressionKind::Call {
                name, arguments, ..
            } if name == "move" && arguments.len() == 1 => {
                return self.receiver_type(&arguments[0]);
            }
            ExpressionKind::Call { name, .. } => {
                if let Some(variable) = self.scopes.iter().rev().find_map(|scope| scope.get(name)) {
                    return self
                        .types
                        .callback(variable.ty)
                        .map(|signature| signature.result)
                        .ok_or_else(|| {
                            Diagnostic::new(expression.span, "variable is not callable")
                        });
                }
                return self
                    .signatures
                    .get(name)
                    .map(|signature| signature.return_type)
                    .ok_or_else(|| {
                        Diagnostic::new(expression.span, format!("unknown function '{name}'"))
                    });
            }
            ExpressionKind::MethodCall { receiver, name, .. } => {
                if let Type::Struct(id) = self.receiver_type(receiver)? {
                    if self.types.structs[id]
                        .fields
                        .iter()
                        .any(|field| field.name == *name)
                    {
                        let ty = self.field_type(Type::Struct(id), name, expression.span)?;
                        if let Some(signature) = self.types.callback(ty) {
                            return Ok(signature.result);
                        }
                    }
                    if let Some(signature) = self.signatures.get(&method_name(id, name)) {
                        return Ok(signature.return_type);
                    }
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

    fn probe_mutability(&mut self, expression: &Expression) -> Result<bool, Diagnostic> {
        return match &expression.kind {
            ExpressionKind::Variable { name, .. } => {
                let variable = self.available(name, expression.span)?;
                Ok(match variable.ty {
                    Type::Borrow { mutable, .. } => mutable,
                    _ => variable.mutable,
                })
            }
            ExpressionKind::Field { base, name, .. } => {
                let base_type = self.receiver_type(base)?;
                let ty = self.field_type(base_type, name, expression.span)?;
                Ok(self.probe_mutability(base)?
                    && !matches!(ty, Type::Borrow { mutable: false, .. }))
            }
            ExpressionKind::Index { base, .. } => match self.receiver_type(base)? {
                Type::Pointer(id) => Ok(self.types.pointers[id].mutable),
                _ => self.probe_mutability(base),
            },
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => match self.receiver_type(operand)? {
                Type::Borrow { mutable, .. } => Ok(mutable),
                Type::Pointer(id) => Ok(self.types.pointers[id].mutable),
                _ => self.probe_mutability(operand),
            },
            _ => Err(Diagnostic::new(
                expression.span,
                "address target must be a place",
            )),
        };
    }

    fn probe_volatile(&mut self, expression: &Expression) -> Result<bool, Diagnostic> {
        return match &expression.kind {
            ExpressionKind::Variable { name, .. } => {
                let variable = self.available(name, expression.span)?;
                Ok(match variable.ty {
                    Type::Borrow { volatile, .. } => volatile,
                    _ => variable.volatile,
                })
            }
            ExpressionKind::Field { base, .. } => self.probe_volatile(base),
            ExpressionKind::Index { base, .. } => match self.receiver_type(base)? {
                Type::Pointer(id) => Ok(self.types.pointers[id].volatile),
                _ => self.probe_volatile(base),
            },
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => match self.receiver_type(operand)? {
                Type::Pointer(id) => Ok(self.types.pointers[id].volatile),
                Type::Borrow { volatile, .. } => Ok(volatile),
                Type::BoxInt => Ok(false),
                _ => self.probe_volatile(operand),
            },
            _ => Err(Diagnostic::new(
                expression.span,
                "address target must be a place",
            )),
        };
    }

    fn array_literal(
        &mut self,
        expression: &Expression,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        let ExpressionKind::ArrayLiteral { id, elements } = &expression.kind else {
            unreachable!()
        };
        let id = match (*id, expected) {
            (Some(id), _) | (None, Some(Type::Array(id))) => id,
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "an array initializer needs an array type",
                ));
            }
        };
        let array = self.types.arrays[id];
        if elements.len() != array.length {
            return Err(Diagnostic::new(
                expression.span,
                format!(
                    "array initializer needs {} elements, got {}",
                    array.length,
                    elements.len()
                ),
            ));
        }
        let mut captures = Vec::new();
        for element in elements {
            self.require_type(element, array.element)?;
            captures.extend(
                self.capture_expressions
                    .get(&element.span)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        self.remember_captures(expression.span, captures);
        let ty = Type::Array(id);
        self.expression_types.insert(expression.span, ty);
        return Ok(ty);
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
        if !self.types.structs[id].complete {
            return Err(Diagnostic::new(
                expression.span,
                "cannot initialize an incomplete struct",
            ));
        }
        let declared = self.types.structs[id].fields.clone();
        let is_union = self.types.structs[id].is_union;
        if is_union {
            self.require_unsafe(expression.span, "initializing a union")?;
        }
        if (is_union || self.types.structs[id].tagged) && fields.len() != 1 {
            return Err(Diagnostic::new(
                expression.span,
                "a union initializer selects exactly one field",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut captures = Vec::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                return Err(Diagnostic::new(
                    field.span,
                    format!("field '{}' is initialized more than once", field.name),
                ));
            }
            let ty = self.field_type(Type::Struct(id), &field.name, field.span)?;
            self.record_field_symbol(Type::Struct(id), &field.name, field.name_span);
            self.require_type(&field.value, ty)?;
            let mut nested = if ty.is_borrow() && !self.unevaluated {
                vec![captures::Capture::new(self.references[&field.value.span])]
            } else {
                self.capture_expressions
                    .get(&field.value.span)
                    .cloned()
                    .unwrap_or_default()
            };
            for capture in &mut nested {
                capture.prepend(if self.types.structs[id].tagged {
                    "*"
                } else {
                    &field.name
                });
            }
            if captures.len() + nested.len() > 4096 {
                return Err(Diagnostic::new(
                    expression.span,
                    "reference-field tracking exceeds 4096 fields",
                ));
            }
            captures.extend(nested);
        }
        self.remember_captures(expression.span, captures);
        for field in &declared {
            if !is_union && !self.types.structs[id].tagged && !seen.contains(field.name.as_str()) {
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

    fn checked_place(&mut self, expression: &Expression) -> Result<Place, Diagnostic> {
        let place = match self.check_place(expression) {
            Ok(place) => place,
            Err(error) => {
                if self.completion
                    && let ExpressionKind::Field { base, .. } = &expression.kind
                {
                    let _ = self.receiver_type(base);
                }
                return Err(error);
            }
        };
        let place = if place.ty.is_borrow() {
            let ty = self.stored_reference(expression, &place)?;
            self.reference_place(expression, ty)?
        } else {
            place
        };
        self.receiver_types.insert(expression.span, place.ty);
        self.place_volatile.insert(expression.span, place.volatile);
        return Ok(place);
    }

    fn check_place(&mut self, expression: &Expression) -> Result<Place, Diagnostic> {
        match &expression.kind {
            ExpressionKind::Call { .. }
            | ExpressionKind::MethodCall { .. }
            | ExpressionKind::StaticCall { .. } => {
                let place = self.temporary_place(expression)?;
                if self.expression_types[&expression.span].is_borrow() {
                    return Ok(place);
                }
                return Err(Diagnostic::new(
                    expression.span,
                    "a temporary value is not an assignable place",
                ));
            }
            ExpressionKind::Index { base, index } => {
                let place = if base.is_place() {
                    self.checked_place(base)?
                } else {
                    self.temporary_place(base)?
                };
                let element = match place.ty {
                    Type::Array(id) => self.types.arrays[id].element,
                    Type::Pointer(id) => self.types.pointers[id].pointee,
                    _ => {
                        return Err(Diagnostic::new(base.span, "indexing requires an array"));
                    }
                };
                let previous_loans = self.loans.len();
                self.borrow(
                    place.root.clone(),
                    place.fields.clone(),
                    false,
                    place.via,
                    base.span,
                )?;
                let index_type = self.expression(index)?;
                self.integer(index_type, index.span)?;
                if let Type::Array(id) = place.ty {
                    if let Some(value) = constant_integer(index)
                        && (value < 0
                            || (self.types.arrays[id].length != 0
                                && value >= self.types.arrays[id].length as i128))
                    {
                        return Err(Diagnostic::new(index.span, "array index is out of bounds"));
                    }
                } else {
                    self.pointer_offset(place.ty, index, index_type)?;
                }
                self.loans.truncate(previous_loans);
                let mutable = if let Type::Pointer(id) = place.ty {
                    self.types.pointers[id].mutable
                } else {
                    place.mutable
                };
                return Ok(Place {
                    ty: element,
                    mutable,
                    volatile: if let Type::Pointer(id) = place.ty {
                        self.types.pointers[id].volatile
                    } else {
                        place.volatile
                    },
                    ..place
                });
            }
            ExpressionKind::Field {
                base,
                name,
                name_span,
            } => {
                let place = self.checked_place(base)?;
                let ty = self.field_type(place.ty, name, expression.span)?;
                self.record_field_symbol(place.ty, name, *name_span);
                let projection = if matches!(place.ty, Type::Struct(id) if self.types.structs[id].is_union || self.types.structs[id].tagged)
                {
                    "*"
                } else {
                    name
                };
                let mut fields = place.fields;
                fields.push(projection.to_owned());
                return Ok(Place {
                    ty,
                    mutable: place.mutable
                        && !matches!(place.ty, Type::Array(id) if self.types.arrays[id].length == 0),
                    fields,
                    ..place
                });
            }
            ExpressionKind::Variable { name, name_span } => {
                let place = self.place(expression)?;
                let variable = self.lookup(name, expression.span)?;
                self.record_binding_symbol(*name_span, variable);
                self.expression_types
                    .entry(expression.span)
                    .or_insert(place.ty);
                return Ok(place);
            }
            ExpressionKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                let place = if operand.is_place() {
                    self.checked_place(operand)?
                } else {
                    self.temporary_place(operand)?
                };
                let (ty, mutable, volatile) = match place.ty {
                    Type::BoxInt => (Type::Integer(IntType::I32), place.mutable, false),
                    Type::Pointer(id) => {
                        self.require_unsafe(expression.span, "dereferencing a raw pointer")?;
                        let pointer = self.types.pointers[id];
                        if pointer.pointee == Type::Void {
                            return Err(Diagnostic::new(
                                expression.span,
                                "void pointers must be cast before dereferencing",
                            ));
                        }
                        (pointer.pointee, pointer.mutable, pointer.volatile)
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            expression.span,
                            "dereference requires a raw pointer or a heap owner",
                        ));
                    }
                };
                return Ok(Place {
                    ty,
                    mutable,
                    volatile,
                    ..place
                });
            }
            _ => {
                return Err(Diagnostic::new(
                    expression.span,
                    "borrow or dereference target must be a named variable, its pointee, or a returned borrow",
                ));
            }
        }
    }

    fn temporary_place(&mut self, expression: &Expression) -> Result<Place, Diagnostic> {
        let ty = self.reference_expression(expression)?;
        if !ty.is_borrow() && !matches!(ty, Type::Pointer(_)) {
            return Err(Diagnostic::new(
                expression.span,
                "dereference of a temporary requires a returned borrow or raw pointer",
            ));
        }
        if ty.is_borrow() {
            return self.reference_place(expression, ty);
        }
        return Ok(Place {
            ty,
            mutable: false,
            volatile: false,
            root: (self.scopes.len(), format!("@{}", expression.span.start)),
            fields: Vec::new(),
            via: None,
        });
    }

    fn field_type(&self, ty: Type, name: &str, span: Span) -> Result<Type, Diagnostic> {
        let field_type = self.types.field(ty, name, span)?;
        if matches!(ty, Type::Struct(id) if self.types.structs[id].is_union) {
            self.require_unsafe(span, "accessing a union field")?;
        }
        if let Type::Struct(id) = ty
            && self.current_struct != Some(id)
            && self.types.structs[id]
                .fields
                .iter()
                .any(|field| field.name == name && field.private)
        {
            return Err(Diagnostic::new(
                span,
                format!(
                    "field '{name}' is private to struct '{}'",
                    self.types.structs[id].name
                ),
            ));
        }
        return Ok(field_type);
    }

    fn place(&self, expression: &Expression) -> Result<Place, Diagnostic> {
        let ExpressionKind::Variable { name, .. } = &expression.kind else {
            return Err(Diagnostic::new(
                expression.span,
                "expected a named variable",
            ));
        };
        let variable = self.available(name, expression.span)?;
        let (root, fields) = if let Some(id) = variable.reference {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == id)
                .expect("local reference has a live loan");
            (loan.root.clone(), loan.fields.clone())
        } else {
            (self.root(name), Vec::new())
        };
        return Ok(Place {
            ty: variable.ty.referent(),
            volatile: match variable.ty {
                Type::Borrow { volatile, .. } => volatile,
                _ => variable.volatile,
            },
            mutable: variable.mutable
                && match variable.ty {
                    Type::Borrow { mutable, .. } => mutable,
                    _ => true,
                },
            root,
            fields,
            via: variable.reference,
        });
    }

    fn root(&self, name: &str) -> (usize, String) {
        for (index, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                if let Some(storage) = scope[name].storage {
                    return (0, self.global_storage[storage].link_name.clone());
                }
                return (
                    index,
                    if index == 0 {
                        self.global_symbols[name].clone()
                    } else {
                        name.to_owned()
                    },
                );
            }
        }
        unreachable!("variable is resolved before finding its root");
    }

    fn access(
        &self,
        root: &(usize, String),
        fields: &[String],
        exclusive: bool,
        via: Option<usize>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let parents: Vec<_> = via.into_iter().collect();
        self.access_through(root, fields, exclusive, &parents, span)?;
        if let Some(loan) = via.and_then(|id| self.loans.iter().find(|loan| loan.id == id))
            && &loan.root == root
        {
            let suffix = fields.strip_prefix(loan.fields.as_slice()).unwrap_or(&[]);
            for (other, projection) in &loan.alternatives {
                let mut projected = projection.clone();
                projected.extend_from_slice(suffix);
                self.access_through(other, &projected, exclusive, &parents, span)?;
            }
        }
        return Ok(());
    }

    fn access_through(
        &self,
        root: &(usize, String),
        fields: &[String],
        exclusive: bool,
        parents: &[usize],
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.unevaluated {
            return Ok(());
        }
        let mut authorized = HashSet::new();
        let mut pending = parents.to_vec();
        while let Some(id) = pending.pop() {
            if authorized.insert(id)
                && let Some(loan) = self.loans.iter().find(|loan| loan.id == id)
            {
                pending.extend(&loan.parents);
            }
        }
        if self.loans.iter().any(|loan| {
            ((&loan.root == root && fields_overlap(fields, &loan.fields))
                || loan
                    .alternatives
                    .iter()
                    .any(|(other, projection)| other == root && fields_overlap(fields, projection)))
                && !authorized.contains(&loan.id)
                && (exclusive || loan.mutable)
        }) {
            let name = root
                .1
                .strip_prefix("!capture:")
                .map_or_else(|| root.1.clone(), |capture| capture.replace(':', "."));
            return Err(Diagnostic::new(
                span,
                format!("conflicting access to '{name}' while it is borrowed"),
            ));
        }
        return Ok(());
    }

    fn borrow(
        &mut self,
        root: (usize, String),
        fields: Vec<String>,
        mutable: bool,
        parent: Option<usize>,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        if self.unevaluated {
            return Ok(usize::MAX);
        }
        self.access(&root, &fields, mutable, parent, span)?;
        let alternatives = if let Some(parent) = parent {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == parent)
                .expect("parent loan");
            let suffix = fields.strip_prefix(loan.fields.as_slice()).unwrap_or(&[]);
            loan.alternatives
                .iter()
                .map(|(root, projection)| {
                    let mut projection = projection.clone();
                    projection.extend_from_slice(suffix);
                    (root.clone(), projection)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for (other, projection) in &alternatives {
            self.access(other, projection, mutable, parent, span)?;
        }
        let id = self.next_loan;
        self.next_loan += 1;
        self.loans.push(Loan {
            id,
            root,
            fields,
            mutable,
            parents: parent.into_iter().collect(),
            alternatives,
            scope: None,
        });
        return Ok(id);
    }

    fn returned_reference(
        &mut self,
        span: Span,
        arguments: &[Expression],
        sources: &[usize],
        previous_loans: usize,
        return_type: Type,
    ) -> Result<(), Diagnostic> {
        let mutable = matches!(return_type, Type::Borrow { mutable: true, .. })
            || self.types.contains_mutable_borrows(return_type);
        let mut origins = Vec::new();
        let mut parents = Vec::new();
        let mut ids = Vec::new();
        for index in sources {
            let span = arguments[*index].span;
            if let Some(id) = self.references.get(&span) {
                ids.push(*id);
            }
            ids.extend(
                self.capture_expressions
                    .get(&span)
                    .into_iter()
                    .flatten()
                    .map(|capture| capture.id()),
            );
        }
        for id in ids {
            let loan = self
                .loans
                .iter()
                .find(|loan| loan.id == id)
                .expect("borrowed argument has a loan");
            origins.push((loan.root.clone(), loan.fields.clone()));
            origins.extend(loan.alternatives.clone());
            if self.loans[..previous_loans]
                .iter()
                .any(|existing| existing.id == id)
            {
                parents.push(id);
            }
            let mut pending = loan.parents.clone();
            let mut seen = HashSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if self.loans[..previous_loans]
                    .iter()
                    .any(|loan| loan.id == id)
                {
                    parents.push(id);
                } else if let Some(loan) = self.loans.iter().find(|loan| loan.id == id) {
                    pending.extend(&loan.parents);
                }
            }
        }
        let mut unique = HashSet::new();
        origins.retain(|origin| unique.insert(origin.clone()));
        parents.sort_unstable();
        parents.dedup();
        self.loans.truncate(previous_loans);
        for (root, fields) in &origins {
            self.access_through(root, fields, mutable, &parents, span)?;
        }
        if origins.is_empty() {
            if return_type.is_borrow() {
                return Err(Diagnostic::new(
                    span,
                    "returned reference requires a live borrow source",
                ));
            }
            return Ok(());
        }
        let (root, fields) = origins.remove(0);
        let id = self.next_loan;
        self.next_loan += 1;
        self.loans.push(Loan {
            id,
            root,
            fields,
            alternatives: origins,
            parents,
            mutable,
            scope: None,
        });
        if return_type.is_borrow() {
            self.references.insert(span, id);
        }
        if self.types.contains_borrows(return_type.referent()) {
            let mut fields = Vec::new();
            captures::reference_fields(
                self.types,
                return_type.referent(),
                Vec::new(),
                &mut fields,
                span,
            )?;
            let captures = fields
                .into_iter()
                .map(|(path, _)| captures::Capture::at(path, id))
                .collect();
            self.remember_captures(span, captures);
        }
        return Ok(());
    }

    fn declare(
        &mut self,
        name: &str,
        ty: Type,
        state: State,
        span: Span,
        declaration: Option<Span>,
    ) -> Result<(), Diagnostic> {
        if !self.types.is_sized(ty) {
            return Err(Diagnostic::new(
                span,
                "a value declaration requires a complete object type",
            ));
        }
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(Diagnostic::new(
                span,
                format!("variable '{name}' is already declared in this scope"),
            ));
        }
        scope.insert(
            name.to_owned(),
            Variable {
                storage: None,
                volatile: false,
                external: false,
                mutable: true,
                declaration,
                ty,
                state,
                reference: None,
            },
        );
        return Ok(());
    }

    fn lookup(&self, name: &str, span: Span) -> Result<Variable, Diagnostic> {
        for scope in self.scopes.iter().rev() {
            if let Some(variable) = scope.get(name) {
                if variable.external {
                    self.require_unsafe(span, "accessing an external C variable")?;
                } else if variable.storage.is_some() && variable.mutable {
                    self.require_unsafe(span, "accessing mutable global or static storage")?;
                }
                return Ok(*variable);
            }
        }
        return Err(Diagnostic::new(span, format!("unknown variable '{name}'")));
    }

    fn available(&self, name: &str, span: Span) -> Result<Variable, Diagnostic> {
        let variable = self.lookup(name, span)?;
        if self.unevaluated {
            return Ok(variable);
        }
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
        if self.unevaluated {
            return;
        }
        for scope in self.scopes.iter_mut().rev() {
            if let Some(variable) = scope.get_mut(name) {
                variable.state = state;
                return;
            }
        }
        unreachable!("variable was resolved before changing its state");
    }
}

fn constant_integer(expression: &Expression) -> Option<i128> {
    return integer_literal(expression).map(|(value, _)| value);
}

// Unsuffixed literals can adopt their destination type. A suffix fixes the
// source type, including its native C rank, before any checked conversion.
fn integer_literal(expression: &Expression) -> Option<(i128, Option<IntType>)> {
    return match &expression.kind {
        ExpressionKind::Integer(value) => Some((i128::from(*value), None)),
        ExpressionKind::TypedInteger { value, kind } => Some((i128::from(*value), Some(*kind))),
        ExpressionKind::Unary {
            operator: operator @ (UnaryOperator::Plus | UnaryOperator::Negate),
            operand,
        } => match operand.kind {
            ExpressionKind::Integer(_) | ExpressionKind::TypedInteger { .. } => {
                let (mut value, kind) = integer_literal(operand)?;
                if *operator == UnaryOperator::Negate {
                    value = -value;
                    if let Some(kind) = kind
                        && !kind.signed()
                    {
                        value &= kind.maximum();
                    }
                }
                Some((value, kind))
            }
            _ => None,
        },
        _ => None,
    };
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

fn fields_overlap(left: &[String], right: &[String]) -> bool {
    for (a, b) in left.iter().zip(right) {
        if a == "*" || b == "*" {
            return true;
        }
        if a != b {
            return false;
        }
    }
    return true;
}
