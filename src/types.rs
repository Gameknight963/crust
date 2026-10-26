use std::collections::HashSet;

use crate::ast::{ArrayType, EnumDefinition, EnumVariant, Field, IntType, Program, Type};
use crate::diagnostic::{Diagnostic, Span};

pub struct StructInfo {
    pub name: String,
    pub fields: Vec<Field>,
    pub owns: bool,
    pub has_destructor: bool,
}

pub struct Types {
    pub enums: Vec<EnumDefinition>,
    pub enum_names: Vec<String>,
    pub arrays: Vec<ArrayType>,
    array_owns: Vec<bool>,
    pub structs: Vec<StructInfo>,
    // Dependencies precede the structs that contain them by value.
    pub order: Vec<Type>,
}

impl Types {
    pub fn build(program: &Program) -> Result<Types, Diagnostic> {
        let mut enums = Vec::new();
        let mut enumerators = HashSet::new();
        for (id, (name, span)) in program.enum_names.iter().enumerate() {
            let mut definitions = program
                .enums
                .iter()
                .filter(|definition| definition.id == id);
            let definition = definitions
                .next()
                .ok_or_else(|| Diagnostic::new(*span, format!("unknown enum '{name}'")))?;
            if definitions.next().is_some() {
                return Err(Diagnostic::new(
                    definition.span,
                    format!("enum '{name}' is already defined"),
                ));
            }
            if definition.variants.is_empty() {
                return Err(Diagnostic::new(
                    definition.span,
                    "an enum needs at least one value",
                ));
            }
            for variant in &definition.variants {
                if matches!(
                    variant.name.as_str(),
                    "Self" | "this" | "print" | "move" | "box_int"
                ) || !enumerators.insert(variant.name.clone())
                {
                    return Err(Diagnostic::new(
                        variant.span,
                        format!(
                            "enum value '{}' is already defined or reserved",
                            variant.name
                        ),
                    ));
                }
            }
            enums.push(definition.clone());
        }
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
            enums,
            enum_names: program
                .enum_names
                .iter()
                .map(|entry| entry.0.clone())
                .collect(),
            arrays: program.arrays.clone(),
            array_owns: vec![false; program.arrays.len()],
            structs,
            order: Vec::new(),
        };
        let mut states = vec![0; types.structs.len()];
        let mut array_states = vec![0; types.arrays.len()];
        for id in 0..states.len() {
            types.visit(
                Type::Struct(id),
                &mut states,
                &mut array_states,
                program.struct_names[id].1,
            )?;
        }
        for id in 0..array_states.len() {
            types.visit(
                Type::Array(id),
                &mut states,
                &mut array_states,
                types.arrays[id].span,
            )?;
        }
        return Ok(types);
    }

    fn visit(
        &mut self,
        ty: Type,
        states: &mut [u8],
        array_states: &mut [u8],
        span: Span,
    ) -> Result<bool, Diagnostic> {
        let state = match ty {
            Type::Array(id) => array_states[id],
            Type::Struct(id) => states[id],
            _ => return Ok(self.owns(ty)),
        };
        if state == 2 {
            return Ok(self.owns(ty));
        }
        if state == 1 {
            return Err(Diagnostic::new(
                span,
                format!("recursive by-value layout involving {}", self.describe(ty)),
            ));
        }
        if let Type::Array(id) = ty {
            array_states[id] = 1;
            let array = self.arrays[id];
            self.array_owns[id] = self.visit(array.element, states, array_states, array.span)?;
            array_states[id] = 2;
            self.order.push(ty);
            return Ok(self.array_owns[id]);
        }
        let Type::Struct(id) = ty else { unreachable!() };
        states[id] = 1;
        let mut owns = self.structs[id].has_destructor;
        for field in self.structs[id].fields.clone() {
            owns |= self.visit(field.ty, states, array_states, field.span)?;
        }
        self.structs[id].owns = owns;
        states[id] = 2;
        self.order.push(ty);
        return Ok(owns);
    }

    pub fn owns(&self, ty: Type) -> bool {
        return match ty {
            Type::BoxInt => true,
            Type::Struct(id) => self.structs[id].owns,
            Type::Array(id) => self.array_owns[id],
            _ => false,
        };
    }

    pub fn byte_array_length(&self, ty: Type) -> Option<usize> {
        let id = match ty {
            Type::Array(id)
            | Type::Borrow {
                pointee: crate::ast::Pointee::Array(id),
                ..
            } => id,
            _ => return None,
        };
        let array = self.arrays[id];
        return if array.element == Type::Integer(IntType::U8) {
            Some(array.length)
        } else {
            None
        };
    }

    pub fn enumerator(&self, name: &str) -> Option<(usize, &EnumVariant)> {
        return self.enums.iter().find_map(|definition| {
            definition
                .variants
                .iter()
                .find(|variant| variant.name == name)
                .map(|variant| (definition.id, variant))
        });
    }

    pub fn integer_representation(&self, ty: Type) -> Option<IntType> {
        return match ty {
            Type::Enum(id) => Some(self.enums[id].underlying),
            _ => ty.integer(),
        };
    }

    pub fn describe(&self, ty: Type) -> String {
        return match ty {
            Type::Integer(IntType::I32) => String::from("int"),
            Type::Integer(kind) => kind.c_name().to_owned(),
            Type::Bool => String::from("bool"),
            Type::Void => String::from("void"),
            Type::BoxInt => String::from("BoxInt"),
            Type::Struct(id) => format!("struct {}", self.structs[id].name),
            Type::Enum(id) => format!("enum {}", self.enum_names[id]),
            Type::Array(id) => format!(
                "{}[{}]",
                self.describe(self.arrays[id].element),
                self.arrays[id].length
            ),
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
