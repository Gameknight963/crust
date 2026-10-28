# C header bindings

`crust import-c` reads a C header through Clang and writes declarations for selected
functions, external variables, named enums, typedefs, and integer macro constants. Ordinary Crust source imports the
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
| `--struct <tag>` | Select a complete native struct |
| `--union <tag>` | Select a complete native union |
| `--typedef <name>` | Select a C typedef, repeated for each alias |
| `--macro <name>` | Select an object-like integer or boolean macro constant |
| `--output <file.cr>` | Write UTF-8 bindings to a file |
| `--force` | Replace an existing output file |
| `--include-dir <path>` | Add a C header search directory |
| `--define <name=value>` | Set a C preprocessor definition |
| `--clang <path>` | Select the Clang executable |

Clang must be available when generating bindings. Without `--output`, the command
writes to stdout. Generation finishes before the output file is written, and the
command protects its input header from replacement.

## Imported declarations

- Native C integers, booleans, floating-point types, and `void`
- Raw pointers with const pointees and multiple pointer levels
- Adjusted array parameters and top-level parameter qualifiers
- Opaque struct and union tags
- Selected complete native records, with scalar, pointer, callback, enum, and selected record fields
- Named enums with native integer representations and member values
- Selected scalar, pointer, callback, and opaque aggregate typedefs
- Function pointers, callback returns, and variadic functions
- Header-defined static functions, including `static inline` and callback APIs
- Header-local static variables with native initialization
- Fixed-array record fields, including nested arrays, callbacks, and qualified elements
- Const and volatile record fields with preserved access permissions
- Mutable and const external variables, including pointers and callback globals
- Windows DLL imports for functions and data
- [Native calling conventions](language.md#calling-conventions)
- Integer and boolean macro constants with their native scalar types

Clang preprocesses includes and macros before Crust reads the function types.
Typedefs resolve to their native types. Structs and unions use opaque pointers
unless their complete definitions are selected. Names reserved by Crust receive a `c_` prefix and retain their
native symbol through a renamed import.
Keyword-named C tags use opaque aliases such as `struct c_init = "init";` and
`union c_move = "move";`. Exported C headers retain their original tag names.
Selection names the native declaration. A later macro with the same
name does not redirect the binding.

Declarations with assembler symbol labels retain an `extern header` reference.
The native compiler uses that header to preserve the symbol's linker name.

C array parameters become pointers, including `[static N]` and `restrict`
declarators. Their minimum-length and aliasing requirements belong to the unsafe
caller contract. The [C algorithms example](../examples/c_algorithms.cr) sorts,
searches, and copies arrays through the C standard library.

The generator checks its declarations against the original C types before
returning them. Types requiring further compiler support produce a diagnostic.
Aggregates passed by value require selecting their complete definitions.

For header-local static functions and variables, generated bindings retain an
`extern header` reference. The native compiler compiles their definitions from
that header in each consuming translation unit. Keep the header available and
use the same include paths and preprocessor definitions when generating bindings and building
the project. A selected static function must have a definition in the header.
Header-local storage has a separate instance in each translation unit, following
C linkage rules. All Crust source imports for one target share a translation
unit.

The [header-only example](../examples/header_only_project/) wraps a packed C
record with bitfields in a move-only Crust type. Its bindings call ordinary
static functions and a static inline getter, and expose a header-local counter.

Thread-local data require native C accessor functions.

## Macro constants

```text
crust import-c native.h --macro API_LIMIT --macro API_FLAGS --output bindings.cr
```

Integer and boolean object-like macros become `const` globals with their native
scalar types. Clang evaluates casts, arithmetic, size queries, and macros reached
through includes or `--define`. Generation checks the type and full-width value,
including negative values and unsigned 64-bit limits.

Bindings retain the native name when available. Reserved names and collisions
with selected enum members receive a `c_` prefix and a suffix when needed.
Regenerate bindings when the header configuration or target changes.

These bindings are read-only values. Array lengths and enum initializers require
integer constant expressions, so they cannot use these globals. Import C enum
values with `--enum` when those values need constant-expression use.

Function-like macros and macros producing pointers, strings, or floating values
use native C wrappers.

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

## Complete native records

```text
crust import-c native.h --struct Point --union Number --function translate --output bindings.cr
```

Generated records use the header's actual C type:

```c
extern header "native.h";
extern struct Point { int x; int y; };
extern union Number { uint64_t bits; double number; };
```

Select nested record fields by their tags as well. The native compiler supplies
size, alignment, packing, and the by-value calling convention. Generated checks
verify field types and that struct fields are nonoverlapping and listed in
header order. Packed fields use ordinary Crust access and borrowing rules.

Tag and field aliases preserve native names, for example:

```c
extern struct Local = "NativeRecord" { int c_init = "init"; };
```

Header macros are preserved around native field access. Source rename adds an
alias when needed to preserve the C declaration. Bitfields, flexible arrays,
and named fields with anonymous aggregate types use native accessors.

```c
extern struct Readings {
    const int id;
    volatile int updates;
    int values[3];
};
```

Fixed arrays support value copies, field assignment, checked indexing, and
ordinary array references. Const fields allow initialization and shared borrows.
A record containing const fields supports fresh copies and function returns.
Whole-value assignment is rejected because C treats that record as nonmodifiable.
Other fields remain writable through mutable references. Volatile fields preserve
volatile reads, writes, and reference qualifiers, including inside packed arrays.

Const and volatile fields require copyable values. Source initialization of a
const native array is limited to 4096 elements. Larger const arrays can be supplied
by a native factory. Flexible and zero-length C array fields use native accessors.

## Native macro wrappers

A typed header function can expose a function-like macro, string macro, or
floating macro. For example:

```c
#define API_DOUBLE(value) ((value) + (value))
#define API_NAME "example"
#define API_SCALE 1.5L

static inline int api_double(int value) { return API_DOUBLE(value); }
static inline const char *api_name(void) { return API_NAME; }
static inline long double api_scale(void) { return API_SCALE; }
```

```text
crust import-c native.h --function api_double --function api_name --function api_scale --output bindings.cr
```

The native compiler expands the macros inside their wrappers. Crust checks the
wrapper signatures, and callers use the generated functions inside `unsafe`.
The same pattern exposes native atomic and thread-local storage through accessor
functions.
