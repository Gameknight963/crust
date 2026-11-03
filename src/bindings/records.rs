#[derive(Clone, PartialEq, Eq)]
pub(super) struct Group {
    pub id: usize,
    pub tag: &'static str,
}

pub(super) struct RecordField {
    pub name: String,
    pub probe: String,
    pub bit_width: Option<u32>,
    pub groups: Vec<Group>,
}

pub(super) struct Record {
    pub name: String,
    pub tag: &'static str,
    pub fields: Vec<RecordField>,
}
