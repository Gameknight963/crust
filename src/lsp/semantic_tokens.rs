use super::semantic::{Context, DocumentParams, RequestError};
use super::*;
use crate::analysis::SymbolKind;
use crate::ast::IntType;
use crate::diagnostic::Span;
use crate::lexer::{Token, tokenize};

pub(super) const TYPES: &[&str] = &[
    "type",
    "struct",
    "enum",
    "interface",
    "typeParameter",
    "function",
    "method",
    "property",
    "enumMember",
    "parameter",
    "variable",
];
pub(super) const MODIFIERS: &[&str] = &["declaration", "defaultLibrary"];

const VISUAL_STUDIO_TYPES: &[&str] = &[
    "type",
    "struct name",
    "enum name",
    "interface name",
    "type parameter name",
    "method name",
    "method name",
    "field name",
    "enum member name",
    "parameter name",
    "local name",
    "keyword",
];

pub(super) fn legend(visual_studio: bool) -> &'static [&'static str] {
    return if visual_studio {
        VISUAL_STUDIO_TYPES
    } else {
        TYPES
    };
}

fn kind(kind: SymbolKind) -> usize {
    return match kind {
        SymbolKind::Alias => 0,
        SymbolKind::Struct | SymbolKind::Union => 1,
        SymbolKind::Enum => 2,
        SymbolKind::Interface => 3,
        SymbolKind::TypeParameter => 4,
        SymbolKind::Function => 5,
        SymbolKind::Method => 6,
        SymbolKind::Field => 7,
        SymbolKind::EnumMember => 8,
        SymbolKind::Parameter => 9,
        SymbolKind::Variable | SymbolKind::Global => 10,
    };
}

fn builtin(token: &Token) -> Option<usize> {
    return match token {
        Token::Int
        | Token::Char
        | Token::Signed
        | Token::Unsigned
        | Token::Short
        | Token::Long
        | Token::Bool
        | Token::Void
        | Token::BoxInt => Some(0),
        Token::Identifier(name) if IntType::named(name).is_some() => Some(0),
        Token::Identifier(name) if matches!(name.as_str(), "print" | "move" | "box_int") => Some(5),
        Token::Identifier(name) if name == "this" => Some(9),
        _ => None,
    };
}

pub(super) fn request(
    params: Value,
    documents: &HashMap<String, Document>,
    roots: &[PathBuf],
    visual_studio: bool,
) -> Result<Value, RequestError> {
    let params: DocumentParams = serde_json::from_value(params)?;
    let Some(context) = Context::load(&params.text_document.uri, documents, roots) else {
        return Ok(json!({"data": []}));
    };
    let analysis = &context.analysis;
    let mut declarations = HashMap::new();
    for symbol in &analysis.symbols {
        declarations
            .entry(symbol.declaration)
            .or_insert_with(HashSet::new)
            .insert(kind(symbol.kind));
    }
    let mut bindings: HashMap<Span, (HashSet<usize>, bool)> = HashMap::new();
    for symbol in &analysis.symbols {
        if symbol.location.file == context.file {
            let entry = bindings.entry(symbol.location.span).or_default();
            entry.0.insert(kind(symbol.kind));
            entry.1 = true;
        }
    }
    for reference in &analysis.references {
        if reference.location.file == context.file
            && let Some(kinds) = reference
                .declaration
                .and_then(|target| declarations.get(&target))
        {
            bindings
                .entry(reference.location.span)
                .or_default()
                .0
                .extend(kinds);
        }
    }
    let text = &analysis.files[context.file].text;
    let tokens = tokenize(text).unwrap_or_default();
    let mut data = Vec::new();
    let (mut scanned, mut line, mut column, mut previous_cr) = (0, 0, 0, false);
    let (mut last_line, mut last_column) = (0, 0);
    for token in tokens {
        // Walking the source once keeps UTF-16 positions linear in file size,
        // including comments and string literals between classified names.
        for character in text[scanned..token.span.start].chars() {
            match character {
                '\r' => {
                    line += 1;
                    column = 0;
                    previous_cr = true;
                }
                '\n' => {
                    if !previous_cr {
                        line += 1;
                    }
                    column = 0;
                    previous_cr = false;
                }
                _ => {
                    column += character.len_utf16();
                    previous_cr = false;
                }
            }
        }
        scanned = token.span.start;
        let classification = if let Some((kinds, declaration)) = bindings.get(&token.span) {
            (kinds.len() == 1).then(|| (*kinds.iter().next().unwrap(), usize::from(*declaration)))
        } else {
            builtin(&token.kind).map(|kind| (kind, 2))
        };
        let Some((kind, modifiers)) = classification else {
            continue;
        };
        let kind = if visual_studio && kind == 0 && modifiers & 2 != 0 {
            VISUAL_STUDIO_TYPES.len() - 1
        } else {
            kind
        };
        let length = text[token.span.start..token.span.end]
            .encode_utf16()
            .count();
        if length == 0 {
            continue;
        }
        let delta_line = line - last_line;
        let delta_column = if delta_line == 0 {
            column - last_column
        } else {
            column
        };
        data.extend([delta_line, delta_column, length, kind, modifiers]);
        last_line = line;
        last_column = column;
    }
    return Ok(json!({"data": data}));
}
