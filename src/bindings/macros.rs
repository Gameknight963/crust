use super::*;
use crate::ast::IntType;

pub(super) struct Constant {
    pub name: String,
    integer: Option<IntType>,
    value: i128,
}

impl Constant {
    fn ty(&self) -> &str {
        return self.integer.map_or("bool", IntType::c_name);
    }

    pub fn declaration(&self, name: &str) -> String {
        let value = if self.integer.is_none() {
            if self.value == 0 {
                "false".to_owned()
            } else {
                "true".to_owned()
            }
        } else if self.value == i128::from(i64::MIN) {
            "(-9223372036854775807LL - 1LL)".to_owned()
        } else if self.value < 0 {
            format!("({}LL)", self.value)
        } else {
            format!("{}ULL", self.value)
        };
        return format!("const {} {name} = {value};", self.ty());
    }

    pub fn verification(&self) -> String {
        let raw = if self.value < 0 {
            self.value + (1i128 << 64)
        } else {
            self.value
        };
        return format!(
            "_Static_assert(_Generic(({}), {}: 1, default: 0), \"macro type changed: {}\");\n_Static_assert((unsigned long long)({}) == {raw}ULL, \"macro value changed: {}\");\n",
            self.name,
            if self.integer.is_some() {
                self.ty()
            } else {
                "_Bool"
            },
            self.name,
            self.name,
            self.name
        );
    }
}

pub(super) fn load(header: &Path, options: &Options) -> Result<Vec<Constant>, String> {
    if options.macros.is_empty() {
        return Ok(Vec::new());
    }
    let mut source = String::new();
    for name in &options.macros {
        source.push_str(&format!(
            "#ifndef {name}\n#error selected C macro {name} is undefined\n#endif\n"
        ));
    }
    source.push_str("enum __crust_import_macro_probes {\n");
    for (index, name) in options.macros.iter().enumerate() {
        source.push_str(&format!("__crust_import_macro_type_{index} = _Generic(({name}), char: 1, signed char: 2, unsigned char: 3, short: 4, unsigned short: 5, int: 6, unsigned int: 7, long: 8, unsigned long: 9, long long: 10, unsigned long long: 11, _Bool: 12, default: 0),\n__crust_import_macro_value_{index} = (unsigned long long)({name}),\n"));
    }
    source.push_str("};\n");
    let bytes = clang(
        header,
        options,
        &source,
        true,
        Some("__crust_import_macro_probes"),
    )
    .map_err(|error| format!("integer macro analysis failed: {error}"))?;
    let probe: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let constants = enums::descendants(&probe, "EnumConstantDecl");
    let value = |name: &str| -> Result<i128, String> {
        return enums::constant(
            constants
                .iter()
                .find(|node| node["name"] == name)
                .ok_or("Clang omitted a macro constant")?,
        );
    };
    let mut output = Vec::new();
    for (index, name) in options.macros.iter().enumerate() {
        let code = value(&format!("__crust_import_macro_type_{index}"))?;
        let integer = if code == 12 {
            None
        } else {
            Some(enums::representation(code).map_err(|_| {
                format!("C macro '{name}' requires an integer or boolean constant expression")
            })?)
        };
        let mut raw = value(&format!("__crust_import_macro_value_{index}"))?;
        if integer.is_some_and(IntType::signed) && raw > i128::from(i64::MAX) {
            raw -= 1i128 << 64;
        }
        if integer.is_some_and(|ty| !ty.fits(raw)) || (integer.is_none() && !matches!(raw, 0 | 1)) {
            return Err(format!(
                "C macro '{name}' does not fit its native representation"
            ));
        }
        output.push(Constant {
            name: name.clone(),
            integer,
            value: raw,
        });
    }
    return Ok(output);
}
