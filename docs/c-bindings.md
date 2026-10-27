# C header bindings

`crust import-c` reads a C header through Clang and writes declarations for selected
functions, external variables, named enums, and typedefs. Ordinary Crust source imports the
generated file.

```powershell
cargo run -- import-c examples/header_project/native.h `
  --function native_create --function native_value `
  --function native_status --function native_add --function native_destroy `
  --variable native_limit `
  --output examples/header_project/bindings.cr --force
cargo run -- run examples/header_project
```

The [example](../examples/header_project/) wraps an opaque C handle in a move-only
Crust struct with a destructor.

## Options

| Option | Purpose |
| --- | --- |
| `--function <name>` | Select a C function, repeated for each export |
| `--variable <name>` | Select external C storage, repeated for each variable |
| `--enum <tag>` | Select a named C enum, repeated for each tag |
| `--typedef <name>` | Select a C typedef, repeated for each alias |
| `--output <file.cr>` | Write UTF-8 bindings to a file |
| `--force` | Replace an existing output file |
| `--include-dir <path>` | Add a C header search directory |
| `--define <name=value>` | Set a C preprocessor definition |
| `--clang <path>` | Select the Clang executable |

Clang must be available when generating bindings. Without `--output`, the command
writes to stdout. Generation finishes before the output file is written, and the
command protects its input header from replacement.

## Imported declarations

- Native C integers, booleans, and `void`
- Raw pointers with const pointees and multiple pointer levels
- Adjusted array parameters and top-level parameter qualifiers
- Opaque struct and union tags
- Named enums with native integer representations and member values
- Selected scalar, pointer, callback, and opaque aggregate typedefs
- Function pointers, callback returns, and variadic functions
- Mutable and const external variables, including pointers and callback globals
- Windows DLL imports for functions and data
- [Native calling conventions](language.md#calling-conventions)

Clang preprocesses includes and macros before Crust reads the function types.
Typedefs resolve to their native types. Structs and unions are exposed through
opaque pointers. Names reserved by Crust receive a `c_` prefix and retain their
native symbol through a renamed import.
Selection names the native declaration. A later macro with the same
name does not redirect the binding.

C array parameters become pointers, including `[static N]` and `restrict`
declarators. Their minimum-length and aliasing requirements belong to the unsafe
caller contract. The [C algorithms example](../examples/c_algorithms.cr) sorts,
searches, and copies arrays through the C standard library.

The generator checks its declarations against the original C types before
returning them. Types requiring further compiler support produce a diagnostic.
Aggregates passed by value and header-local static functions need a native C
wrapper with an exported pointer-based interface.
Thread-local data require native C accessor functions.

## Typedefs

Select aliases directly when they are useful in Crust source:

```powershell
cargo run -- import-c native/api.h --typedef NativeCount --typedef NativeCallback --output types.cr
```

The generator writes Crust typedefs for the selected names. Functions still use
their resolved native types. Pointer targets retain const and volatile
qualifiers, and callbacks retain their calling conventions.

Aliases sharing a name with an imported enum tag receive a `c_` prefix so both
types remain available. Crust built-in names such as `size_t` receive the same
treatment. Opaque struct and union aliases expose their pointers through the
native tag.

Generation checks pointer type compatibility and the size and alignment of
complete scalar and pointer aliases. Typedefs with extra alignment or unsupported
array layouts produce a diagnostic. For top-level const or volatile aliases,
import the affected C functions and variables directly.

## Native enums and headers

Bindings that use a native enum retain its C type through a header reference:

```c
extern header "native.h";

extern enum NativeStatus : int32_t {
    NativeReady = 1,
    NativeError = -1,
};

extern NativeStatus native_status(const struct NativeCounter* counter);
```

The header path is relative to the generated binding file. Output written to
stdout uses an absolute path. `extern header` adds a C include to the generated
translation unit. Crust declarations describe the API exposed to Crust code.

Enums used by selected functions and variables are included automatically.
`--enum` also imports enums directly from headers containing constants:

```powershell
cargo run -- import-c native/constants.h --enum Status --output constants.cr
```

The generator reads enum values and representation through Clang. Native
compilation checks the enum's compatible integer type, size, and alignment.
This preserves native type identity for const enum pointers and objects.
Renamed tags use `extern enum c_init = "init" : int32_t { ... };`.

Anonymous enums and enum layouts with extra alignment require a C wrapper.

## Linking

Link the C implementation or library through the target's
[native options](projects.md). Use matching `defines` and `--define` values when
the header changes its API through preprocessor configuration.
Generated declarations use the same unsafe-call
rules as handwritten `extern` declarations.

For an existing DLL, put its import library in `link-inputs` and its DLL in
`runtime-libraries`. The build deploys the DLL beside the executable and creates
its application manifest. Runtime files also propagate through linked targets.
