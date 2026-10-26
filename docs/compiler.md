# Compiler

The compiler processes each target's entry file through tokenization, parsing,
ownership checking, C generation, and native compilation.

## Source files

| File | Responsibility |
| --- | --- |
| `src/main.rs` | CLI commands, source loading, build output, execution |
| `src/lib.rs` | Frontend and source-to-C APIs |
| `src/lexer.rs` | Tokens and scanning |
| `src/diagnostic.rs` | Source locations and diagnostics |
| `src/ast.rs` | Syntax tree and type representation |
| `src/parser.rs` | Declarations, statements, expressions |
| `src/parser/generics.rs` | Generic declarations, concrete instantiation, and interface requirements |
| `src/check.rs` | Names, types, ownership states, call-argument borrows |
| `src/types.rs` | Struct and array layout dependencies and ownership propagation |
| `src/emit_c.rs` | Generated C and cleanup |
| `src/runtime_numbers.h` | Embedded C helpers for checked arithmetic, indexing, and text output |
| `src/native.rs` | Native compiler invocation and Windows toolchain discovery |
| `src/project.rs` | TOML project loading, discovery, and target selection |
| `src/lsp.rs` | Document synchronization and compiler diagnostics |
| `src/bin/crust-lsp.rs` | Standalone stdio language server |
| `tests/common/mod.rs` | Allocator tracking for native cleanup tests |

## Native compilation

- Generated C uses C11
- Enums lower to distinct generated typedef names with their declared integer representation
- Raw pointer types use generated C typedefs, with struct and array tags declared before pointer typedefs
- Struct values use generated aliases, and opaque aliases preserve their native C tags
- External declarations and calls retain their C symbol names
- Function and pointer typedefs are emitted in dependency order, including callbacks returning or accepting other function pointers
- Callback calls evaluate the function pointer before the arguments and use the declared C signature
- Generic declarations instantiate concrete structs and functions for each used combination of type arguments
- Expression types, conversions, resolved calls, and constants are recorded separately for each function, including generic instances sharing source spans
- Interface constraints compare concrete method signatures before function bodies are checked, and calls retain ordinary direct dispatch
- The driver invokes `CC`, defaulting to `clang` on Windows and `cc` elsewhere
- Compiler arguments follow the Clang/GCC convention
- Single-file build output goes to `target/crust/`, using the source file's basename
- Project build output goes to `<project>/target/crust/<target>/`
- Target native options add C11 source files, include paths, explicit link inputs, and library search and name arguments
- Native paths are resolved relative to the manifest, and compiler arguments are passed directly to the process
- On Windows, the driver locates the MSVC tools through `cl.exe` on `PATH` when Clang needs help selecting the installation
- Toolchain environment changes apply to the child compiler process
- Pointer-sized types follow the Rust host, with a generated C assertion checking native pointer width
- `char` follows the host C signedness, with a generated assertion checking the selected C compiler
- `run` forwards the low eight bits of the native program's exit code

## Ownership and cleanup

- A borrow records its pointee type and access permission in a shared type representation
- Local reference bindings retain a loan ID and source root until their block ends
- Places and loans retain struct field paths, with overlapping prefixes identifying conflicts between a field and its parents
- Distinct field paths permit independent access, while array indices remain conservative
- Reborrow loans record their parent, allowing access through that ancestry while checking conflicts with sibling and child loans
- Call argument loans are temporary, and scope exit releases local-reference loans
- Borrowed return signatures identify a single borrowed input, and return checking verifies the source root
- Calls transfer the selected argument's origin into a returned loan and retain its surviving reborrow ancestry
- Reference initializers retain temporary ancestors, and statement and condition boundaries release other temporary loans
- Named owning structs have separate initialized-value flags in generated functions
- A move clears the source's flag, and reassignment restores it
- Cleanup checks the flag before destroying a named struct
- Struct layouts contain the declared fields
- Fixed-size arrays use generated C structs containing a native array, preserving by-value copies, returns, size, and alignment
- Layout dependency ordering includes arrays and rejects cycles through array elements
- Owning arrays have live-value flags and drop elements in reverse index order
- Indexed places capture the array address and check the index before element access
- String literals lower to numeric byte-array initializers, preserving UTF-8 bytes across C compiler execution encodings
- Text output bounds its zero-byte scan by the array capacity
- `BoxInt` uses a null pointer as its internal moved-value flag
- Generated struct cleanup calls the custom destructor, then cleans up owning fields in reverse declaration order
- Method calls lower to ordinary functions with receiver parameters
- Unsafe blocks lower to ordinary C blocks with the same cleanup rules
- Raw pointer indexing captures the base and index once, with checked conversion of offsets to the host `ptrdiff_t` representation
- Field privacy is checked in the frontend and preserves the C field layout

## Diagnostics

The checker records expression types, contextual integer constants, and numeric
conversions by source span. C generation uses those records to preserve integer
promotions and insert range checks. Signed arithmetic checks bounds before the
operation. Unsigned arithmetic uses the corresponding wrapping C operations.

Diagnostics use `file:line:column` locations. Spans contain UTF-8 byte offsets
and one-based character columns. Tabs count as one column.

## Development

```powershell
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```

Native execution tests compile generated C. Ownership tests track allocations,
frees, and destructor output to verify cleanup behavior.

Language-server tests launch `crust-lsp` and exchange framed protocol messages
through its standard input and output.
