# Compiler

The compiler processes each target's entry file and imported sources through tokenization, parsing,
ownership checking, C generation, and native compilation.

## Source files

| File | Responsibility |
| --- | --- |
| `src/main.rs` | CLI commands, source loading, build output, execution |
| `src/lib.rs` | Frontend and source-to-C APIs |
| `src/lexer.rs` | Tokens and scanning |
| `src/diagnostic.rs` | Source locations and diagnostics |
| `src/frontend.rs` | Frontend worker stack and thread-start diagnostics |
| `src/parser/limits.rs` | Parser recursion, expression-tree, and type-composition limits |
| `src/analysis.rs` | Public symbol, reference, and expression-type snapshots |
| `src/analysis/rename.rs` | Rename edits, native-name preservation, and binding validation |
| `src/analysis/completion.rs` | Lexical completion scopes, declaration order, and shadowing |
| `src/analysis/members.rs` | Resolved receiver types, member sets, and private-field visibility |
| `src/analysis/signatures.rs` | Resolved call signatures, argument boundaries, and parameter label ranges |
| `src/bindings.rs` | Clang header analysis and generated binding validation |
| `src/bindings/enums.rs` | Native enum discovery, integer representation, and constant probes |
| `src/bindings/render.rs` | C type trees to Crust declarations |
| `src/cli_bindings.rs` | Header binding command and output handling |
| `src/ast.rs` | Syntax tree and type representation |
| `src/parser.rs` | Declarations, statements, expressions |
| `src/parser/generics.rs` | Generic declarations, concrete instantiation, and interface requirements |
| `src/parser/calling_conventions.rs` | Target calling conventions and DLL import attributes |
| `src/parser/constants.rs` | Integer constant evaluation, array lengths, and enum dependencies |
| `src/parser/integers.rs` | C integer type specifiers |
| `src/parser/symbols.rs` | Source declarations and lexical type-parameter bindings |
| `src/check.rs` | Names, types, ownership states, call-argument borrows |
| `src/types.rs` | Struct and array layout dependencies and ownership propagation |
| `src/emit_c.rs` | Generated C and cleanup |
| `src/header.rs` | C export prototypes, callback declarations, and native tag preservation |
| `src/runtime_numbers.h` | Embedded C helpers for checked arithmetic, indexing, and text output |
| `src/native.rs` | Native compiler invocation and Windows toolchain discovery |
| `src/native/archive.rs` | Static archive staging, C object compilation, and archiver invocation |
| `src/native/executable.rs` | Executable backups, native output checks, and failure recovery |
| `src/native/staging.rs` | Fresh build directories and tracked-file cleanup |
| `src/native/volatile_tests.rs` | Optimized LLVM checks for volatile loads and stores |
| `src/project.rs` | TOML project loading, discovery, and target selection |
| `src/project/links.rs` | Native target dependency order and transitive archive inputs |
| `src/source.rs` | Import graph loading, source identity, and diagnostic mapping |
| `src/lsp.rs` | Document synchronization and compiler diagnostics |
| `src/lsp/semantic.rs` | Hover, definition, references, document symbols, and request positions |
| `src/lsp/completion.rs` | Completion items, prefix ranges, and partial-word analysis repairs |
| `src/lsp/workspace.rs` | Workspace folders, target loading, and cross-project symbol search |
| `src/lsp/watch.rs` | Client file-watcher registration and disk-change event validation |
| `src/lsp/rename.rs` | Project-wide rename and affected-target validation |
| `src/lsp/signature.rs` | Signature-help responses and active parameters |
| `src/lsp/semantic_tokens.rs` | Resolved identifier classification and UTF-16 semantic-token encoding |
| `src/bin/crust-lsp.rs` | Standalone stdio language server |
| `tests/common/mod.rs` | Allocator tracking for native cleanup tests |

## Source loading

`source::SourceSet` loads relative imports once and assigns each file a distinct
byte-offset range. Tokens retain their file-local lines and columns. File-end
markers keep declarations within their source file, and frontend errors map back
to the original path and byte range.

Native header references resolve relative to their declaring source file.
`SourceSet::native_headers` exposes their canonical paths for build-input
protection, and generated C includes those paths directly.

`SourceSet::load_with` accepts a reader for unsaved editor text. Its `check` and
`compile` methods process the full import graph. The string-based `check` and
`compile` functions process the supplied source text directly.

