use super::*;

pub(super) struct Record {
    pub name: String,
    pub tag: &'static str,
    pub fields: Vec<(String, String)>,
}

pub(super) fn load(header: &Path, options: &Options) -> Result<(Vec<Record>, Vec<Value>), String> {
    let mut selected = HashSet::new();
    let mut records = Vec::new();
    let mut source = String::new();
    for (tag, names) in [("struct", &options.structs), ("union", &options.unions)] {
        for name in names {
            if !identifier(name) {
                return Err(format!("invalid C {tag} selection '{name}'"));
            }
            if !selected.insert(name.clone()) {
                return Err(format!("C record '{name}' was selected twice"));
            }
            let bytes = clang(header, options, "", true, Some(name))?;
            let nodes: Vec<Value> = serde_json::Deserializer::from_slice(&bytes)
                .into_iter()
                .collect::<Result<_, _>>()
                .map_err(|error| error.to_string())?;
            let record = nodes
                .iter()
                .find(|node| {
                    kind(node) == "RecordDecl"
                        && node["name"] == *name
                        && node["completeDefinition"] == true
                        && node["tagUsed"] == tag
                })
                .ok_or_else(|| {
                    format!("C header has no complete {tag} declaration named '{name}'")
                })?;
            source.push_str(&format!("#pragma push_macro(\"{name}\")\n#undef {name}\n"));
            let mut fields = Vec::new();
            for field in children(record)
                .iter()
                .filter(|node| kind(node) == "FieldDecl")
            {
                let Some(field_name) = field["name"].as_str() else {
                    continue;
                };
                if !identifier(field_name) {
                    return Err(format!(
                        "C field '{name}.{field_name}' requires an ASCII name"
                    ));
                }
                if field["isBitfield"] == true {
                    return Err(format!(
                        "C bitfield '{name}.{field_name}' requires a native accessor"
                    ));
                }
                let probe = format!("__crust_import_record_{}_{}", records.len(), fields.len());
                source.push_str(&format!("#pragma push_macro(\"{field_name}\")\n#undef {field_name}\ntypedef __typeof__((({tag} {name} *)0)->{field_name}) {probe};\n#pragma pop_macro(\"{field_name}\")\n"));
                fields.push((field_name.to_owned(), probe));
            }
            source.push_str(&format!("#pragma pop_macro(\"{name}\")\n"));
            records.push(Record {
                name: name.clone(),
                tag,
                fields,
            });
        }
    }
    if records.is_empty() {
        return Ok((records, Vec::new()));
    }
    let bytes = clang(
        header,
        options,
        &source,
        true,
        Some("__crust_import_record_"),
    )?;
    let probes = serde_json::Deserializer::from_slice(&bytes)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    return Ok((records, probes));
}
