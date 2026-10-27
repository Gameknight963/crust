Crust (C-Rust get it) is to Rust (the language) as Crust is to Rust (the game)

# Crust

A C-like language with ownership checking, written in Rust. The compiler generates
C and builds native executables, static libraries, and shared libraries.

## Goals

- Familiar C syntax
- Compile-time ownership and borrow checking
- Automatic resource cleanup with destructors
- Integration with C libraries and headers

## Example

```c
void increment(int& value) {
    value += 1;
}

int main() {
    BoxInt first = box_int(42);
    BoxInt second = move(first);
    increment(&*second);
    print(*second);
    return 0;
}
```

Crust supports structs, typedefs, static and instance methods, shared and exclusive borrows, and
`~Type()` destructors. Safe `T&` references act as aliases and use `&value` to borrow.
See [examples/](examples/) for runnable programs.

Fixed-size arrays support checked indexing and resource cleanup. String literals
use UTF-8 byte arrays.

### Ownership and containers

- [C allocation](examples/c_owner.cr): private storage, checked borrows, and a destructor
- [Generic buffer](examples/generic_buffer.cr): one container for integers, characters, and structs
- [Generic box](examples/generic_box.cr): heap values, including other owners
- [Container project](examples/container_project/): buffers, boxes, and vectors across imported files
- [Vector edits](examples/vector_edits.cr): insertion, removal, and cleanup
- [Sorting](examples/sorting.cr): a comparison interface and a vector of heap owners
- [File resource](examples/file_project/): binary I/O, ownership transfer, and automatic closing

### C integration

- [Headers](examples/header_project/): generated bindings and an opaque handle wrapper
- [C algorithms](examples/c_algorithms.cr): sorting and searching through standard library callbacks
- [Volatile access](examples/volatile.cr): qualified pointers and borrowed views
- [Exports](examples/export_project/): a C source file calling a named Crust function
- [Static library](examples/library_project/): a native archive linked into a C consumer
- [Shared library](examples/shared_project/): a C export linked into a Crust executable
- [Target links](examples/linked_project/): automatic library builds and native linking
- [Calling conventions](examples/calling_convention_project/): `__vectorcall` callbacks on Windows x86 and x64

## Getting started

Install Rust and Cargo, plus a C11 compiler and its development libraries.
The compiler driver defaults to Clang on Windows and `cc` elsewhere.

```powershell
cargo run -- examples/hello.cr
cargo run -- examples/methods.cr
cargo run -- examples/destructors.cr
cargo run -- run --project examples/project
```

In VS Code, install the recommended **rust-analyzer** extension. **Ctrl+Shift+B**
builds the project, and **Terminal > Run Task** lists the examples.

To select a C compiler, set `CC` to its executable path:

```powershell
$env:CC = 'clang'
```

## Commands

Use `cargo run -- <command> <file.cr>` for a source file, or run a command in a
directory containing `crust.toml`:

| Command | Action |
| --- | --- |
| `run` | Compile and run |
| `build` | Build executables and libraries |
| `check` | Check names, types, ownership, and borrows |
| `emit-c` | Write generated C to stdout |
| `emit-header` | Write C declarations for named exports to stdout |
| `tokens` | Print tokens and source locations |
| `metadata` | Write project targets and output paths as JSON |

Passing a source filename directly also compiles and runs it:

```powershell
cargo run -- examples/ownership.cr
```

## Documentation

- [Language reference](docs/language.md)
- [Projects](docs/projects.md)
- [C header bindings](docs/c-bindings.md)
- [Editor setup](docs/editors.md)
- [Compiler and development](docs/compiler.md)
- [Roadmap](docs/roadmap.md)
