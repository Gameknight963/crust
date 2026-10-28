# C compatibility

Crust uses native C declarations, generated header bindings, and native C source
files to work with C libraries. Generated code targets C11 and uses the host ABI.

| Feature | Direct support | Remaining work or wrapper route |
| --- | --- | --- |
| Native integer types, fixed-width aliases, and booleans | Yes | Integer operations and narrowing conversions use checked Crust rules |
| `float`, `double`, and `long double` | Yes | Floating constant evaluation in integer constant expressions |
| Raw pointers, qualifiers, and pointer arithmetic | Yes | Raw storage access and C calls require `unsafe` |
| Functions, callbacks, variadic declarations, calling conventions | Yes | Old declarations with unspecified parameter types need explicit prototypes |
| Named enums and explicit values | Yes | Selected header enums preserve their native representation |
| Fixed-size arrays and strings | Yes | Variable-length arrays and flexible array members use native wrappers and pointer/length APIs |
| Ordinary structs and unions | Source layouts, opaque C tags, and selected complete native records with by-value APIs | Extend field imports to arrays, qualifiers, and anonymous aggregates |
| Packing, explicit alignment, and bitfields | Native record layout, packed-field borrows, and aligned generic allocation | Bitfields use native accessors |
| External variables, including DLL data | Yes | Source globals use generated symbols and can expose access through exported functions |
| Source globals and local static storage | Constant initialization and copyable types | Resource-owning static storage requires initialization and shutdown rules |
| C preprocessor definitions and header preprocessing | Definitions, preprocessing, and selected integer/boolean macro constants | Native wrappers for function, string, and floating macros |
| Header-local functions and variables | Import static function definitions, inline functions, and static storage | Header-local storage follows C translation-unit linkage |
| Atomics and thread-local storage | Through native C wrappers | Expose checked synchronization and storage APIs |
| Complex numbers and extended integer types | Through native C wrappers | Add direct types and ABI validation |
| Assembly and compiler-specific extensions | Through native C wrappers | Keep extensions in native source files compiled by their required toolchain |

Native source files and their include paths, definitions, libraries, and link
inputs belong in [project configuration](projects.md). A wrapper can expose a
C-compatible function around a declaration that the header importer cannot yet
represent. Crust can then wrap that function in an owning type with safe methods.

## Validation

- Compare imported scalar types, enum representations, calling conventions, and pointer qualifiers against native declarations.
- Compare aggregate size, alignment, and field offsets before exposing a new imported layout.
- Exercise arguments and return values in both directions, including callbacks and variadic promotions.
- Check shared-library imports, runtime deployment, and generated export headers on each supported platform.
- Document the unsafe contract for ownership, validity, aliasing, callbacks, and synchronization at each boundary.

The [binding tests](../tests/bindings.rs) validate selected header declarations
through Clang and native execution. [Project integration tests](../tests/projects.rs)
exercise compiler and linker behavior across project inputs.

Native wrapper tests also execute atomic and thread-local accessors, flexible
array allocation, variable-length arrays, complex arithmetic, 128-bit integer
arithmetic, and function/string/floating macro accessors. Their public signatures
use scalar values and opaque pointers. The native compiler owns the underlying
representations and storage rules.
