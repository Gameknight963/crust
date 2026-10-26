# C header bindings

`crust import-c` reads a C header through Clang and writes declarations for selected
functions. The generated file can be imported by ordinary Crust source.

```powershell
cargo run -- import-c examples/header_project/native.h --function native_create --function native_value --function native_add --function native_destroy --output examples/header_project/bindings.cr --force
cargo run -- run examples/header_project
```

The [example](../examples/header_project/) wraps an opaque C handle in a move-only
Crust struct with a destructor.

## Options

| Option | Purpose |
| --- | --- |
| `--function <name>` | Select a C function, repeated for each export |
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
- Opaque struct and union tags
- Function pointers, callback returns, and variadic functions
- [Native calling conventions](language.md#calling-conventions)

Clang preprocesses includes and macros before Crust reads the function types.
Typedefs resolve to their native types. Structs and unions are exposed through
opaque pointers. Names reserved by Crust receive a `c_` prefix and retain their
native symbol through a renamed import.
Function selection names the native declaration. A later macro with the same
name does not redirect the binding.

The generator checks its declarations against the original C types before
returning them. Types requiring further compiler support produce a diagnostic.
Aggregates passed by value and header-local static functions need a native C
wrapper with an exported pointer-based interface.

Link the C implementation or library through the target's
[native options](projects.md). Use matching `defines` and `--define` values when
the header changes its API through preprocessor configuration.
Generated declarations use the same unsafe-call
rules as handwritten `extern` declarations.
