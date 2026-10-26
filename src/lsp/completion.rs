use super::semantic::{Context, PositionParams, offset};
use super::*;
use crate::analysis::{MemberAccess, SymbolKind};
use crate::lexer::{Token, tokenize};

struct Site {
    start: usize,
    end: usize,
    cursor: usize,
    member: Option<(usize, MemberAccess)>,
}

#[derive(Deserialize)]
struct CompletionParams {
    #[serde(flatten)]
    position: PositionParams,
    context: Option<CompletionContext>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletionContext {
    trigger_kind: u8,
    trigger_character: Option<String>,
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
        .enumerate()
        .rev()
        .find(|(_, token)| token.kind != Token::EndOfFile && token.span.end <= start);
    let member = previous.and_then(|(index, token)| {
        let access = match token.kind {
            Token::Dot => MemberAccess::Value,
            Token::Arrow => MemberAccess::Dereference,
            Token::ColonColon => MemberAccess::Type,
            _ => return None,
        };
        return index
            .checked_sub(1)
            .map(|index| (tokens[index].span.end - 1, access));
    });
    return Some(Site {
        start,
        end,
        cursor,
        member,
    });
}

fn context(uri: &str, site: &Site, documents: &HashMap<String, Document>) -> Option<Context> {
    if let Some(context) = Context::load_completion(uri, documents) {
        return Some(context);
    }
    // Repair just the word being completed, keeping byte offsets stable when
    // it has enough space. This snapshot is used only for completion requests.
    let replacements: &[&str] = if site.member.is_some() {
        &["__crust_completion", "__crust_completion;"]
    } else {
        &["0", "int", "0;"]
    };
    for replacement in replacements {
        let mut documents = documents.clone();
        let document = documents.get_mut(uri)?;
        let padding = " ".repeat((site.end - site.start).saturating_sub(replacement.len()));
        document
            .text
            .replace_range(site.start..site.end, &format!("{replacement}{padding}"));
        if let Some(context) = Context::load_completion(uri, &documents) {
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

pub(super) fn request(
    params: Value,
    documents: &HashMap<String, Document>,
) -> Result<Value, super::semantic::RequestError> {
    let params: CompletionParams = serde_json::from_value(params)?;
    return Ok(complete(params, documents));
}

fn complete(params: CompletionParams, documents: &HashMap<String, Document>) -> Value {
    let trigger = params.context;
    let params = params.position;
    let Some(document) = documents.get(&params.text_document.uri) else {
        return Value::Array(Vec::new());
    };
    let Some(cursor) = offset(&document.text, &params.position) else {
        return Value::Array(Vec::new());
    };
    let Some(site) = site(&document.text, cursor) else {
        return Value::Array(Vec::new());
    };
    if trigger.is_some_and(|trigger| {
        trigger.trigger_kind == 2 && trigger.trigger_character.is_some() && site.member.is_none()
    }) {
        return Value::Array(Vec::new());
    }
    let prefix = &document.text[site.start..site.cursor];
    let range = json!({ "start": position(&document.text, site.start), "end": position(&document.text, site.end) });
    let mut items = std::collections::BTreeMap::new();
    if let Some(context) = context(&params.text_document.uri, &site, documents) {
        let symbols = if let Some((offset, access)) = site.member {
            context
                .analysis
                .member_completions_at(context.file, offset, access)
        } else {
            context.analysis.completions_at(context.file, site.start)
        };
        for symbol in symbols {
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
    if site.member.is_some() {
        return Value::Array(items.into_values().collect());
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
