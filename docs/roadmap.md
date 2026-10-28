# Roadmap

## Implemented foundations

- Ownership checking for heap values and structs
- Shared and exclusive call borrows
- Instance methods, value receivers, and destructors
- Bare struct names, basic typedefs, and static methods
- For loops, break/continue cleanup, and compound assignments
- Single-statement control-flow bodies with nearest-if else binding and ordinary cleanup
- Do-while loops with body-first checking, condition-directed continue, and ownership cleanup
- Conditional expressions with short-circuit evaluation, owner-state merging, and integer constants
- Prefix and postfix increment and decrement with single destination evaluation and volatile accesses
- Boolean values, scalar conversions, and a common borrow representation
- Fixed-width and pointer-sized integers, checked conversions, casts, and size/alignment queries
- ASCII character literals and hexadecimal, binary, octal, and decimal integers
- C integer literal suffixes with native types, constant expressions, and exact variadic argument types
- A C-backed file resource with automatic closing, checked explicit close, and synchronized binary I/O
- Fixed-size arrays, nested initializers, checked indexing, array borrows, and owning-element cleanup
- UTF-8 byte-array string literals, literal concatenation, and bounded text output
- Local references with lexical lifetimes, nested reborrows, and scope-based loan release
- Distinct enum types, explicit representations, scoped and unscoped values, and checked casts
- Borrowed returns tied to a single input or receiver, returned-reference chaining, and expression temporary loans
- Reference fields in value aggregates with source lifetimes through copies, moves, nested values, slices, and returned views
- Explicit borrows(...) return contracts with multiple possible sources and inherited reborrow guards
- Alias-style `T&` references with contextual address-taking and direct value, field, method, and indexing access
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
- File-local static functions and generic helpers, with source-aware lookup and editor completion
- Multi-file generic containers and LSP analysis of unsaved dependencies
- Shared source library for generic boxes, buffers, vectors, and stable sorting
- Shared and mutable slices with runtime lengths, checked indexing, array conversions, and container views
- Native argc/argv entry points and program argument forwarding through the driver
- Scoped enum class declarations with qualified members and independent member names
- Generic tagged unions, checked payload access, active-payload cleanup, and consuming extraction
- Tagged-union switch cases with fallthrough, default, loop control, and ownership-state merging
- Library Result<T, E> values with success/error construction and borrowed or consuming access
- Fallible box and buffer factories and vector growth, with rejected values returned in allocation errors
- Owned byte buffers and validated UTF-8 strings with fallible copying, appending, and checked byte boundaries
- Unsafe function and method contracts, including generic instances and interface requirements
- Explicit raw-storage initialization and destruction, plus a generic heap owner for copyable values and owning types
- Unsafe whole-value moves from raw storage, consuming box extraction, and growable vectors of owning elements
- Ordered vector insertion and removal, allocation reuse after clearing, and cleanup for arrays of owners
- Generic stable sorting through interfaces, with vector swaps that preserve ownership
- Variadic C declarations and callbacks with default integer argument promotions
- Native-layout unions, opaque union tags, generic unions, and overlapping field-borrow checks
- Native C integer spellings and conversion ranks, plus compatible repeated typedefs and external declarations
- Integer shifts and compound shifts with C promotions, checked counts, and signed overflow checks
- Integer constant expressions in enum initializers, including imported enum dependencies and source references
- Constant expressions for fixed array lengths in typedefs, fields, locals, and generic arguments
- Renamed C imports with direct symbol linkage and cross-alias signature checks
- Named C exports with compatible prototypes, native calling conventions, and automatic local cleanup
- Explicit C and Windows calling conventions for native declarations, callbacks, functions, and methods
- External C variables with const permissions, renamed imports, shared alias borrow roots, and source navigation
- Volatile C objects, qualified raw pointers, and borrowed views with preserved access semantics
- Clang-backed generation of selected C function bindings with native type compatibility checks
- Header-generated external variables with const objects, callback globals, and native storage checks
- Windows DLL function and data imports, including header generation and import-library linking
- Header-generated native enum types and values, with explicit tag selection, relative C header references, and layout checks
- Selected C typedef imports with qualified pointers, callbacks, opaque aggregates, and native layout validation
- Opaque C tag aliases with keyword-safe imports, preserved export headers, and binding-checked source rename
- Per-target C preprocessor definitions, including function macros and quoted values
- Selected integer and boolean C macro constants with native scalar types and checked full-width values
- Shared source-analysis snapshots and LSP hover, definition, and document-symbol requests
- Navigation for typedefs, type names, interfaces, and generic parameters, with outlines for unused generic declarations
- Reference searches across source imports with binding-aware redeclaration grouping
- Source declaration rename with binding checks, preserved C names, designated initializers, and affected-target validation
- Scope completion with shadowing, imported declarations, generic parameters, and partial-word recovery
- Resolved member completion for fields, methods, and enums, including generic receivers and private-field visibility
- Workspace symbol search across unopened project targets and shared imports, with unsaved source overlays and folder updates
- Signature help with resolved generic and callback signatures, parsed argument boundaries, and partial-argument recovery
- Semantic highlighting from resolved bindings, declaration and built-in modifiers, and dependency refresh requests
- Bounded parser recursion, expression trees, and composed types, with consistent frontend worker stacks
- TOML project loading and named executable targets
- Static-library targets with C exports, private helpers, native object inputs, and fresh archive builds
- C header generation for scalar, enum, opaque-pointer, and callback exports
- Native target links with dependency builds, transitive archive ordering, and cycle diagnostics
- Local path dependencies with qualified target links, shared diamond dependencies, and consumer-owned artifacts
- Transitive native library names and search directories supplied by linked library targets
- Shared-library targets, Windows import libraries and DLL deployment, transitive linking, and runtime dependency paths
- Configured C runtime libraries with transitive deployment, input protection, and metadata
- Source names for native debug locals, parameters, fields, and readable concrete types
- Source line mapping in generated C for native function, expression, and cleanup locations
- Per-target native debug information with PDB validation, debug bundle recovery, and protected symbol outputs
- Native address/undefined sanitizer instrumentation with transitive library settings and executed sanitizer probes
- CI configuration for Windows, Linux, macOS, native compiler coverage, mutation tests, sanitizers, and editor packages
- Host compiler archives with the standalone LSP, source library, checksums, and executed package smoke checks
- Standalone stdio language server with live compiler diagnostics
- VS Code client, syntax highlighting, and Neovim setup
- VS Code project build, check, and run tasks with compiler error links
- VS Code native debugger launch with build completion checks, runtime paths, arguments, and cancellation handling
- Visual Studio VSIX with a bundled server, shared grammar, and native semantic classifications
- Visual Studio project build and check commands, Error List navigation, and executable console launch
- Optional MSBuild `.crproj` projects with bundled Visual Studio SDK paths, source items, build properties, and native builds
- Visual Studio native debugger launch for executable project targets with local arguments and working-directory settings

