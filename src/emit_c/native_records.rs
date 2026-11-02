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
    pub(super) fn bitfield(&self, expression: &Expression) -> Option<(usize, String, u32)> {
        let ExpressionKind::Field { base, name, .. } = &expression.kind else {
            return None;
        };
        let Type::Struct(id) = self.place_type(base) else {
            return None;
        };
        let field = self.types.structs[id]
            .fields
            .iter()
            .find(|field| field.name == *name)?;
        return field.bit_width.map(|width| (id, name.clone(), width));
    }

    pub(super) fn capture_destination(&mut self, expression: &Expression) -> CapturedPlace {
        if let Some((id, name, _)) = self.bitfield(expression) {
            let ExpressionKind::Field { base, .. } = &expression.kind else {
                unreachable!()
            };
            let place = self.place(base);
            let pointer = self.temporary(self.address_type(base, true), &format!("&({place})"));
            let access = if self
                .place_volatile
                .get(&base.span)
                .copied()
                .unwrap_or(false)
            {
                "mutv"
            } else {
                "mut"
            };
            return CapturedPlace::Bitfield {
                pointer,
                id,
                name,
                access,
            };
        }
        let place = self.place(expression);
        let address = self.temporary(self.address_type(expression, true), &format!("&({place})"));
        return CapturedPlace::Address(address);
    }

    pub(super) fn check_bitfield_value(&mut self, ty: Type, width: u32, value: &str, span: Span) {
        if ty == Type::Bool {
            return;
        }
        let Some(kind) = self.types.integer_representation(ty) else {
            return;
        };
        if width >= u32::from(kind.bits()) {
            return;
        }
        let condition = if kind.signed() {
            let min = -(1i128 << (width - 1));
            let max = (1i128 << (width - 1)) - 1;
            format!("(int64_t)({value}) < INT64_C({min}) || (int64_t)({value}) > INT64_C({max})")
        } else {
            let max = (1u128 << width) - 1;
            format!("(uint64_t)({value}) > UINT64_C({max})")
        };
        let location = self.runtime_location(span);
        self.line(&format!(
            "if ({condition}) crust_rt_fail(\"bitfield value out of range\", {location});"
        ));
    }

    pub(super) fn store_destination(
        &mut self,
        expression: &Expression,
        destination: &CapturedPlace,
        value: &str,
    ) {
        match destination {
            CapturedPlace::Address(pointer) => self.line(&format!("*{pointer} = {value};")),
            CapturedPlace::Bitfield {
                pointer,
                id,
                name,
                access,
            } => {
                let field = self.types.structs[*id]
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .unwrap();
                self.check_bitfield_value(
                    field.ty,
                    field.bit_width.unwrap(),
                    value,
                    expression.span,
                );
                self.line(&format!(
                    "crust_native_bit_store_{access}_{id}_{name}({pointer}, {value});"
                ));
            }
        }
    }

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
                if field.bit_width.is_some() {
                    let record = unaligned_name(Type::Struct(id));
                    let ty = c_type(field.ty);
                    for (access, qualifiers) in [
                        ("mut", ""),
                        ("const", "const "),
                        ("mutv", "volatile "),
                        ("constv", "const volatile "),
                    ] {
                        self.line(&format!("static inline {ty} crust_native_bit_load_{access}_{id}_{}({record} {qualifiers}*crust_record) {{ return crust_record->{native}; }}",field.name));
                        if !field.readonly && (access == "mut" || access == "mutv") {
                            self.line(&format!("static inline void crust_native_bit_store_{access}_{id}_{}({record} {qualifiers}*crust_record, {ty} crust_value) {{ crust_record->{native} = crust_value; }}",field.name));
                        }
                    }
                    continue;
                }
                self.line(&format!("_Static_assert(__builtin_types_compatible_p(__typeof__(&((crust_type_{id} *)0)->{}), {}), \"native field type mismatch: {}.{}\");",native,self.native_field_pointer(field.ty,&qualifiers),info.name,field.name));
                if matches!(field.ty, Type::Array(_)) {
                    self.line(&format!("_Static_assert(sizeof({}) == sizeof(((crust_type_{id} *)0)->{native}), \"native array layout mismatch\");",c_type(field.ty)));
                }
                if !info.is_union && index > 0 && info.fields[index - 1].bit_width.is_none() {
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