The `check_with_kind`, `compile_with_kind`, and `analyze_with_kind` APIs accept
`ProgramKind::Library` for entries without `main`. Checked snapshots retain that
kind through editor analysis and rename validation. Library emission gives
ordinary Crust functions internal linkage and preserves named C exports.

Static-library builds compile each C source separately and copy explicit native
objects under distinct staging names. The archiver writes a fresh archive before
replacing the destination with a single filesystem rename. Staging cleanup
removes the files created for that
build, including partial outputs after a tool failure.
The archive signature is checked before replacement, including when an archiver
exits successfully. Missing, truncated, and thin archive outputs preserve the
previous library.

Native target links use an iterative depth-first traversal to validate cycles
and order builds. Executable link inputs reverse that order so dependent
archives precede their dependencies. Archive builds retain separate outputs
for linked targets. The native drivers require target references to be resolved
to file inputs before invocation.

`emit_header` checks an executable or library entry and renders its named C
exports. Callback aliases are allocated within each exported symbol's prefix,
with names reserved against other native symbols. Pointer qualifiers are rendered
at each indirection level. Native tags and exported names use macro guards, and
native enum dependencies retain the source program's C headers.

Pointer and borrow types carry pointee volatility. External variables retain
object volatility separately. Place analysis propagates it through dereferences,
fields, and array elements, and emission uses that metadata for captured
assignment destinations. Binding generation and public headers preserve the
qualifier at each pointer level. An optimized Clang IR test checks volatile
loads and stores after borrowed access and aggregate projection.

Frontend APIs run parsing, checking, analysis, and emission on a scoped worker
with a 16 MiB stack. This gives command-line and editor clients the same stack
budget. Parser call nesting, expression trees, and composed pointer, array,
borrow, and callback types have a 64-level limit with source diagnostics.
Expression and type limits use iterative walks, including chains built without
recursive parsing. Struct tags end the type-composition walk, allowing recursive
structures through pointers.

`crust::analyze` and `SourceSet::analyze` return source snapshots with declarations,
resolved references, and expression types. Locations identify a file index and a
file-local byte span. Generic instances share their original declarations, and
ambiguous concrete types in replayed bodies are omitted from type queries.
Type references retain lexical typedef and generic-parameter bindings. Built-in
types have no source declaration.
Symbols expose a canonical declaration location for compatible redeclarations.
`Analysis::references_at` collects bound uses and optionally declarations, with
duplicate locations from generic replay removed.
`Analysis::rename` returns source edits for locals, explicit parameters, free
functions, instance and static methods, typedefs, interfaces, and external variables.
It reanalyzes edited snapshot text and compares
translated reference targets to reject name capture. Native imports gain explicit
aliases when needed to preserve their C names.

The LSP loads project targets with unsaved source overlays, merges shared-file
edits, and rechecks every affected target. LSP clients receive a workspace edit
for review and application.

Source discovery combines projects above open files with explicit workspace roots.
Diagnostics and semantic requests select importing targets that contain an open
file, including dependencies outside the project directory. Workspace-folder
and watched-file changes recheck open files and request semantic-token refresh
when supported. Invalid project configurations produce diagnostics on the manifest.

`Analysis::completions_at` returns visible declarations at a source position.
Block scopes retain local declaration boundaries, parameters belong to their
functions, and generic parameters stay within their source declaration.
The LSP filters names by the typed prefix and emits UTF-16 replacement ranges.
Incomplete words can be replaced in a temporary analysis snapshot without
changing the editor document or its published diagnostics.

`Analysis::member_completions_at` selects members through resolved receiver types
and the access operator. Replayed generic bodies offer source members common to
their concrete receivers. Receiver probes retain type information independently
of expression conversions and code generation.

Completion analysis retains function information gathered before a body error.
This produces an `AnalyzedProgram`. Native emission accepts a `CheckedProgram`,
which is constructed after full validation. Other semantic requests continue to
require a successfully checked program.

Call analysis records resolved function or callback targets before argument
validation. `Analysis::signature_help_at` selects the innermost call containing
the cursor. Parsed argument spans separate argument commas from nested syntax,
and parameter label ranges use UTF-16 offsets. Lowered instance calls retain
their original argument list for signature help.

