use super::*;

fn slot(span: Span) -> String {
    return format!("crust_temporary_{}_{}", span.start, span.end);
}
fn drop_name(ty: Type) -> String {
    return match ty {
        Type::Struct(id) => format!("crust_temporary_drop_struct_{id}"),
        _ => unreachable!(),
    };
}

impl Emitter<'_> {
    pub(super) fn temporary_runtime(&mut self) {
        self.line(
            "struct crust_temporary { void *crust_value; void (*crust_drop)(void *); };
static inline void crust_cleanup_temporaries(struct crust_temporary *crust_values, size_t *crust_count) {
    while (*crust_count > 0) { --*crust_count; crust_values[*crust_count].crust_drop(crust_values[*crust_count].crust_value); }
}",
        );
        for id in 0..self.types.structs.len() {
            if self.types.owns(Type::Struct(id)) {
                self.line(&format!("static inline void {}(void *crust_value) {{ crust_drop_struct_{id}(({} *)crust_value); }}", drop_name(Type::Struct(id)),native_records::unaligned_name(Type::Struct(id))));
            }
        }
    }

    pub(super) fn temporary_slots(&mut self) {
        if self.temporary_owners.is_empty() {
            return;
        }
        self.line(&format!(
            "struct crust_temporary crust_temporaries[{}]; size_t crust_temporary_count = 0;",
            self.temporary_owners.len()
        ));
    }

    pub(super) fn cleanup_temporaries(&mut self) {
        if !self.temporary_owners.is_empty() {
            self.line("crust_cleanup_temporaries(crust_temporaries, &crust_temporary_count);");
        }
    }

    pub(super) fn materialize_temporary(&mut self, expression: &Expression, ty: Type) -> String {
        let previous = self.materializing.replace((expression.span, ty));
        let value = self.expression(expression).expect("temporary owner value");
        self.materializing = previous;
        let slot = slot(expression.span);
        self.line(&format!("{} {slot} = {value};", c_type(ty)));
        if self.types.owns(ty) {
            self.line(&format!("crust_temporaries[crust_temporary_count++] = (struct crust_temporary){{ &{slot}, {} }};", drop_name(ty)));
        }
        return slot;
    }
}
