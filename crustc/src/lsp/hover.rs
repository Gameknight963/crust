use super::*;
use crate::analysis::SymbolKind;
use crate::lexer::Token;

pub(super) fn classified(context: &semantic::Context, hover: &mut Value) {
    let Some(text) = hover["contents"]["value"].as_str() else {
        return;
    };
    let text = text
        .lines()
        .filter(|line| !line.starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut runs = Vec::new();
    let mut previous = 0;
    for token in crate::lexer::discovery_tokens(&text) {
        if token.kind == Token::EndOfFile {
            continue;
        }
        if token.span.start > previous {
            runs.push(run("text", &text[previous..token.span.start], Value::Null));
        }
        let spelling = &text[token.span.start..token.span.end];
        let symbol = context
            .analysis
            .symbols
            .iter()
            .find(|symbol| symbol.name == spelling);
        let class = match &token.kind {
            Token::Identifier(name) if crate::ast::IntType::named(name).is_some() => "keyword",
            Token::Identifier(_) => symbol
                .map(|symbol| match symbol.kind {
                    SymbolKind::Struct | SymbolKind::Union => "struct name",
                    SymbolKind::Alias => "type",
                    SymbolKind::Enum => "enum name",
                    SymbolKind::Interface => "interface name",
                    SymbolKind::TypeParameter => "type parameter name",
                    SymbolKind::Function | SymbolKind::Method => "method name",
                    SymbolKind::Field => "field name",
                    SymbolKind::EnumMember => "enum member name",
                    SymbolKind::Parameter => "parameter name",
                    SymbolKind::Variable | SymbolKind::Global => "local name",
                })
                .unwrap_or("identifier"),
            Token::IntegerLiteral(_)
            | Token::SuffixedIntegerLiteral { .. }
            | Token::FloatingLiteral { .. } => "number",
            Token::StringLiteral(_) => "string",
            _ if spelling
                .chars()
                .all(|ch| !ch.is_alphanumeric() && ch != '_') =>
            {
                "punctuation"
            }
            _ => "keyword",
        };
        let target = symbol.map(|symbol| json!({"uri":context.uri(symbol.location.file),"range":context.range(symbol.location)})).unwrap_or(Value::Null);
        runs.push(run(class, spelling, target));
        previous = token.span.end;
    }
    if previous < text.len() {
        runs.push(run("text", &text[previous..], Value::Null));
    }
    hover["_vs_rawContent"] = json!({"_vs_type":"ContainerElement","Style":1,"Elements":[{
        "_vs_type":"ClassifiedTextElement","Runs":runs
    }]});
}

fn run(class: &str, text: &str, target: Value) -> Value {
    return json!({"_vs_type":"ClassifiedTextRun","ClassificationTypeName":class,"Text":text,
        "Style":0,"_crust_target":target,"_crust_navigationHint":"Go to definition"});
}
