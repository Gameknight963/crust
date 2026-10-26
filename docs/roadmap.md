# Roadmap

## Implemented foundations

- Ownership checking for heap values and structs
- Shared and exclusive call borrows
- Instance methods, value receivers, and destructors
- Bare struct names, basic typedefs, and static methods
- For loops, break/continue cleanup, and compound assignments
- Boolean values, scalar conversions, and a common borrow representation
- Fixed-width and pointer-sized integers, checked conversions, casts, and size/alignment queries
- ASCII character literals and hexadecimal, binary, octal, and decimal integers
- Fixed-size arrays, nested initializers, checked indexing, array borrows, and owning-element cleanup
- UTF-8 byte-array string literals, literal concatenation, and bounded text output
- Local references with lexical lifetimes, nested reborrows, and scope-based loan release
- Distinct enum types, explicit representations, scoped and unscoped values, and checked casts
- Borrowed returns tied to a single input or receiver, returned-reference chaining, and expression temporary loans
- Disjoint struct-field borrows with parent overlap checking
- Native C `char`, raw pointers, unsafe blocks, and external C function declarations
- Private struct fields and a move-only owner backed by C allocation
- Unsafe pointer indexing and arithmetic, plus a C-backed integer buffer with checked access
- Per-target native C sources, include paths, object/library inputs, and native linking
- Opaque C struct tags, forward declarations, and private handle wrappers
- Function pointer typedefs, function and static-method addresses, and C callbacks in both directions
- Generic structs and functions with explicit type arguments and per-instantiation ownership checking
- A C-backed generic buffer with checked borrowed access and automatic cleanup
- Structural interface constraints, static interfaces, and static member requirements
- Relative source imports, dependency collection, and diagnostics with original file locations
- Multi-file generic containers and LSP analysis of unsaved dependencies
- Unsafe function and method contracts, including generic instances and interface requirements
- Explicit raw-storage initialization and destruction, plus a generic heap owner for copyable values and owning types
- Unsafe whole-value moves from raw storage, consuming box extraction, and growable vectors of owning elements
- Variadic C declarations and callbacks with default integer argument promotions
- Native-layout unions, opaque union tags, generic unions, and overlapping field-borrow checks
- Native C integer spellings and conversion ranks, plus compatible repeated typedefs and external declarations
- Integer shifts and compound shifts with C promotions, checked counts, and signed overflow checks
- Integer constant expressions in enum initializers, including imported enum dependencies and source references
- Constant expressions for fixed array lengths in typedefs, fields, locals, and generic arguments
- Renamed C imports with direct symbol linkage and cross-alias signature checks
- Explicit C and Windows calling conventions for native declarations, callbacks, functions, and methods
- External C variables with const permissions, renamed imports, shared alias borrow roots, and source navigation
- Clang-backed generation of selected C function bindings with native type compatibility checks
- Header-generated external variables with const objects, callback globals, and native storage checks
- Windows DLL function and data imports, including header generation and import-library linking
- Header-generated native enum types and values, with relative C header references and layout checks
- Per-target C preprocessor definitions, including function macros and quoted values
- Shared source-analysis snapshots and LSP hover, definition, and document-symbol requests
- Navigation for typedefs, type names, interfaces, and generic parameters, with outlines for unused generic declarations
- Reference searches across source imports with binding-aware redeclaration grouping
- Local and parameter rename with source reanalysis, capture checks, and versioned LSP edits
- Scope completion with shadowing, imported declarations, generic parameters, and partial-word recovery
- Resolved member completion for fields, methods, and enums, including generic receivers and private-field visibility
- TOML project loading and named executable targets
- Standalone stdio language server with live compiler diagnostics
- VS Code client, syntax highlighting, and Neovim setup

## Compiler work

1. Add module namespaces, visibility, project dependencies, and library targets
2. Build generic owners, slices, containers, and resource wrappers using Crust and C allocation
3. Extend C header importing and ABI coverage to layout attributes and further C declarations

A substantial validation target is a multi-file program using a generic container
backed by C allocation, with automatic cleanup and safe access to its contents.

## Editor support

- Extend rename to additional symbol kinds and add signature help
- Extend the standalone `crust-lsp` server and VS Code client with shared semantic information
- Build Visual Studio integration on the same language server
- Use `crust.toml` as the authoritative project configuration
- Add optional MSBuild `.crproj` integration for Visual Studio
- Add native source-level debugging through the generated C backend

The LSP uses shared compiler analysis for diagnostics, hover, navigation, and
source outlines, reference searches, local rename, and scope and member completion.
Signature help and broader rename support can extend the same analysis.

## Further design

- Borrowed-field lifetimes and field moves in the presence of destructors
- Temporary owner lifetimes and borrow contracts with multiple borrowed inputs
- Tagged unions and fallible-operation conventions
- Module naming and visibility
- Constrained `weak`, `expect`, and `replace` typedefs across a build
- Static-only types and first-use type initialization
