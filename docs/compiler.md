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
| `src/check.rs` | Names, types, ownership states, call-argument borrows |
| `src/types.rs` | Struct and array layout dependencies and ownership propagation |
| `src/emit_c.rs` | Generated C and cleanup |
| `src/runtime_numbers.h` | Embedded portable C helpers for wide arithmetic and checked conversions |
| `src/native.rs` | Native compiler invocation and Windows toolchain discovery |
| `src/project.rs` | TOML project loading, discovery, and target selection |
| `src/lsp.rs` | Document synchronization and compiler diagnostics |
| `src/bin/crust-lsp.rs` | Standalone stdio language server |
| `tests/common/mod.rs` | Allocator tracking for native cleanup tests |

## Native compilation

- Generated C uses C11
- The driver invokes `CC`, defaulting to `clang` on Windows and `cc` elsewhere
- Compiler arguments follow the Clang/GCC convention
- Single-file build output goes to `target/crust/`, using the source file's basename
- Project build output goes to `<project>/target/crust/<target>/`
- On Windows, the driver locates the MSVC tools through `cl.exe` on `PATH` when Clang needs help selecting the installation
- Toolchain environment changes apply to the child compiler process
- Pointer-sized types follow the Rust host, with a generated C assertion checking native pointer width
- `run` forwards the low eight bits of the native program's exit code

## Ownership and cleanup

- A borrow records its pointee type and access permission in a shared type representation
- Named owning structs have separate initialized-value flags in generated functions
- A move clears the source's flag, and reassignment restores it
- Cleanup checks the flag before destroying a named struct
- Struct layouts contain the declared fields
- Fixed-size arrays use generated C structs containing a native array, preserving by-value copies, returns, size, and alignment
- Layout dependency ordering includes arrays and rejects cycles through array elements
- Owning arrays have live-value flags and drop elements in reverse index order
- Indexed places capture the array address and check the index before element access
- `BoxInt` uses a null pointer as its internal moved-value flag
- Generated struct cleanup calls the custom destructor, then cleans up owning fields in reverse declaration order
- Method calls lower to ordinary functions with receiver parameters

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
