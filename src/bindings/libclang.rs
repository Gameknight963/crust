//! The stable libclang C API. No Clang executable or C++ ABI crosses this boundary.
#![allow(non_snake_case)]
use libloading::Library;
use serde_json::{Value, json};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_ulong, c_void};
use std::path::PathBuf;

#[repr(C)]
#[derive(Clone, Copy)]
struct Cursor {
    kind: c_uint,
    xdata: c_int,
    data: [*const c_void; 3],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Type {
    kind: c_uint,
    data: [*mut c_void; 2],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct StringValue {
    data: *const c_void,
    flags: c_uint,
}
#[repr(C)]
struct Unsaved {
    name: *const c_char,
    contents: *const c_char,
    length: c_ulong,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Location {
    data: [*const c_void; 2],
    int_data: c_uint,
}

macro_rules! api {
    ($($name:ident($($arg:ty),*) -> $result:ty;)+) => {
        struct Api { $($name: unsafe extern "C" fn($($arg),*) -> $result,)+ _library: Library, _dependencies:Vec<Library> }
        #[allow(non_snake_case)]
        impl Api {
            fn load() -> Result<Self, String> {
                let mut candidates=Vec::new();
                let explicit=std::env::var_os("CRUST_LIBCLANG");
                if let Some(path)=&explicit { candidates.push(PathBuf::from(path)); }
                if let Ok(executable)=std::env::current_exe() {
                    if let Some(directory)=executable.parent() {
                        for name in library_names() { candidates.push(directory.join(name)); candidates.push(directory.join("../lib").join(name)); }
                    }
                }
                for name in library_names() { candidates.push(PathBuf::from(name)); }
                if cfg!(windows) { candidates.push(PathBuf::from("C:/Program Files/LLVM/bin/libclang.dll")); }
                if cfg!(target_os="macos") {
                    if let Ok(output)=std::process::Command::new("xcrun").args(["--find","clang"]).output() {
                        let path=PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
                        if let Some(directory)=path.parent(){candidates.push(directory.join("../lib/libclang.dylib"));}
                    }
                }
                if let Ok(entries)=std::fs::read_dir("/usr/lib") {
                    for entry in entries.flatten() {
                        if entry.file_name().to_string_lossy().starts_with("llvm-") {
                            candidates.push(entry.path().join("lib/libclang.so.1"));
                            candidates.push(entry.path().join("lib/libclang.so"));
                        }
                    }
                }
                let mut errors=Vec::new();
                if explicit.is_some(){candidates.truncate(1);}
                for path in candidates {
                    let mut dependencies=Vec::new();
                    if let Some(directory)=path.parent() && let Ok(entries)=std::fs::read_dir(directory) {
                        for entry in entries.flatten(){
                            if entry.file_name().to_string_lossy().starts_with("libLLVM") {
                                if let Ok(library)=unsafe{Library::new(entry.path())}{dependencies.push(library);}
                            }
                        }
                    }
                    // The library stays loaded until every translation unit is disposed.
                    let library=match unsafe { Library::new(&path) } { Ok(value)=>value, Err(error)=>{errors.push(error.to_string());continue;} };
                    unsafe {
                        return Ok(Self {$($name: *library.get(concat!(stringify!($name), "\0").as_bytes()).map_err(|error|error.to_string())?,)+ _library:library,_dependencies:dependencies});
                    }
                }
                return Err(format!("could not load bundled libclang; set CRUST_LIBCLANG to the library path: {}", errors.last().map(String::as_str).unwrap_or("no library found")));
            }
        }
    }
}

fn library_names() -> &'static [&'static str] {
    if cfg!(windows) {
        return &["libclang.dll"];
    }
    if cfg!(target_os = "macos") {
        return &["libclang.dylib"];
    }
    return &["libclang.so", "libclang.so.1"];
}

api! {
    clang_createIndex(c_int,c_int) -> *mut c_void;
    clang_Cursor_isAnonymousRecordDecl(Cursor) -> c_uint;
    clang_getCursorTLSKind(Cursor) -> c_uint;
    clang_disposeIndex(*mut c_void) -> ();
    clang_parseTranslationUnit2(*mut c_void,*const c_char,*const *const c_char,c_int,*mut Unsaved,c_uint,c_uint,*mut *mut c_void) -> c_int;
    clang_disposeTranslationUnit(*mut c_void) -> ();
    clang_getTranslationUnitCursor(*mut c_void) -> Cursor;
    clang_visitChildren(Cursor,extern "C" fn(Cursor,Cursor,*mut c_void)->c_uint,*mut c_void) -> c_uint;
    clang_getCursorSpelling(Cursor) -> StringValue;
    clang_getCursorLocation(Cursor) -> Location;
    clang_getPresumedLocation(Location,*mut StringValue,*mut c_uint,*mut c_uint) -> ();
    clang_getTypeSpelling(Type) -> StringValue;
    clang_getCString(StringValue) -> *const c_char;
    clang_disposeString(StringValue) -> ();
    clang_getCursorType(Cursor) -> Type;
    clang_getCanonicalType(Type) -> Type;
    clang_getTypeDeclaration(Type) -> Cursor;
    clang_getPointeeType(Type) -> Type;
    clang_getArrayElementType(Type) -> Type;
    clang_getArraySize(Type) -> i64;
    clang_getResultType(Type) -> Type;
    clang_getNumArgTypes(Type) -> c_int;
    clang_getArgType(Type,c_uint) -> Type;
    clang_isFunctionTypeVariadic(Type) -> c_uint;
    clang_getFunctionTypeCallingConv(Type) -> c_uint;
    clang_isConstQualifiedType(Type) -> c_uint;
    clang_isVolatileQualifiedType(Type) -> c_uint;
    clang_isRestrictQualifiedType(Type) -> c_uint;
    clang_isCursorDefinition(Cursor) -> c_uint;
    clang_Cursor_isBitField(Cursor) -> c_uint;
    clang_getFieldDeclBitWidth(Cursor) -> c_int;
    clang_getEnumDeclIntegerType(Cursor) -> Type;
    clang_getEnumConstantDeclValue(Cursor) -> i64;
    clang_getEnumConstantDeclUnsignedValue(Cursor) -> u64;
    clang_getNumDiagnostics(*mut c_void) -> c_uint;
    clang_getDiagnostic(*mut c_void,c_uint) -> *mut c_void;
    clang_getDiagnosticSeverity(*mut c_void) -> c_uint;
    clang_formatDiagnostic(*mut c_void,c_uint) -> StringValue;
    clang_disposeDiagnostic(*mut c_void) -> ();
}

struct Unit {
    api: Api,
    index: *mut c_void,
    unit: *mut c_void,
}
impl Drop for Unit {
    fn drop(&mut self) {
        unsafe {
            (self.api.clang_disposeTranslationUnit)(self.unit);
            (self.api.clang_disposeIndex)(self.index);
        }
    }
}

impl Unit {
    fn location(&self, cursor: Cursor) -> (PathBuf, usize, usize) {
        let mut file = StringValue {
            data: std::ptr::null(),
            flags: 0,
        };
        let mut line = 0;
        let mut column = 0;
        unsafe {
            (self.api.clang_getPresumedLocation)(
                (self.api.clang_getCursorLocation)(cursor),
                &mut file,
                &mut line,
                &mut column,
            );
        }
        let path = PathBuf::from(self.string(file));
        return (
            crate::source::identity(&path).unwrap_or(path),
            line as usize,
            column as usize,
        );
    }
    fn parse(source: &str, target: Option<&str>) -> Result<Self, String> {
        let api = Api::load()?;
        let name = CString::new("crust-native.c").unwrap();
        let source = CString::new(source).map_err(|_| "native source contains a null byte")?;
        let mut arguments = vec![
            CString::new("-x").unwrap(),
            CString::new("cpp-output").unwrap(),
            CString::new("-std=c11").unwrap(),
        ];
        if let Some(target) = target {
            arguments.push(
                CString::new(format!("--target={target}")).map_err(|_| "invalid native target")?,
            );
        }
        let pointers: Vec<_> = arguments.iter().map(|s| s.as_ptr()).collect();
        let mut unsaved = Unsaved {
            name: name.as_ptr(),
            contents: source.as_ptr(),
            length: source.as_bytes().len() as c_ulong,
        };
        unsafe {
            let index = (api.clang_createIndex)(0, 0);
            let mut unit = std::ptr::null_mut();
            // C was preprocessed by CC. Skip bodies for declaration discovery.
            let error = (api.clang_parseTranslationUnit2)(
                index,
                name.as_ptr(),
                pointers.as_ptr(),
                pointers.len() as c_int,
                &mut unsaved,
                1,
                0x40,
                &mut unit,
            );
            if error != 0 || unit.is_null() {
                (api.clang_disposeIndex)(index);
                return Err(format!(
                    "libclang could not parse native declarations ({error})"
                ));
            }
            let result = Self { api, index, unit };
            for i in 0..(result.api.clang_getNumDiagnostics)(unit) {
                let diagnostic = (result.api.clang_getDiagnostic)(unit, i);
                let severity = (result.api.clang_getDiagnosticSeverity)(diagnostic);
                let message = result.string((result.api.clang_formatDiagnostic)(diagnostic, 3));
                (result.api.clang_disposeDiagnostic)(diagnostic);
                if severity >= 3 {
                    return Err(message);
                }
            }
            return Ok(result);
        }
    }
    fn string(&self, value: StringValue) -> String {
        unsafe {
            let pointer = (self.api.clang_getCString)(value);
            let text = if pointer.is_null() {
                String::new()
            } else {
                CStr::from_ptr(pointer).to_string_lossy().into_owned()
            };
            (self.api.clang_disposeString)(value);
            return text;
        }
    }
    fn name(&self, cursor: Cursor) -> String {
        return self.string(unsafe { (self.api.clang_getCursorSpelling)(cursor) });
    }
    fn children(&self, cursor: Cursor) -> Vec<Cursor> {
        extern "C" fn visit(cursor: Cursor, _parent: Cursor, data: *mut c_void) -> c_uint {
            unsafe { &mut *data.cast::<Vec<Cursor>>() }.push(cursor);
            return 1;
        }
        let mut output = Vec::new();
        unsafe {
            (self.api.clang_visitChildren)(cursor, visit, (&mut output as *mut Vec<Cursor>).cast());
        }
        return output;
    }
    fn fields(
        &self,
        cursor: Cursor,
        groups: &[super::records::Group],
        next: &mut usize,
        output: &mut Vec<(Cursor, Vec<super::records::Group>)>,
    ) -> Result<(), String> {
        if groups.len() >= 128 {
            return Err("anonymous C groups exceed the nesting limit".to_owned());
        }
        for child in self.children(cursor) {
            if matches!(child.kind, 2 | 3)
                && unsafe { (self.api.clang_Cursor_isAnonymousRecordDecl)(child) } != 0
            {
                let mut nested = groups.to_vec();
                nested.push(super::records::Group {
                    id: *next,
                    tag: if child.kind == 3 { "union" } else { "struct" },
                });
                *next += 1;
                self.fields(child, &nested, next, output)?;
            } else if child.kind == 6 && !self.name(child).is_empty() {
                output.push((child, groups.to_vec()));
            }
        }
        return Ok(());
    }

    fn ty(&self, ty: Type, depth: usize) -> Result<Value, String> {
        if depth > 128 {
            return Err("native declaration type nesting exceeds 128".to_owned());
        }
        let ty = unsafe { (self.api.clang_getCanonicalType)(ty) };
        let spelling = self.string(unsafe { (self.api.clang_getTypeSpelling)(ty) });
        let mut value = unsafe {
            match ty.kind {
                2..=23 => {
                    json!({"kind":"BuiltinType","type":{"qualType":spelling.trim_start_matches("const ").trim_start_matches("volatile ")}})
                }
                101 => {
                    json!({"kind":"PointerType","inner":[self.ty((self.api.clang_getPointeeType)(ty),depth+1)?]})
                }
                105 | 106 => {
                    let cursor = (self.api.clang_getTypeDeclaration)(ty);
                    json!({"kind":if ty.kind==105{"RecordType"}else{"EnumType"},"decl":{"name":self.name(cursor)},"type":{"qualType":if ty.kind==105{format!("{} {}",if cursor.kind==3{"union"}else{"struct"},self.name(cursor))}else{spelling}}})
                }
                110 | 111 => {
                    let mut inner = vec![self.ty((self.api.clang_getResultType)(ty), depth + 1)?];
                    for i in 0..(self.api.clang_getNumArgTypes)(ty).max(0) {
                        inner.push(
                            self.ty((self.api.clang_getArgType)(ty, i as c_uint), depth + 1)?,
                        );
                    }
                    let convention = match (self.api.clang_getFunctionTypeCallingConv)(ty) {
                        1 => "cdecl",
                        2 => "stdcall",
                        3 => "fastcall",
                        12 => "vectorcall",
                        other => {
                            return Err(format!("unsupported native calling convention {other}"));
                        }
                    };
                    json!({"kind":if ty.kind==111{"FunctionProtoType"}else{"FunctionNoProtoType"},"id":spelling,"cc":convention,"variadic":(self.api.clang_isFunctionTypeVariadic)(ty)!=0,"inner":inner})
                }
                112 | 114 | 115 => {
                    json!({"kind":match ty.kind{112=>"ConstantArrayType",114=>"IncompleteArrayType",_=>"VariableArrayType"},"size":(self.api.clang_getArraySize)(ty),"inner":[self.ty((self.api.clang_getArrayElementType)(ty),depth+1)?]})
                }
                _ => {
                    return Err(format!(
                        "unsupported native C type '{spelling}' (libclang kind {})",
                        ty.kind
                    ));
                }
            }
        };
        let mut qualifiers = Vec::new();
        unsafe {
            if (self.api.clang_isConstQualifiedType)(ty) != 0 {
                qualifiers.push("const");
            }
            if (self.api.clang_isVolatileQualifiedType)(ty) != 0 {
                qualifiers.push("volatile");
            }
            if (self.api.clang_isRestrictQualifiedType)(ty) != 0 {
                qualifiers.push("restrict");
            }
        }
        if !qualifiers.is_empty() {
            value = json!({"kind":"QualType","qualifiers":qualifiers.join(" "),"inner":[value]});
        }
        return Ok(value);
    }
}

/// Discover the declarations referenced by Crust, using the stable C API.
pub(crate) struct Declarations {
    pub text: String,
    pub locations: std::collections::HashMap<String, (PathBuf, usize, usize)>,
}
pub(crate) fn declarations(
    source: &str,
    target: Option<&str>,
    names: &std::collections::HashSet<String>,
) -> Result<Declarations, String> {
    let unit = Unit::parse(source, target)?;
    let cursors = unit.children(unsafe { (unit.api.clang_getTranslationUnitCursor)(unit.unit) });
    let mut probes = Vec::new();
    let mut records = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut variables = Vec::new();
    let mut aliases = Vec::new();
    let mut wanted = names.clone();
    for cursor in &cursors {
        let name = unit.name(*cursor);
        if names.contains(&format!("c_{name}")) {
            wanted.insert(name);
        }
    }

    fn dependencies(node: &Value, wanted: &mut std::collections::HashSet<String>) {
        if let Some(name) = node["decl"]["name"].as_str() {
            wanted.insert(name.to_owned());
        }
        for child in super::children(node) {
            dependencies(child, wanted);
        }
    }
    loop {
        let previous = wanted.len();
        for cursor in &cursors {
            let name = unit.name(*cursor);
            if cursor.kind == 5
                && unit
                    .children(*cursor)
                    .iter()
                    .any(|c| wanted.contains(&unit.name(*c)))
            {
                wanted.insert(name.clone());
            }
            if !wanted.contains(&name) {
                continue;
            }
            if matches!(cursor.kind, 2 | 3) {
                let mut fields = Vec::new();
                unit.fields(*cursor, &[], &mut 0, &mut fields)?;
                for (child, _) in fields {
                    dependencies(
                        &unit.ty(unsafe { (unit.api.clang_getCursorType)(child) }, 0)?,
                        &mut wanted,
                    );
                }
            } else if matches!(cursor.kind, 8 | 9 | 20) {
                dependencies(
                    &unit.ty(unsafe { (unit.api.clang_getCursorType)(*cursor) }, 0)?,
                    &mut wanted,
                );
            }
        }
        if previous == wanted.len() {
            break;
        }
    }
    let mut locations = std::collections::HashMap::new();
    let mut dll_imports = std::collections::HashSet::new();
    for cursor in &cursors {
        let name = unit.name(*cursor);
        if name.is_empty() || !wanted.contains(&name) {
            continue;
        }
        if cursor.kind == 9 && unsafe { (unit.api.clang_getCursorTLSKind)(*cursor) } != 0 {
            return Err(format!(
                "C variable '{name}' requires a native thread-local wrapper"
            ));
        }
        if unit.children(*cursor).iter().any(|child| child.kind == 418) {
            return Err(format!(
                "C declaration '{name}' is declared for DLL export. Use the header's consumer configuration"
            ));
        }
        if unit.children(*cursor).iter().any(|child| child.kind == 419) {
            dll_imports.insert(name.clone());
        }
        locations
            .entry(name.clone())
            .or_insert_with(|| unit.location(*cursor));
        match cursor.kind {
            8 => functions.push((
                name,
                unit.ty(unsafe { (unit.api.clang_getCursorType)(*cursor) }, 0)?,
            )),
            9 => variables.push((
                name,
                unit.ty(unsafe { (unit.api.clang_getCursorType)(*cursor) }, 0)?,
            )),
            20 => {
                if crate::ast::IntType::named(&name).is_none() {
                    aliases.push((
                        name,
                        unit.ty(unsafe { (unit.api.clang_getCursorType)(*cursor) }, 0)?,
                    ));
                }
            }
            2 | 3 if unsafe { (unit.api.clang_isCursorDefinition)(*cursor) } != 0 => {
                records.push((*cursor, name))
            }
            5 if unsafe { (unit.api.clang_isCursorDefinition)(*cursor) } != 0 => {
                enums.push((*cursor, name))
            }
            _ => {}
        }
    }
    let function_names: Vec<_> = functions.iter().map(|(name, _)| name.clone()).collect();
    let mut renderer = super::render::Renderer::new(&[], &[], &function_names);
    let mut enum_definitions = Vec::new();
    for (cursor, name) in enums {
        let ty = unsafe { (unit.api.clang_getEnumDeclIntegerType)(cursor) };
        let underlying = match ty.kind {
            4 | 13 => crate::ast::IntType::Char,
            5 => crate::ast::IntType::U8,
            14 => crate::ast::IntType::I8,
            8 => crate::ast::IntType::U16,
            16 => crate::ast::IntType::I16,
            9 => crate::ast::IntType::U32,
            17 => crate::ast::IntType::I32,
            10 => crate::ast::IntType::ULong,
            18 => crate::ast::IntType::Long,
            11 => crate::ast::IntType::ULongLong,
            19 => crate::ast::IntType::LongLong,
            _ => return Err("unsupported native enum representation".to_owned()),
        };
        let values = unit
            .children(cursor)
            .into_iter()
            .filter(|c| c.kind == 7)
            .map(|c| {
                (unit.name(c), unsafe {
                    if underlying.signed() {
                        i128::from((unit.api.clang_getEnumConstantDeclValue)(c))
                    } else {
                        i128::from((unit.api.clang_getEnumConstantDeclUnsignedValue)(c))
                    }
                })
            })
            .collect();
        enum_definitions.push(super::enums::Enum {
            name,
            underlying,
            values,
        });
    }
    renderer.enums(enum_definitions)?;
    let mut definitions = Vec::new();
    for (cursor, name) in records {
        let mut fields = Vec::new();
        let mut exposed = Vec::new();
        unit.fields(cursor, &[], &mut 0, &mut exposed)?;
        for (field, groups) in exposed {
            let field_name = unit.name(field);
            locations
                .entry(field_name.clone())
                .or_insert_with(|| unit.location(field));
            let probe = format!("crust_native_field_{}", probes.len());
            probes.push(json!({"name":probe,"inner":[unit.ty(unsafe{(unit.api.clang_getCursorType)(field)},0)?]}));
            fields.push(super::records::RecordField {
                name: field_name,
                probe,
                bit_width: if unsafe { (unit.api.clang_Cursor_isBitField)(field) } != 0 {
                    Some(unsafe { (unit.api.clang_getFieldDeclBitWidth)(field) } as u32)
                } else {
                    None
                },
                groups,
            });
        }
        definitions.push(super::records::Record {
            name,
            tag: if cursor.kind == 3 { "union" } else { "struct" },
            fields,
        });
    }
    renderer.complete_records(definitions, &probes)?;
    let mut output = String::new();
    let mut seen = std::collections::HashSet::new();
    for (name, ty) in functions {
        if !seen.insert(name.clone()) {
            continue;
        }
        let signature = renderer.signature(&ty)?;
        let binding = renderer.binding_name(&name);
        let alias = if name == binding {
            String::new()
        } else {
            format!(" = \"{name}\"")
        };
        output.push_str(&format!(
            "extern {}{} {}{}({}){alias};\n",
            if dll_imports.contains(&name) {
                "__declspec(dllimport) "
            } else {
                ""
            },
            signature.result,
            signature.convention,
            binding,
            signature.parameters
        ));
    }
    for (name, ty) in variables {
        if !seen.insert(name.clone()) {
            continue;
        }
        let binding = renderer.binding_name(&name);
        let alias = if name == binding {
            String::new()
        } else {
            format!(" = \"{name}\"")
        };
        output.push_str(&format!(
            "extern {}{} {binding}{alias};\n",
            if dll_imports.contains(&name) {
                "__declspec(dllimport) "
            } else {
                ""
            },
            renderer.object_type(&ty)?
        ));
    }
    for (name, ty) in aliases {
        let binding = renderer.typedef_name(&name);
        output.push_str(&format!(
            "typedef {} {binding};\n",
            renderer.alias_type(&ty)?.0
        ));
    }
    return Ok(Declarations {
        text: format!("{}\n{output}", renderer.declarations(true)),
        locations,
    });
}
