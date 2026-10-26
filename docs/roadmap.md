# Roadmap

## Implemented foundations

- Ownership checking for heap values and structs
- Shared and exclusive call borrows
- Instance methods, value receivers, and destructors
- Bare struct names, basic typedefs, and static methods
- For loops, break/continue cleanup, and compound assignments
- Boolean values, scalar conversions, and a common borrow representation
- TOML project loading and named executable targets
- Standalone stdio language server with live compiler diagnostics
- VS Code client, syntax highlighting, and Neovim setup

## Compiler work

1. Expand basic types with integer widths, unsigned and pointer-sized integers, and checked conversions
2. Add arrays, indexing, character and string literals, and enums
3. Extend borrows to locals and returns tied to an input or receiver. Define temporary lifetimes and disjoint field access
4. Add raw pointers, unsafe operations, external C declarations, function pointers, opaque types, and native linking
5. Add generic types and functions, interface constraints, and static interface requirements
6. Add modules, imports, visibility, project dependencies, and library targets
7. Build generic owners, slices, containers, and resource wrappers using Crust and C allocation
8. Extend C header importing and ABI coverage to unions, variadic functions, calling conventions, and layout attributes

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
- Tagged unions and fallible-operation conventions
- Module naming and visibility
- Constrained `weak`, `expect`, and `replace` typedefs across a build
- Static-only types and first-use type initialization
