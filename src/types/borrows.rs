use super::*;

impl Types {
    pub fn carries_borrows(&self, ty: Type) -> bool {
        return ty.is_borrow()
            || (matches!(ty, Type::Struct(_) | Type::Array(_)) && self.contains_borrows(ty));
    }
    pub fn contains_borrows(&self, ty: Type) -> bool {
        return self.borrow_property(ty, false, &mut HashSet::new());
    }

    pub fn contains_mutable_borrows(&self, ty: Type) -> bool {
        return self.borrow_property(ty, true, &mut HashSet::new());
    }

    fn borrow_property(&self, ty: Type, mutable: bool, visited: &mut HashSet<Type>) -> bool {
        if !visited.insert(ty) {
            return false;
        }
        return match ty {
            Type::Borrow {
                mutable: exclusive, ..
            } => !mutable || exclusive,
            Type::Struct(id) => self.structs[id]
                .fields
                .iter()
                .any(|field| self.borrow_property(field.ty, mutable, visited)),
            Type::Array(id) => self.borrow_property(self.arrays[id].element, mutable, visited),
            // Generic owners store T behind a raw pointer. T's reference
            // lifetimes still belong to the concrete owner's type.
            Type::Pointer(id) => self.borrow_property(self.pointers[id].pointee, mutable, visited),
            _ => false,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_members_follow_nested_storage_and_stop_at_recursive_type_cycles() {
        let source = "struct Leaf { int value; }; struct Wrapper { Leaf* data; Wrapper* next; };";
        let program = crate::parser::parse(crate::lexer::tokenize(source).unwrap()).unwrap();
        let mut types = Types::build(&program).unwrap();
        let leaf = Type::Struct(0);
        let wrapper = Type::Struct(1);
        assert!(!types.contains_borrows(wrapper));
        types.structs[0].fields[0].ty = Type::Integer(IntType::I32)
            .borrowed_qualified(false, false)
            .unwrap();
        assert!(types.carries_borrows(leaf));
        assert!(types.carries_borrows(wrapper));
        assert!(!types.contains_mutable_borrows(wrapper));
        // A raw address remains a raw value. A containing aggregate retains
        // the lifetimes of its typed storage.
        assert!(!types.carries_borrows(Type::Pointer(0)));
        types.structs[0].fields[0].ty = Type::Integer(IntType::I32)
            .borrowed_qualified(true, false)
            .unwrap();
        assert!(types.contains_mutable_borrows(wrapper));
    }
}
