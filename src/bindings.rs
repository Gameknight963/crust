//! Native C declaration discovery through libclang's stable C API.
use serde_json::Value;
use std::collections::{HashMap, HashSet};
mod enums;
mod libclang;
mod records;
mod render;
pub(crate) use libclang::declarations as native_declarations;

fn crust_identifier(name: &str) -> bool {
    let Ok(tokens) = crate::lexer::tokenize(name) else {
        return false;
    };
    return tokens.len() == 2
        && matches!(&tokens[0].kind, crate::lexer::Token::Identifier(value) if value == name)
        && crate::ast::IntType::named(name).is_none()
        && !matches!(name, "Self" | "this" | "print" | "move" | "slice");
}

fn children(node: &Value) -> &[Value] {
    return node["inner"].as_array().map(Vec::as_slice).unwrap_or(&[]);
}

fn kind(node: &Value) -> &str {
    return node["kind"].as_str().unwrap_or("");
}

fn type_children(node: &Value) -> impl DoubleEndedIterator<Item = &Value> {
    return children(node)
        .iter()
        .filter(|node| kind(node).ends_with("Type"));
}
