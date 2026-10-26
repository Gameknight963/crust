use std::collections::HashSet;

use crate::ast::{Field, Program, Type};
use crate::diagnostic::{Diagnostic, Span};

pub struct StructInfo {
    pub name: String,
    pub fields: Vec<Field>,
    pub owns: bool,
    pub has_destructor: bool,
}

pub struct Types {
    pub structs: Vec<StructInfo>,
    // Dependencies precede the structs that contain them by value.
    pub order: Vec<usize>,
}

impl Types {
    pub fn build(program: &Program) -> Result<Types, Diagnostic> {
        let mut definitions = vec![None; program.struct_names.len()];
        for definition in &program.structs {
            if definitions[definition.id].is_some() {
                return Err(Diagnostic::new(
                    definition.span,
                    format!(
                        "struct '{}' is already defined",
                        program.struct_names[definition.id].0
                    ),
                ));
            }
            definitions[definition.id] = Some(definition);
        }

        let mut structs = Vec::new();
        for (id, (name, first_use)) in program.struct_names.iter().enumerate() {
            let definition = definitions[id]
                .ok_or_else(|| Diagnostic::new(*first_use, format!("unknown struct '{name}'")))?;
            if definition.fields.is_empty() {
                return Err(Diagnostic::new(
                    definition.span,
                    "a struct needs at least one field",
                ));
            }
            let mut names = HashSet::new();
            for field in &definition.fields {
                if !names.insert(&field.name) {
                    return Err(Diagnostic::new(
                        field.span,
                        format!(
                            "field '{}' is already declared in struct '{name}'",
                            field.name
                        ),
                    ));
                }
                if field.ty == Type::Void || field.ty.is_borrow() {
                    return Err(Diagnostic::new(
                        field.span,
                        "struct fields must be values; void and borrowed fields are not supported",
                    ));
                }
            }
            structs.push(StructInfo {
                name: name.clone(),
                fields: definition.fields.clone(),
                owns: false,
                has_destructor: definition.has_destructor,
            });
        }

        let mut types = Types {
            structs,
            order: Vec::new(),
        };
        let mut states = vec![0; types.structs.len()];
        for id in 0..states.len() {
            types.visit(id, &mut states, program.struct_names[id].1)?;
        }
        return Ok(types);
    }

    fn visit(&mut self, id: usize, states: &mut [u8], span: Span) -> Result<(), Diagnostic> {
        if states[id] == 2 {
            return Ok(());
        }
        if states[id] == 1 {
            return Err(Diagnostic::new(
                span,
                format!(
                    "recursive by-value layout involving struct '{}'",
                    self.structs[id].name
                ),
            ));
        }
        states[id] = 1;
        let mut owns = self.structs[id].has_destructor;
        for field in self.structs[id].fields.clone() {
            match field.ty {
                Type::BoxInt => owns = true,
                Type::Struct(child) => {
                    self.visit(child, states, field.span)?;
                    owns = owns || self.structs[child].owns;
                }
                _ => {}
            }
        }
        self.structs[id].owns = owns;
        states[id] = 2;
        self.order.push(id);
        return Ok(());
    }

    pub fn owns(&self, ty: Type) -> bool {
        return match ty {
            Type::BoxInt => true,
            Type::Struct(id) => self.structs[id].owns,
            _ => false,
        };
    }

    pub fn describe(&self, ty: Type) -> String {
        return match ty {
            Type::Int => String::from("int"),
            Type::Bool => String::from("bool"),
            Type::Void => String::from("void"),
            Type::BoxInt => String::from("BoxInt"),
            Type::Struct(id) => format!("struct {}", self.structs[id].name),
            Type::Borrow { pointee, mutable } => format!(
                "{}{}^",
                if mutable { "" } else { "const " },
                self.describe(pointee.ty())
            ),
        };
    }

    pub fn field(&self, ty: Type, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if let Type::Struct(id) = ty {
            return self.structs[id]
                .fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.ty)
                .ok_or_else(|| {
                    Diagnostic::new(
                        span,
                        format!("struct '{}' has no field '{name}'", self.structs[id].name),
                    )
                });
        }
        return Err(Diagnostic::new(
            span,
            "field access needs a struct value; use '->' for a borrowed struct",
        ));
    }
}
