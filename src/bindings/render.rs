use super::*;

#[derive(Clone)]
pub(super) struct Signature {
    pub result: String,
    pub convention: String,
    pub parameters: String,
    pub alias: String,
}

pub(super) struct Renderer {
    declarations: Vec<(Option<String>, String)>,
    callbacks: HashSet<String>,
    functions: HashMap<String, Signature>,
    records: HashSet<String>,
    enums: HashMap<String, String>,
    names: HashSet<String>,
    next_alias: usize,
}

impl Renderer {
    pub fn new(declarations: &[Value], probes: &[Value], functions: &[String]) -> Self {
        let mut names: HashSet<_> = functions.iter().cloned().collect();
        for declaration in declarations {
            if let Some(name) = declaration["name"].as_str() {
                names.insert(name.to_owned());
            }
        }
        for probe in probes {
            Self::reserve_type_names(probe, &mut names);
        }
        return Self {
            declarations: Vec::new(),
            callbacks: HashSet::new(),
            functions: HashMap::new(),
            records: HashSet::new(),
            enums: HashMap::new(),
            names,
            next_alias: 0,
        };
    }

    fn reserve_type_names(node: &Value, names: &mut HashSet<String>) {
        if let Some(name) = node["decl"]["name"].as_str() {
            names.insert(name.to_owned());
        }
        for child in type_children(node) {
            Self::reserve_type_names(child, names);
        }
    }

    pub fn binding_name(&mut self, native: &str) -> String {
        if crust_identifier(native) {
            return native.to_owned();
        }
        let mut name = format!("c_{native}");
        while !crust_identifier(&name) || !self.names.insert(name.clone()) {
            name.push('_');
        }
        return name;
    }

    pub fn typedef_name(&mut self, native: &str) -> String {
        let name = self.binding_name(native);
        if !self.enums.values().any(|tag| tag == &name) {
            return name;
        }
        let mut alias = format!("c_{name}");
        while !self.names.insert(alias.clone()) {
            alias.push('_');
        }
        return alias;
    }

    pub fn alias_type(&mut self, node: &Value) -> Result<(String, bool), String> {
        let qualified = Self::unwrap(node)?;
        if kind(qualified) == "QualType" {
            return Err(format!(
                "top-level typedef qualifiers '{}' require importing the C function or variable directly",
                qualified["qualifiers"].as_str().unwrap_or("unknown")
            ));
        }
        let underlying = Self::unqualified(node)?;
        let sized = !(matches!(
            kind(underlying),
            "RecordType" | "FunctionProtoType" | "FunctionNoProtoType"
        ) || (kind(underlying) == "BuiltinType"
            && underlying["type"]["qualType"] == "void"));
        return Ok((self.ty(node, true)?, sized));
    }

    pub fn enums(&mut self, definitions: Vec<enums::Enum>) -> Result<(), String> {
        for definition in &definitions {
            self.names.insert(definition.name.clone());
            self.names
                .extend(definition.values.iter().map(|(name, _)| name.clone()));
        }
        for definition in definitions {
            let name = self.binding_name(&definition.name);
            let native_name = if name != definition.name {
                format!(" = \"{}\"", definition.name)
            } else {
                String::new()
            };
            let mut declaration = format!(
                "extern enum {name}{native_name} : {} {{\n",
                definition.underlying.c_name()
            );
            for (native, value) in definition.values {
                let member = self.binding_name(&native);
                declaration.push_str(&format!("    {member} = {value},\n"));
            }
            declaration.push_str("};");
            self.enums.insert(definition.name, name);
            self.declarations.push((None, declaration));
        }
        return Ok(());
    }

    pub fn check_name(&mut self, index: usize) -> String {
        let mut name = format!("__crust_import_check_{index}");
        while !self.names.insert(name.clone()) {
            name.push('_');
        }
        return name;
    }

    pub fn declarations(&self, public: bool) -> String {
        return self
            .declarations
            .iter()
            .filter(|(alias, _)| {
                !public
                    || alias
                        .as_ref()
                        .is_none_or(|alias| self.callbacks.contains(alias))
            })
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
    }

    fn unwrap(mut node: &Value) -> Result<&Value, String> {
        while matches!(
            kind(node),
            "TypeOfExprType"
                | "TypeOfType"
                | "TypedefType"
                | "ParenType"
                | "ElaboratedType"
                | "AttributedType"
                | "MacroQualifiedType"
                | "AdjustedType"
                | "DecayedType"
                | "PredefinedSugarType"
        ) {
            node = type_children(node)
                .next_back()
                .ok_or("Clang omitted a nested C type")?;
        }
        return Ok(node);
    }

