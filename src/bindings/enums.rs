use crate::ast::IntType;
pub(super) struct Enum {
    pub name: String,
    pub underlying: IntType,
    pub values: Vec<(String, i128)>,
}
