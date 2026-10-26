use super::semantic::{Context, PositionParams, offset};
use super::*;
use crate::analysis::SymbolKind;
use crate::lexer::{Token, tokenize};

struct Site {
    start: usize,
    end: usize,
    cursor: usize,
}

fn site(text: &str, cursor: usize) -> Option<Site> {
    // Tokenizing the prefix excludes strings and unfinished block comments.
    let tokens = tokenize(&text[..cursor]).ok()?;
    let last = tokens
        .iter()
        .rev()
        .find(|token| token.kind != Token::EndOfFile);
    let mut gap = &text[last.map_or(0, |token| token.span.end)..cursor];
    loop {
        gap = gap.trim_start();
        if let Some(comment) = gap.strip_prefix("//") {
            gap = &comment[comment.find(['\r', '\n'])?..];
        } else if let Some(comment) = gap.strip_prefix("/*") {
            gap = &comment[comment.find("*/")? + 2..];
        } else {
            break;
        }
    }
    let word = |byte: &u8| byte.is_ascii_alphanumeric() || *byte == b'_';
    let start = text.as_bytes()[..cursor]
        .iter()
        .rposition(|byte| !word(byte))
        .map_or(0, |index| index + 1);
    let end = cursor
        + text.as_bytes()[cursor..]
            .iter()
            .take_while(|byte| word(byte))
            .count();
    if start < end && text.as_bytes()[start].is_ascii_digit() {
        return None;
    }
    let previous = tokens
        .iter()
        .rev()
        .find(|token| token.kind != Token::EndOfFile && token.span.end <= start);
    if previous
        .is_some_and(|token| matches!(token.kind, Token::Dot | Token::Arrow | Token::ColonColon))
    {
        return None;
    }
    return Some(Site { start, end, cursor });
}

fn context(uri: &str, site: &Site, documents: &HashMap<String, Document>) -> Option<Context> {
    if let Some(context) = Context::load(uri, documents) {
        return Some(context);
    }
    // Repair just the word being completed, keeping byte offsets stable when
    // it has enough space. This snapshot is used only for completion requests.
    for replacement in ["0", "int", "0;"] {
        let mut documents = documents.clone();
        let document = documents.get_mut(uri)?;
        let padding = " ".repeat((site.end - site.start).saturating_sub(replacement.len()));
        document
            .text
            .replace_range(site.start..site.end, &format!("{replacement}{padding}"));
        if let Some(context) = Context::load(uri, &documents) {
            return Some(context);
        }
    }
    return None;
}

fn kind(kind: SymbolKind) -> u8 {
    return match kind {
        SymbolKind::Function => 3,
        SymbolKind::Method => 2,
        SymbolKind::Struct | SymbolKind::Union => 22,
        SymbolKind::Enum => 13,
        SymbolKind::EnumMember => 20,
        SymbolKind::Field => 5,
        SymbolKind::Parameter | SymbolKind::Variable | SymbolKind::Global => 6,
        SymbolKind::Interface => 8,
        SymbolKind::Alias | SymbolKind::TypeParameter => 25,
    };
}

pub(super) fn complete(params: PositionParams, documents: &HashMap<String, Document>) -> Value {
    let Some(document) = documents.get(&params.text_document.uri) else {
        return Value::Array(Vec::new());
    };
    let Some(cursor) = offset(&document.text, &params.position) else {
        return Value::Array(Vec::new());
    };
    let Some(site) = site(&document.text, cursor) else {
        return Value::Array(Vec::new());
    };
    let prefix = &document.text[site.start..site.cursor];
    let range = json!({ "start": position(&document.text, site.start), "end": position(&document.text, site.end) });
    let mut items = std::collections::BTreeMap::new();
    if let Some(context) = context(&params.text_document.uri, &site, documents) {
        for symbol in context.analysis.completions_at(context.file, site.start) {
            if symbol.name.starts_with(prefix) {
                items.insert(
                    symbol.name.clone(),
                    json!({
                        "label": symbol.name, "kind": kind(symbol.kind), "detail": symbol.detail,
                        "textEdit": { "range": range, "newText": symbol.name }
                    }),
                );
            }
        }
    }
    for name in [
        "int",
        "char",
        "signed",
        "unsigned",
        "short",
        "long",
        "bool",
        "void",
        "true",
        "false",
        "return",
        "if",
        "else",
        "while",
        "for",
        "break",
        "continue",
        "const",
        "struct",
        "union",
        "enum",
        "interface",
        "import",
        "init",
        "destroy",
        "BoxInt",
        "typedef",
        "static",
        "extern",
        "unsafe",
        "private",
        "sizeof",
        "alignof",
        "int8_t",
        "int16_t",
        "int32_t",
        "int64_t",
        "uint8_t",
        "uint16_t",
        "uint32_t",
        "uint64_t",
        "size_t",
        "ptrdiff_t",
        "intptr_t",
        "uintptr_t",
        "print",
        "box_int",
        "move",
    ] {
        if name.starts_with(prefix) {
            items.entry(name.to_owned()).or_insert_with(|| {
                json!({
                    "label": name, "kind": if matches!(name, "print" | "box_int" | "move") { 3 } else { 14 }, "textEdit": { "range": range, "newText": name }
                })
            });
        }
    }
    return Value::Array(items.into_values().collect());
}
