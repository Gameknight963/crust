use super::*;

pub(super) struct Record {
    pub name: String,
    pub tag: &'static str,
    pub fields: Vec<(String, String, Option<u32>)>,
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
                let bit_width = if field["isBitfield"] == true {
                    let value = children(field)
                        .iter()
                        .find(|node| kind(node) == "ConstantExpr")
                        .ok_or("Clang omitted a bitfield width")?;
                    Some(
                        u32::try_from(enums::constant(value)?)
                            .map_err(|_| "invalid C bitfield width")?,
                    )
                } else {
                    None
                };
                let probe = format!("__crust_import_record_{}_{}", records.len(), fields.len());
                let field_type = if bit_width.is_some() {
                    field["type"]["qualType"]
                        .as_str()
                        .ok_or("Clang omitted a bitfield type")?
                        .to_owned()
                } else {
                    format!("__typeof__((({tag} {name} *)0)->{field_name})")
                };
                let type_names: Vec<_> = if bit_width.is_some() {
                    field_type
                        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .filter(|word| {
                            !word.is_empty() && !word.starts_with(|ch: char| ch.is_ascii_digit())
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                for name in &type_names {
                    source.push_str(&format!("#pragma push_macro(\"{name}\")\n#undef {name}\n"));
                }
                source.push_str(&format!("#pragma push_macro(\"{field_name}\")\n#undef {field_name}\ntypedef {field_type} {probe};\n#pragma pop_macro(\"{field_name}\")\n"));
                for name in type_names.iter().rev() {
                    source.push_str(&format!("#pragma pop_macro(\"{name}\")\n"));
                }
                fields.push((field_name.to_owned(), probe, bit_width));
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