Workspace symbol queries load every target in the configured workspace projects
and combine them with open-document source graphs. Unsaved documents override
disk text. Results deduplicate canonical declaration locations across shared
imports, and folder notifications update the project's search roots.

Semantic tokens combine canonical declaration kinds with resolved source uses.
The encoder walks source text once to compute UTF-16 positions across Unicode
comments, string literals, and line endings. Unambiguous bindings receive their
symbol kind, and built-in names receive the `defaultLibrary` modifier. Full-token
responses clear on frontend errors. Clients advertising refresh support receive
`workspace/semanticTokens/refresh` after document notifications.

## C header bindings

`bindings::generate` asks Clang for native declarations and structured types,
renders selected functions, variables, and enums, and checks the resulting Crust declarations.
Declaration queries filter to selected names, and type queries carry the
reachable typedef and pointer information needed by those functions.
Independent Clang type-compatibility assertions verify generated function and
object types against the original header. Binding tests require Clang on `PATH`.
Reachable native enums retain their C tags through emitted header references.
Clang constant probes determine their compatible integer types and member values.

The CLI writes the generated UTF-8 file after validation. See
[C header bindings](c-bindings.md) for commands and supported declarations.

## Native compilation

- Generated C uses C11
- Enums lower to distinct generated typedef names with their declared integer representation
- External enums alias native C tags and assert compatible integer types, size, and alignment
- Raw pointer types use generated C typedefs, with struct and array tags declared before pointer typedefs
- Struct values use generated aliases, and opaque aliases preserve their native C tags
- Union types share the aggregate type registry and generate native C unions, including opaque union tags
- C imports and exports retain their native symbol names in declarations, definitions, calls, and function pointers
- Renamed imports keep separate Crust and C names, and declarations sharing a C symbol are checked for compatible signatures
- External variables emit native object declarations, retain const permissions, and share borrow roots across aliases of one C symbol
- Windows DLL imports preserve native storage attributes across compatible declarations and native-symbol aliases
- Function and pointer typedefs are emitted in dependency order, including callbacks returning or accepting other function pointers
- Function declarations and callback typedefs emit their effective calling convention, with matching checks for native symbols and interface requirements
- Callback calls evaluate the function pointer before the arguments and use the declared C signature
- Variadic function types retain the fixed parameter list and ellipsis, and additional call arguments receive C integer promotions before emission
- Generic declarations instantiate concrete structs and functions for each used combination of type arguments
- Expression types, conversions, resolved calls, and constants are recorded separately for each function, including generic instances sharing source spans
- Interface constraints compare concrete method signatures before function bodies are checked, and calls retain ordinary direct dispatch
- The driver invokes `CC`, defaulting to `clang` on Windows and `cc` elsewhere
- Compiler arguments follow the Clang/GCC convention
- Single-file build output goes to `target/crust/`, using the source file's basename
- Project build output goes to `<project>/target/crust/<target>/`
- Target native options add C11 source files, include paths, explicit link inputs, and library search and name arguments
- Target macro definitions pass directly to the C preprocessor, with shared validation for native builds and header binding generation
- Native paths are resolved relative to the manifest, and compiler arguments are passed directly to the process
- On Windows, the driver locates the MSVC tools through `cl.exe` on `PATH` when Clang needs help selecting the installation
- Toolchain environment changes apply to the child compiler process
- Pointer-sized types follow the Rust host, with a generated C assertion checking native pointer width
- `char` follows the host C signedness, with a generated assertion checking the selected C compiler
- Native integer spellings retain C type identity and conversion rank, and generated assertions check the host's integer widths and standard typedefs
- `run` forwards the low eight bits of the native program's exit code

MSVC treats enum types as distinct in `_Generic`. Native enum assertions under
MSVC check signedness, size, and alignment. Clang also checks integer type
compatibility.

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
- `init` stores an evaluated value into raw storage without dropping previous contents, and `destroy` calls the existing cleanup operation without deallocating the storage
- `move` transfers owning locals and copies ordinary values, allowing the same generic body to accept either
- Unsafe moves from raw pointer dereferences or indices capture a whole value without destroying its source, leaving initialization bookkeeping to the container
- Union field projections include a wildcard path component, making overlapping storage remain overlapping through nested fields and reborrows

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
