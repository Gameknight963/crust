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
- TOML project loading and named executable targets
- Standalone stdio language server with live compiler diagnostics
- VS Code client, syntax highlighting, and Neovim setup

## Compiler work

1. Add modules, imports, visibility, project dependencies, and library targets
2. Build generic owners, slices, containers, and resource wrappers using Crust and C allocation
3. Extend C header importing and ABI coverage to unions, variadic functions, calling conventions, and layout attributes

A substantial validation target is a multi-file program using a generic container
backed by C allocation, with automatic cleanup and safe access to its contents.

## Editor support

- Expose frontend diagnostics, source symbols, and type information through shared Rust APIs
- Extend the standalone `crust-lsp` server and VS Code client with shared semantic information
- Build Visual Studio integration on the same language server
- Use `crust.toml` as the authoritative project configuration
- Add optional MSBuild `.crproj` integration for Visual Studio
- Add native source-level debugging through the generated C backend

Initial LSP work will cover document synchronization and diagnostics. Navigation,
completion, hover, and rename can use the shared semantic information as those
APIs become available.

## Further design

- Borrowed-field lifetimes and field moves in the presence of destructors
- Temporary owner lifetimes and borrow contracts with multiple borrowed inputs
- Tagged unions and fallible-operation conventions
- Module naming and visibility
- Constrained `weak`, `expect`, and `replace` typedefs across a build
- Static-only types and first-use type initialization