## Compiler work

The release target is a maintained command-line application using multiple Crust
files, a native C library, containers, files, and recoverable errors.

- [x] Floating-point types, literals, arithmetic, conversions, and C calls
- [x] Integer and enum switch statements
- [x] Global definitions and static variables
- [ ] References stored in structs, returned-reference contracts with multiple inputs, and temporary lifetimes
- [x] Safe strings, byte buffers, and fallible allocation and resource APIs using Result
- [ ] Migrate examples and tests from BoxInt and remove the built-in type
- [x] Project dependencies, library-supplied native dependencies, and reliable rebuilds
- [ ] C compatibility checklist and broader declarations, layouts, alignment, packing, bitfields, and macro coverage
- [ ] Ownership audit, mutation testing, sanitizer runs, and Windows/Linux/macOS CI
- [x] Compiler, LSP, and editor release packages with native compiler setup
- [x] VS Code native debugger launch and original source names in debug information
- [x] Language specification covering lifetimes, conversions, initialization, cleanup, and ABI rules
- [x] Build and maintain the release validation application

## Editor support

- Extend the standalone `crust-lsp` server and VS Code client with shared semantic information
- Use `crust.toml` as the authoritative project configuration
- Extend native debug names to static storage and improve source spellings for shadowed bindings

The LSP uses shared compiler analysis for diagnostics, hover, navigation, and
source outlines, reference searches, rename, and scope and member completion.
Signature help uses the same resolved call analysis.

## Further design

- Borrowed-field lifetimes and field moves in the presence of destructors
- Temporary owner lifetimes
- File-local helper types and further visibility controls
- Constrained `weak`, `expect`, and `replace` typedefs across a build
- Static-only types and first-use type initialization
