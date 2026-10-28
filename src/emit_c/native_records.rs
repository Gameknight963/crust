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
                self.line(&format!("_Static_assert(__builtin_types_compatible_p(__typeof__(&((crust_type_{id} *)0)->{}), {} *), \"native field type mismatch: {}.{}\");",native,c_type(field.ty),info.name,field.name));
                if !info.is_union && index > 0 {
                    let previous = info.fields[index - 1]
                        .native_name
                        .as_ref()
                        .unwrap_or(&info.fields[index - 1].name);
                    self.line(&format!("_Static_assert(__builtin_offsetof(crust_type_{id}, {previous}) + sizeof(((crust_type_{id} *)0)->{previous}) <= __builtin_offsetof(crust_type_{id}, {}), \"native fields must be nonoverlapping and in header order\");",native));
                }
                let record = unaligned_name(Type::Struct(id));
                let ty = unaligned_name(field.ty);
                self.line(&format!("static inline {ty} *crust_native_field_mut_{id}_{}({record} *crust_record) {{ return &crust_record->{}; }}",field.name,native));
                self.line(&format!("static inline {ty} const *crust_native_field_const_{id}_{}({record} const *crust_record) {{ return &crust_record->{}; }}",field.name,native));
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
