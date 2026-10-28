use super::*;

pub(super) fn unaligned_name(ty: Type) -> String {
    let name = c_type(ty)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    return format!("crust_unaligned_{name}");
}

impl Emitter<'_> {
    pub(super) fn native_array_copy_initializer(&self, ty: Type, value: &str) -> String {
        let Type::Array(id) = ty else {
            return value.to_owned();
        };
        let array = self.types.arrays[id];
        let elements = (0..array.length)
            .map(|index| {
                self.native_array_copy_initializer(
                    array.element,
                    &format!("({value}).crust_elements[{index}]"),
                )
            })
            .collect::<Vec<_>>();
        return format!("{{ {} }}", elements.join(", "));
    }
    fn native_field_pointer(&self, mut ty: Type, qualifiers: &str) -> String {
        let mut dimensions = String::new();
        while let Type::Array(id) = ty {
            let array = self.types.arrays[id];
            dimensions.push_str(&format!("[{}]", array.length));
            ty = array.element;
        }
        if dimensions.is_empty() {
            return format!("{} {qualifiers}*", c_type(ty));
        }
        return format!("{} {qualifiers}(*){dimensions}", c_type(ty));
    }
    pub(super) fn unaligned_type(&mut self, ty: Type) {
        self.line(&format!(
            "typedef {} {} __attribute__((__aligned__(1)));",
            c_type(ty),
            unaligned_name(ty)
        ));
    }
    pub(super) fn unaligned_scalar_types(&mut self) {
        for kind in [
            IntType::Char,
            IntType::I8,
            IntType::I16,
            IntType::I32,
            IntType::I64,
            IntType::U8,
            IntType::U16,
            IntType::U32,
            IntType::U64,
            IntType::Long,
            IntType::ULong,
            IntType::LongLong,
            IntType::ULongLong,
        ] {
            self.unaligned_type(Type::Integer(kind));
        }
        for kind in [FloatType::Float, FloatType::Double, FloatType::LongDouble] {
            self.unaligned_type(Type::Float(kind));
        }
        self.unaligned_type(Type::Bool);
        for id in 0..self.types.enums.len() {
            self.unaligned_type(Type::Enum(id));
        }
    }
    pub(super) fn native_record_checks(&mut self) {
        for id in 0..self.types.structs.len() {
            let info = &self.types.structs[id];
            if !info.complete || info.native_name.is_none() {
                continue;
            }
            let names: Vec<_> = info
                .fields
                .iter()
                .map(|field| field.native_name.as_ref().unwrap_or(&field.name).clone())
                .collect();
            for name in &names {
                self.line(&format!("#pragma push_macro(\"{name}\")\n#undef {name}"));
            }
            for (index, field) in info.fields.iter().enumerate() {
                let native = field.native_name.as_ref().unwrap_or(&field.name);
                let qualifiers = format!(
                    "{}{}",
                    if field.readonly { "const " } else { "" },
                    if field.volatile { "volatile " } else { "" }
                );
                self.line(&format!("_Static_assert(__builtin_types_compatible_p(__typeof__(&((crust_type_{id} *)0)->{}), {}), \"native field type mismatch: {}.{}\");",native,self.native_field_pointer(field.ty,&qualifiers),info.name,field.name));
                if matches!(field.ty, Type::Array(_)) {
                    self.line(&format!("_Static_assert(sizeof({}) == sizeof(((crust_type_{id} *)0)->{native}), \"native array layout mismatch\");",c_type(field.ty)));
                }
                if !info.is_union && index > 0 {
                    let previous = info.fields[index - 1]
                        .native_name
                        .as_ref()
                        .unwrap_or(&info.fields[index - 1].name);
                    self.line(&format!("_Static_assert(__builtin_offsetof(crust_type_{id}, {previous}) + sizeof(((crust_type_{id} *)0)->{previous}) <= __builtin_offsetof(crust_type_{id}, {}), \"native fields must be nonoverlapping and in header order\");",native));
                }
                let record = unaligned_name(Type::Struct(id));
                let ty = unaligned_name(field.ty);
                for (access, readonly, volatile) in [
                    ("mut", false, false),
                    ("const", true, false),
                    ("mutv", false, true),
                    ("constv", true, true),
                ] {
                    let record_qualifiers = format!(
                        "{}{}",
                        if readonly { "const " } else { "" },
                        if volatile { "volatile " } else { "" }
                    );
                    let field_qualifiers = format!(
                        "{}{}",
                        if readonly || field.readonly {
                            "const "
                        } else {
                            ""
                        },
                        if volatile || field.volatile {
                            "volatile "
                        } else {
                            ""
                        }
                    );
                    let cast = if matches!(field.ty, Type::Array(_)) {
                        format!("({ty} {field_qualifiers}*)")
                    } else {
                        String::new()
                    };
                    self.line(&format!("static inline {ty} {field_qualifiers}*crust_native_field_{access}_{id}_{}({record} {record_qualifiers}*crust_record) {{ return {cast}&crust_record->{native}; }}",field.name));
                }
            }
            for name in names.iter().rev() {
                self.line(&format!("#pragma pop_macro(\"{name}\")"));
            }
        }
    }

    pub(super) fn native_field_macros(&mut self, restore: bool) {
        let mut names: Vec<_> = self
            .types
            .structs
            .iter()
            .filter(|info| info.complete && info.native_name.is_some())
            .flat_map(|info| {
                info.fields
                    .iter()
                    .map(|field| field.native_name.as_ref().unwrap_or(&field.name).clone())
            })
            .collect();
        names.sort();
        names.dedup();
        for name in names {
            if restore {
                self.line(&format!("#pragma pop_macro(\"{name}\")"));
            } else {
                self.line(&format!("#pragma push_macro(\"{name}\")\n#undef {name}"));
            }
        }
    }
}
