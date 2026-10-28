use std::collections::{HashMap, HashSet};
mod borrows;

use crate::ast::{
    ArrayType, CallingConvention, EnumDefinition, EnumVariant, Field, FunctionType, IntType,
    PointerType, Program, Qualifiers, Type,
};
use crate::diagnostic::{Diagnostic, Span};

pub struct StructInfo {
    pub tagged: bool,
    pub is_union: bool,
    pub complete: bool,
    pub name: String,
    pub native_name: Option<String>,
    pub fields: Vec<Field>,
    pub owns: bool,
    pub has_destructor: bool,
}

pub struct Types {
    pub function_types: Vec<FunctionType>,
    pub pointers: Vec<PointerType>,
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
            let mut local_names = HashSet::new();
            for variant in &definition.variants {
                if !local_names.insert(variant.name.clone())
                    || (!definition.scoped
                        && (matches!(
                            variant.name.as_str(),
                            "Self" | "this" | "print" | "move" | "box_int" | "slice"
                        ) || !enumerators.insert(variant.name.clone())))
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
        let mut native_kinds = HashMap::new();
        for definition in &program.enums {
            if let Some(name) = &definition.native_name {
                native_kinds.insert(name.clone(), "enum");
            }
        }
        for (id, (name, span)) in program.struct_names.iter().enumerate() {
            let Some(definition) = definitions[id] else {
                let native = program.native_tags.get(&id).unwrap_or(name);
                let kind = if program.union_tags.contains(&id) {
                    "union"
                } else {
                    "struct"
                };
                if let Some(previous) = native_kinds.insert(native.clone(), kind)
                    && previous != kind
                {
                    return Err(Diagnostic::new(
                        *span,
                        format!("native tag '{native}' is already declared as {previous}"),
                    ));
                }
                structs.push(StructInfo {
                    tagged: false,
                    is_union: program.union_tags.contains(&id),
                    name: name.clone(),
                    native_name: program.native_tags.get(&id).cloned(),
                    complete: false,
                    fields: Vec::new(),
                    owns: false,
                    has_destructor: false,
                });
                continue;
            };
            if program.native_tags.contains_key(&id) {
                return Err(Diagnostic::new(
                    definition.span,
                    "a native tag alias must remain opaque",
                ));
            }
            if definition.fields.is_empty() {
                return Err(Diagnostic::new(
                    definition.span,
                    if program.union_tags.contains(&id) {
                        "a union needs at least one field"
                    } else {
                        "a struct needs at least one field"
                    },
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
                tagged: definition.tagged,
                is_union: program.union_tags.contains(&id) && !definition.tagged,
                complete: true,
                name: name.clone(),
                native_name: None,
                fields: definition.fields.clone(),
                owns: false,
                has_destructor: definition.has_destructor,
            });
        }

        let mut types = Types {
            function_types: program.function_types.clone(),
            pointers: program.pointers.clone(),
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
            if !types.is_sized(types.arrays[id].element) {
                return Err(Diagnostic::new(
                    types.arrays[id].span,
                    "array elements require a complete object type",
                ));
            }
            types.visit(
                Type::Array(id),
                &mut states,
                &mut array_states,
                types.arrays[id].span,
            )?;
        }
        for function_type in &types.function_types {
            types.check_function_type(function_type)?;
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
            let owns = self.visit(array.element, states, array_states, array.span)?;
            self.array_owns[id] = array.length != 0 && owns;
            array_states[id] = 2;
            self.order.push(ty);
            return Ok(self.array_owns[id]);
        }
        let Type::Struct(id) = ty else { unreachable!() };
        states[id] = 1;
        let mut owns = self.structs[id].has_destructor || self.contains_mutable_borrows(ty);
        for field in self.structs[id].fields.clone() {
            if !self.is_sized(field.ty) {
                return Err(Diagnostic::new(
                    field.span,
                    "struct fields require a complete object type",
                ));
            }
            let field_owns = self.visit(field.ty, states, array_states, field.span)?;
            if self.structs[id].is_union && field_owns {
                return Err(Diagnostic::new(
                    field.span,
                    "union fields require copyable values or raw pointers",
                ));
            }
            owns |= field_owns;
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

    pub fn slice(&mut self, element: Type, mutable: bool, span: Span) -> Type {
        let id = if let Some(id) = self
            .arrays
            .iter()
            .position(|array| array.element == element && array.length == 0)
        {
            id
        } else {
            let id = self.arrays.len();
            self.arrays.push(ArrayType {
                element,
                length: 0,
                span,
            });
            self.array_owns.push(false);
            self.order.push(Type::Array(id));
            id
        };
        return Type::Borrow {
            pointee: crate::ast::Pointee::Slice(id),
            mutable,
            volatile: false,
        };
    }

    pub fn is_sized(&self, ty: Type) -> bool {
        return match ty {
            Type::Void | Type::Function(_) => false,
            Type::Struct(id) => self.structs[id].complete,
            Type::Array(id) => {
                self.arrays[id].length != 0 && self.is_sized(self.arrays[id].element)
            }
            _ => true,
        };
    }

    fn check_function_type(&self, function_type: &FunctionType) -> Result<(), Diagnostic> {
        let valid = |ty| {
            self.is_sized(ty)
                && !self.owns(ty)
                && !self.carries_borrows(ty)
                && !matches!(ty, Type::Array(_))
        };
        if (function_type.result != Type::Void && !valid(function_type.result))
            || function_type.parameters.iter().any(|ty| !valid(*ty))
        {
            return Err(Diagnostic::new(
                function_type.span,
                "callbacks require copyable C values and raw pointers",
            ));
        }
        return Ok(());
    }

    pub fn function_type(
        &mut self,
        result: Type,
        parameters: Vec<Type>,
        variadic: bool,
        calling_convention: CallingConvention,
        span: Span,
    ) -> Result<Type, Diagnostic> {
        let function_type = FunctionType {
            calling_convention,
            variadic,
            result,
            parameters,
            span,
        };
        self.check_function_type(&function_type)?;
        let id = if let Some(id) = self.function_types.iter().position(|existing| {
            existing.result == result
                && existing.parameters == function_type.parameters
                && existing.variadic == variadic
                && existing.calling_convention == calling_convention
        }) {
            id
        } else {
            let id = self.function_types.len();
            self.function_types.push(function_type);
            id
        };
        return Ok(Type::Function(id));
    }

    pub fn callback(&self, ty: Type) -> Option<&FunctionType> {
        if let Type::Pointer(id) = ty
            && let Type::Function(id) = self.pointers[id].pointee
        {
            return Some(&self.function_types[id]);
        }
        return None;
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
        return if array.length != 0
            && matches!(array.element, Type::Integer(IntType::U8 | IntType::Char))
        {
            Some(array.length)
        } else {
            None
        };
    }

    pub fn is_byte_slice(&self, ty: Type) -> bool {
        return matches!(ty, Type::Borrow { pointee: crate::ast::Pointee::Slice(id), .. }
            if matches!(self.arrays[id].element, Type::Integer(IntType::U8 | IntType::Char)));
    }

    pub fn enumerator(&self, name: &str) -> Option<(usize, &EnumVariant)> {
        return self.enums.iter().find_map(|definition| {
            if definition.scoped {
                return None;
            }
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

    pub fn pointer_to(&mut self, pointee: Type, mutable: bool) -> Type {
        return self.qualified_pointer_to(pointee, mutable, false);
    }

    pub fn qualified_pointer_to(&mut self, pointee: Type, mutable: bool, volatile: bool) -> Type {
        let pointer = PointerType {
            pointee,
            mutable,
            volatile,
        };
        let id = if let Some(id) = self
            .pointers
            .iter()
            .position(|existing| *existing == pointer)
        {
            id
        } else {
            let id = self.pointers.len();
            self.pointers.push(pointer);
            id
        };
        return Type::Pointer(id);
    }

    pub fn describe(&self, ty: Type) -> String {
        return match ty {
            Type::Function(id) => {
                let function = &self.function_types[id];
                let mut parameters: Vec<_> = function
                    .parameters
                    .iter()
                    .map(|ty| self.describe(*ty))
                    .collect();
                if function.variadic {
                    parameters.push("...".to_owned());
                }
                format!(
                    "{} {}({})",
                    self.describe(function.result),
                    function.calling_convention.c_prefix(),
                    parameters.join(", ")
                )
            }
            Type::Integer(IntType::I32) => String::from("int"),
            Type::Integer(kind) => kind.c_name().to_owned(),
            Type::Float(kind) => kind.c_name().to_owned(),
            Type::Bool => String::from("bool"),
            Type::Void => String::from("void"),
            Type::BoxInt => String::from("BoxInt"),
            Type::Struct(id) => format!(
                "{} {}",
                if self.structs[id].tagged {
                    "tagged union"
                } else if self.structs[id].is_union {
                    "union"
                } else {
                    "struct"
                },
                self.structs[id].name
            ),
            Type::Enum(id) => format!("enum {}", self.enum_names[id]),
            Type::Pointer(id) => {
                let pointer = self.pointers[id];
                format!(
                    "{}*",
                    Qualifiers {
                        readonly: !pointer.mutable,
                        volatile: pointer.volatile
                    }
                    .describe(pointer.pointee, self.describe(pointer.pointee))
                )
            }
            Type::Array(id) if self.arrays[id].length == 0 => {
                format!("{}[]", self.describe(self.arrays[id].element))
            }
            Type::Array(id) => format!(
                "{}[{}]",
                self.describe(self.arrays[id].element),
                self.arrays[id].length
            ),
            Type::Borrow {
                pointee,
                mutable,
                volatile,
            } => format!(
                "{}&",
                Qualifiers {
                    readonly: !mutable,
                    volatile
                }
                .describe(pointee.ty(), self.describe(pointee.ty()))
            ),
        };
    }

    pub fn field(&self, ty: Type, name: &str, span: Span) -> Result<Type, Diagnostic> {
        if matches!(ty, Type::Array(id) if self.arrays[id].length == 0) && name == "length" {
            return Ok(Type::Integer(IntType::named("size_t").unwrap()));
        }
        if let Type::Struct(id) = ty {
            if !self.structs[id].complete {
                return Err(Diagnostic::new(
                    span,
                    "field access requires a complete struct type",
                ));
            }
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
