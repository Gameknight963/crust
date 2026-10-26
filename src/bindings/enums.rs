use super::*;
use crate::ast::IntType;

pub(super) struct Enum {
    pub name: String,
    pub underlying: IntType,
    pub values: Vec<(String, i128)>,
}

fn discover(node: &Value, names: &mut std::collections::BTreeSet<String>) -> Result<(), String> {
    if kind(node) == "EnumType" {
        let name = node["decl"]["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or("anonymous C enums require a native wrapper")?;
        if !identifier(name) {
            return Err("C enum tags must use ASCII identifiers".to_owned());
        }
        names.insert(name.to_owned());
    }
    for child in type_children(node) {
        discover(child, names)?;
    }
    return Ok(());
}

fn descendants<'a>(node: &'a Value, expected: &str) -> Vec<&'a Value> {
    let mut found = Vec::new();
    if kind(node) == expected {
        found.push(node);
    }
    for child in children(node) {
        found.extend(descendants(child, expected));
    }
    return found;
}

fn constant(node: &Value) -> Result<i128, String> {
    return descendants(node, "ConstantExpr")
        .into_iter()
        .find_map(|node| node["value"].as_str())
        .ok_or("Clang omitted an enum constant value")?
        .parse()
        .map_err(|_| "C enum value exceeds the supported integer range".to_owned());
}

fn representation(code: i128) -> Result<IntType, String> {
    return match code {
        1 => Ok(IntType::Char),
        2 => Ok(IntType::I8),
        3 => Ok(IntType::U8),
        4 => Ok(IntType::I16),
        5 => Ok(IntType::U16),
        6 => Ok(IntType::I32),
        7 => Ok(IntType::U32),
        8 => Ok(IntType::native_long()),
        9 => Ok(IntType::native_long().unsigned()),
        10 => Ok(IntType::native_long_long()),
        11 => Ok(IntType::native_long_long().unsigned()),
        _ => Err("unsupported C enum integer representation".to_owned()),
    };
}

pub(super) fn load(
    header: &Path,
    options: &Options,
    probes: &[Value],
) -> Result<Vec<Enum>, String> {
    let mut names = std::collections::BTreeSet::new();
    for probe in probes {
        discover(probe, &mut names)?;
    }
    let mut members = Vec::new();
    for name in &names {
        let bytes = clang(header, options, "", true, Some(name))?;
        let declarations: Vec<Value> = serde_json::Deserializer::from_slice(&bytes)
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let definition = declarations
            .iter()
            .find(|node| {
                kind(node) == "EnumDecl"
                    && node["name"] == *name
                    && children(node)
                        .iter()
                        .any(|node| kind(node) == "EnumConstantDecl")
            })
            .ok_or_else(|| format!("C enum '{name}' requires a complete global definition"))?;
        members.push(
            children(definition)
                .iter()
                .filter(|node| kind(node) == "EnumConstantDecl")
                .map(|node| {
                    node["name"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("Clang omitted an enum member name")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        if members.last().unwrap().iter().any(|name| !identifier(name)) {
            return Err("C enum members must use ASCII identifiers".to_owned());
        }
    }
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut source = String::new();
    for name in names.iter().chain(members.iter().flatten()) {
        source.push_str(&format!("#undef {name}\n"));
    }
    source.push_str("enum __crust_import_enum_representations {\n");
    for (index, name) in names.iter().enumerate() {
        source.push_str(&format!("__crust_import_enum_repr_{index} = _Generic((enum {name})0, char: 1, signed char: 2, unsigned char: 3, short: 4, unsigned short: 5, int: 6, unsigned int: 7, long: 8, unsigned long: 9, long long: 10, unsigned long long: 11),\n"));
    }
    source.push_str("};\nenum __crust_import_enum_values {\n");
    for (index, name) in names.iter().enumerate() {
        for (member, value) in members[index].iter().enumerate() {
            source.push_str(&format!("__crust_import_enum_value_{index}_{member} = (unsigned long long)((enum {name}){value}),\n"));
        }
    }
    source.push_str("};\n");
    let bytes = clang(header, options, &source, true, Some("__crust_import_enum_"))?;
    let probes: Vec<Value> = serde_json::Deserializer::from_slice(&bytes)
        .into_iter()
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let constants: Vec<_> = probes
        .iter()
        .flat_map(|node| descendants(node, "EnumConstantDecl"))
        .collect();
    let value = |name: &str| -> Result<i128, String> {
        return constant(
            constants
                .iter()
                .find(|node| node["name"] == name)
                .ok_or("Clang omitted an enum probe")?,
        );
    };
    let mut enums = Vec::new();
    for (index, name) in names.into_iter().enumerate() {
        let underlying = representation(value(&format!("__crust_import_enum_repr_{index}"))?)?;
        let mut values = Vec::new();
        for (member, name) in members[index].iter().enumerate() {
            let mut raw = value(&format!("__crust_import_enum_value_{index}_{member}"))?;
            if underlying.signed() && raw > i128::from(i64::MAX) {
                raw -= 1_i128 << 64;
            }
            if !underlying.fits(raw) {
                return Err(format!(
                    "C enum value '{name}' does not fit its native representation"
                ));
            }
            values.push((name.clone(), raw));
        }
        enums.push(Enum {
            name,
            underlying,
            values,
        });
    }
    return Ok(enums);
}
