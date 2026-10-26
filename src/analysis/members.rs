use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberAccess {
    Value,
    Dereference,
    Type,
}

#[derive(Debug)]
struct Member {
    declaration: Location,
    is_static: bool,
    private_owner: Option<usize>,
}

#[derive(Debug, Default)]
pub(super) struct Index {
    types: HashMap<Location, HashSet<Type>>,
    pointees: HashMap<Type, Type>,
    members: HashMap<Type, Vec<Member>>,
    owners: Vec<(Location, usize)>,
}

impl Index {
    pub fn build(checked: &AnalyzedProgram, locate: &impl Fn(Span) -> Location) -> Self {
        let mut index = Self::default();
        for (span, ty) in &checked.program.type_annotations {
            index.types.entry(locate(*span)).or_default().insert(*ty);
        }
        for analysis in checked.analyses.values() {
            let mut types = analysis.expression_types.clone();
            types.extend(&analysis.receiver_types);
            for (span, ty) in types {
                index.types.entry(locate(span)).or_default().insert(ty);
            }
        }
        for (id, pointer) in checked.types.pointers.iter().enumerate() {
            index.pointees.insert(Type::Pointer(id), pointer.pointee);
        }
        for (id, (_, span)) in checked.program.struct_names.iter().enumerate() {
            index
                .types
                .entry(locate(*span))
                .or_default()
                .insert(Type::Struct(id));
        }
        for definition in &checked.program.structs {
            index
                .members
                .entry(Type::Struct(definition.id))
                .or_default()
                .extend(definition.fields.iter().map(|field| Member {
                    declaration: locate(field.span),
                    is_static: false,
                    private_owner: field.private.then_some(definition.id),
                }));
        }
        for definition in &checked.program.enums {
            index
                .types
                .entry(locate(definition.name_span))
                .or_default()
                .insert(Type::Enum(definition.id));
            index.members.insert(
                Type::Enum(definition.id),
                definition
                    .variants
                    .iter()
                    .map(|variant| Member {
                        declaration: locate(variant.span),
                        is_static: true,
                        private_owner: None,
                    })
                    .collect(),
            );
        }
        for function in &checked.program.functions {
            let method = function
                .name
                .strip_prefix('@')
                .or_else(|| function.name.strip_prefix('#'));
            let owner = method
                .and_then(|method| method.split_once('_'))
                .and_then(|(id, _)| id.parse().ok());
            if let Some(id) = owner {
                index
                    .members
                    .entry(Type::Struct(id))
                    .or_default()
                    .push(Member {
                        declaration: locate(function.name_span),
                        is_static: function.name.starts_with('#'),
                        private_owner: None,
                    });
            }
            let owner = owner.or_else(|| {
                function
                    .name
                    .strip_prefix('~')
                    .and_then(|id| id.parse().ok())
            });
            if let Some(id) = owner {
                index.owners.push((locate(function.span), id));
            }
        }
        return index;
    }
}

impl Analysis {
    pub fn member_completions_at(
        &self,
        file: usize,
        offset: usize,
        access: MemberAccess,
    ) -> Vec<&Symbol> {
        let Some((_, types)) = self
            .members
            .types
            .iter()
            .filter(|(location, _)| contains(**location, file, offset, false))
            .min_by_key(|(location, _)| location.span.end - location.span.start)
        else {
            return Vec::new();
        };
        let mut declarations: Option<HashSet<Location>> = None;
        for ty in types {
            let mut ty = *ty;
            if access == MemberAccess::Dereference {
                ty = match ty {
                    Type::Borrow { pointee, .. } => pointee.ty(),
                    Type::Pointer(_) => match self.members.pointees.get(&ty) {
                        Some(ty) => *ty,
                        None => return Vec::new(),
                    },
                    // Arrow receivers are lowered to a dereference with the
                    // same span. Their recorded type already names the struct.
                    _ => ty,
                };
            }
            let candidates: HashSet<_> = self
                .members
                .members
                .get(&ty)
                .into_iter()
                .flatten()
                .filter(|member| {
                    return member.is_static == (access == MemberAccess::Type)
                        && member.private_owner.is_none_or(|id| {
                            self.members.owners.iter().any(|(range, owner)| {
                                *owner == id && contains(*range, file, offset, false)
                            })
                        });
                })
                .map(|member| member.declaration)
                .collect();
            // Replayed generic bodies offer members shared by every concrete
            // receiver at this source position.
            if let Some(declarations) = &mut declarations {
                declarations.retain(|declaration| candidates.contains(declaration));
            } else {
                declarations = Some(candidates);
            }
        }
        let declarations = declarations.unwrap_or_default();
        let mut seen = HashSet::new();
        let mut symbols: Vec<_> = self
            .symbols
            .iter()
            .filter(|symbol| {
                declarations.contains(&symbol.location) && seen.insert(symbol.location)
            })
            .collect();
        symbols.sort_by(|a, b| a.name.cmp(&b.name));
        return symbols;
    }
}