    pub fn signature(&mut self, node: &Value) -> Result<Signature, String> {
        let node = Self::unwrap(node)?;
        if kind(node) != "FunctionProtoType" {
            return Err("a complete C function prototype is required".to_owned());
        }
        let id = node["id"]
            .as_str()
            .ok_or("Clang omitted a function type identity")?;
        if let Some(signature) = self.functions.get(id) {
            return Ok(signature.clone());
        }
        let alias = loop {
            let name = format!("CrustCFunction{}", self.next_alias);
            self.next_alias += 1;
            if self.names.insert(name.clone()) {
                break name;
            }
        };
        let convention = match node["cc"].as_str().unwrap_or("cdecl") {
            "cdecl" => "",
            "stdcall" => "__stdcall ",
            "fastcall" => "__fastcall ",
            "vectorcall" => "__vectorcall ",
            convention => return Err(format!("unsupported C calling convention '{convention}'")),
        }
        .to_owned();
        let mut types = type_children(node);
        let result = self.value_type(types.next().ok_or("Clang omitted a function result")?)?;
        let mut parameters = Vec::new();
        for parameter in types {
            parameters.push(self.value_type(parameter)?);
        }
        if node["variadic"] == true {
            if parameters.is_empty() {
                return Err("variadic C bindings require a fixed parameter".to_owned());
            }
            parameters.push("...".to_owned());
        }
        let parameters = parameters.join(", ");
        self.declarations.push((
            Some(alias.clone()),
            format!("typedef {result} {convention}{alias}({parameters});"),
        ));
        let signature = Signature {
            result,
            convention,
            parameters,
            alias,
        };
        self.functions.insert(id.to_owned(), signature.clone());
        return Ok(signature);
    }

    fn unqualified(node: &Value) -> Result<&Value, String> {
        let mut node = Self::unwrap(node)?;
        while kind(node) == "QualType" {
            node = Self::unwrap(
                type_children(node)
                    .next()
                    .ok_or("Clang omitted a qualified value type")?,
            )?;
        }
        return Ok(node);
    }

    fn value_type(&mut self, node: &Value) -> Result<String, String> {
        return self.ty(Self::unqualified(node)?, false);
    }

    pub fn object_type(&mut self, node: &Value) -> Result<String, String> {
        return self.ty(node, false);
    }

    fn ty(&mut self, node: &Value, opaque: bool) -> Result<String, String> {
        let node = Self::unwrap(node)?;
        match kind(node) {
            "BuiltinType" => {
                let name = node["type"]["qualType"]
                    .as_str()
                    .ok_or("Clang omitted a builtin type name")?;
                return match name {
                    "_Bool" => Ok("bool".to_owned()),
                    "void" | "char" | "signed char" | "unsigned char" | "short"
                    | "unsigned short" | "int" | "unsigned int" | "long" | "unsigned long"
                    | "long long" | "unsigned long long" => Ok(name.to_owned()),
                    _ => Err(format!("unsupported C scalar type '{name}'")),
                };
            }
            "PointerType" => {
                let pointee = type_children(node)
                    .next()
                    .ok_or("Clang omitted a pointer target")?;
                return Ok(format!("{}*", self.ty(pointee, true)?));
            }
            "QualType" => {
                let qualifiers = node["qualifiers"].as_str().unwrap_or("");
                if qualifiers
                    .split_whitespace()
                    .any(|qualifier| !matches!(qualifier, "const" | "volatile"))
                {
                    return Err(format!("unsupported C pointee qualifiers '{qualifiers}'"));
                }
                let inner = type_children(node)
                    .next()
                    .ok_or("Clang omitted a qualified type")?;
                let value = self.ty(inner, opaque)?;
                return Ok(if kind(Self::unqualified(inner)?) == "PointerType" {
                    format!("{value} {qualifiers}")
                } else {
                    format!("{qualifiers} {value}")
                });
            }
            "EnumType" => {
                let name = node["decl"]["name"].as_str().unwrap_or("");
                return self
                    .enums
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("missing representation for C enum '{name}'"));
            }
            "RecordType" => {
                if !opaque {
                    return Err("C aggregates passed by value require a native wrapper".to_owned());
                }
                let name = node["decl"]["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or("anonymous C aggregates require a native wrapper")?;
                if !crust_identifier(name) {
                    return Err(format!(
                        "C tag '{name}' cannot be represented as a Crust identifier"
                    ));
                }
                let tag = if node["type"]["qualType"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("union "))
                {
                    "union"
                } else {
                    "struct"
                };
                let declaration = format!("{tag} {name}");
                if self.records.insert(declaration.clone()) {
                    self.declarations.push((None, format!("{declaration};")));
                }
                return Ok(declaration);
            }
            "FunctionProtoType" => {
                let alias = self.signature(node)?.alias;
                self.callbacks.insert(alias.clone());
                return Ok(alias);
            }
            _ => {
                return Err(format!(
                    "unsupported C type '{}' ({})",
                    node["type"]["qualType"].as_str().unwrap_or("unknown"),
                    kind(node)
                ));
            }
        }
    }
}
