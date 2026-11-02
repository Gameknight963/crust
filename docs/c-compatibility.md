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
| Fixed-size arrays, array typedefs, and strings | Yes | Native flexible members expose raw pointer/length views. Variable-length arrays and C pointers to whole arrays use native wrappers |
| Ordinary structs and unions | Source layouts, opaque C tags, and selected complete native records with by-value APIs, fixed arrays, qualified fields, promoted anonymous groups, and flexible tails | Named anonymous aggregate types use native accessors |
| Packing, explicit alignment, and bitfields | Native record layout, packed-field borrows, aligned generic allocation, and native bitfield reads/writes | Bitfields have checked stores and cannot be addressed |
| External variables, including DLL data | Yes | Source globals use generated symbols and can expose access through exported functions |
| Source globals and local static storage | Constant initialization and copyable types | Resource-owning static storage requires initialization and shutdown rules |
| C preprocessor definitions and header preprocessing | Definitions, preprocessing, and selected integer/boolean macro constants | Native wrappers for function, string, and floating macros |
| Header-local functions and variables | Import static function definitions, inline functions, and static storage | Header-local storage follows C translation-unit linkage |
| Atomics and thread-local storage | Through native C wrappers | Expose checked synchronization and storage APIs |
| Complex numbers and extended integer types | Through native C wrappers with scalar components or fixed-width limbs | Native representation and arithmetic remain inside the wrapper |
| Assembly and compiler-specific extensions | Through native C wrappers | Keep extensions in native source files compiled by their required toolchain |

Native source files and their include paths, definitions, libraries, and link
inputs belong in [project configuration](projects.md). A wrapper can expose a
C-compatible function around a declaration that the header importer cannot yet
represent. Crust can then wrap that function in an owning type with safe methods.
The [wrapper recipes](c-bindings.md#other-native-wrappers) describe the tested
routes and their caller contracts.

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

Run the focused host acceptance gate with:

```text
python tools/check_c_compatibility.py --offline
```

Use `--full` for the complete compiler regression suite. `CC` selects the native
compiler executable. Clang is also required for header import and ABI probes.
The gate fails when either executable is missing.
Set `CRUST_TEST_SQLITE=1` and run `python examples/sqlite_project/prepare.py`
to include the real-library checks. CI enables them on every compiler host.

| Acceptance area | Executed evidence |
| --- | --- |
| Declaration shapes and regeneration | Binding tests regenerate fixed-array aliases, nested callbacks, qualifiers, native enums, and aggregate declarations |
| Layout and field access | Native record tests check size, alignment, packing, fixed arrays, bitfields, promoted groups, and flexible tails against real headers |
| Arguments, returns, and callbacks | Native record tests send packed bitfield/union records through C callbacks and returns. Export tests compile separate C callers |
| Link and deployment | Project tests build static/shared dependency graphs, native sources, export headers, and runtime libraries |
| Wrapper boundaries | Binding tests execute the published wrapper fixture and reject unsupported array declarators with the wrapper diagnostic |
| Resource ownership | SQLite tests exercise a real C library, moves, close failures, and allocation failure |

CI runs the full gate on Windows Clang, Linux Clang/GCC, and macOS Clang. The
sanitizer job repeats the binding, aggregate, and ownership execution tests under
ASan/UBSan. CI also runs deterministic frontend mutations and builds an extracted
compiler archive whose examples execute outside the checkout. A local Windows
run establishes the Windows result. The other hosts require their CI results.

This checklist covers supported declarations and explicit wrapper routes.
Direct complex, atomic, TLS, VLA, and compiler-extension syntax remain outside
the source language. A C API's allocation, retention, aliasing, and concurrency
contracts still require an unsafe boundary and a reviewed owning wrapper.
