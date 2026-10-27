use super::*;
use crate::ast::{CallingConvention, Function};
use crate::check::CallTarget;
use crate::lexer::{SpannedToken, Token, tokenize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureParameter {
    pub label: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    pub parameters: Vec<SignatureParameter>,
    pub variadic: bool,
}

#[derive(Debug, Clone)]
pub struct SignatureHelp {
    pub signatures: Vec<Signature>,
    pub argument: usize,
}

#[derive(Debug)]
struct Call {
    location: Location,
    open: usize,
    separators: Vec<usize>,
    signatures: Vec<Signature>,
}

#[derive(Debug, Default)]
pub(super) struct Index {
    calls: Vec<Call>,
}

fn signature(prefix: String, parameters: Vec<String>, variadic: bool, suffix: &str) -> Signature {
    let mut label = format!("{prefix}(");
    let mut ranges = Vec::new();
    for parameter in parameters
        .into_iter()
        .chain(variadic.then(|| "...".to_owned()))
    {
        if !ranges.is_empty() {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count();
        label.push_str(&parameter);
        let end = label.encode_utf16().count();
        ranges.push(SignatureParameter {
            label: parameter,
            start,
            end,
        });
    }
    label.push(')');
    label.push_str(suffix);
    return Signature {
        label,
        parameters: ranges,
        variadic,
    };
}

fn function_signature(function: &Function, checked: &AnalyzedProgram, name: String) -> Signature {
    let instance = function.name.starts_with('@');
    let method = function
        .name
        .strip_prefix('@')
        .or_else(|| function.name.strip_prefix('#'));
    let name = if let Some(method) = method {
        let (owner, _) = method.split_once('_').unwrap();
        let owner = &checked.types.structs[owner.parse::<usize>().unwrap()].name;
        format!("{owner}{}{name}", if instance { "." } else { "::" })
    } else {
        name
    };
    let parameters = function
        .parameters
        .iter()
        .skip(usize::from(instance))
        .map(|parameter| {
            let ty = checked.types.describe(parameter.ty);
            return if parameter.synthetic {
                ty
            } else {
                format!("{ty} {}", parameter.name)
            };
        })
        .collect();
    let prefix = format!(
        "{}{}{} {name}",
        if function.link_name.is_some() {
            "extern "
        } else if function.requires_unsafe {
            "unsafe "
        } else {
            ""
        },
        if function.calling_convention == CallingConvention::C {
            ""
        } else {
            function.calling_convention.c_prefix()
        },
        checked.types.describe(function.return_type)
    );
    let shared = instance
        && matches!(
            function.parameters[0].ty,
            Type::Borrow { mutable: false, .. }
        );
    return signature(
        prefix,
        parameters,
        function.variadic,
        if shared { " const" } else { "" },
    );
}

fn tokens_between(tokens: &[SpannedToken], start: usize, end: usize) -> &[SpannedToken] {
    let first = tokens.partition_point(|token| token.span.start < start);
    let last = tokens.partition_point(|token| token.span.start < end);
    return tokens.get(first..last).unwrap_or_default();
}

impl Index {
    pub fn build(
        checked: &AnalyzedProgram,
        files: &[File],
        locate: &impl Fn(Span) -> Location,
    ) -> Self {
        let text = |span: Span| {
            let location = locate(span);
            return files[location.file].text[location.span.start..location.span.end].to_owned();
        };
        let tokens: Vec<_> = files
            .iter()
            .map(|file| tokenize(&file.text).unwrap_or_default())
            .collect();
        let mut functions: HashMap<_, _> = checked
            .program
            .functions
            .iter()
            .map(|function| {
                return (
                    function.name.clone(),
                    function_signature(function, checked, text(function.name_span)),
                );
            })
            .collect();
        for (name, prefix, parameters) in [
            ("print", "void print", vec!["value".to_owned()]),
            ("move", "move", vec!["value".to_owned()]),
            ("box_int", "BoxInt box_int", vec!["int value".to_owned()]),
        ] {
            functions.insert(
                name.to_owned(),
                signature(prefix.to_owned(), parameters, false, ""),
            );
        }
        let mut calls: HashMap<Location, Call> = HashMap::new();
        for analysis in checked.analyses.values() {
            for (span, call) in &analysis.calls {
                let information = match &call.target {
                    CallTarget::Function(name) => functions.get(name).cloned(),
                    CallTarget::Callback(ty) => checked.types.callback(*ty).map(|function| {
                        return signature(
                            format!(
                                "{}{} {}",
                                if function.calling_convention == CallingConvention::C {
                                    ""
                                } else {
                                    function.calling_convention.c_prefix()
                                },
                                checked.types.describe(function.result),
                                text(call.callee)
                            ),
                            function
                                .parameters
                                .iter()
                                .map(|ty| checked.types.describe(*ty))
                                .collect(),
                            function.variadic,
                            "",
                        );
                    }),
                };
                let Some(information) = information else {
                    continue;
                };
                let location = locate(*span);
                let callee = locate(call.callee);
                let arguments: Vec<_> = call.arguments.iter().map(|span| locate(*span)).collect();
                let first_argument = arguments
                    .first()
                    .map_or(location.span.end - 1, |argument| argument.span.start);
                let Some(open) =
                    tokens_between(&tokens[location.file], callee.span.end, first_argument)
                        .iter()
                        .rev()
                        .find(|token| token.kind == Token::LeftParen)
                        .map(|token| token.span.end)
                else {
                    continue;
                };
                let separators = arguments
                    .windows(2)
                    .filter_map(|arguments| {
                        return tokens_between(
                            &tokens[location.file],
                            arguments[0].span.end,
                            arguments[1].span.start,
                        )
                        .iter()
                        .find(|token| token.kind == Token::Comma)
                        .map(|token| token.span.start);
                    })
                    .collect();
                let call = calls.entry(location).or_insert_with(|| Call {
                    location,
                    open,
                    separators,
                    signatures: Vec::new(),
                });
                if !call.signatures.contains(&information) {
                    call.signatures.push(information);
                }
            }
        }
        let mut calls: Vec<_> = calls.into_values().collect();
        for call in &mut calls {
            call.signatures.sort_by(|a, b| a.label.cmp(&b.label));
        }
        calls.sort_by_key(|call| {
            (
                call.location.file,
                call.location.span.start,
                call.location.span.end,
            )
        });
        return Self { calls };
    }
}

impl Analysis {
    pub fn signature_help_at(&self, file: usize, offset: usize) -> Option<SignatureHelp> {
        if !self.files.get(file)?.text.is_char_boundary(offset) {
            return None;
        }
        let call = self
            .signatures
            .calls
            .iter()
            .filter(|call| {
                return call.location.file == file
                    && call.open <= offset
                    && offset < call.location.span.end;
            })
            .min_by_key(|call| call.location.span.end - call.location.span.start)?;
        return Some(SignatureHelp {
            signatures: call.signatures.clone(),
            argument: call
                .separators
                .iter()
                .filter(|separator| **separator < offset)
                .count(),
        });
    }
}
