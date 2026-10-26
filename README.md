Crust (C-Rust get it) is to Rust (the language) as Crust is to Rust (the game)

# Crust

A C-like language with ownership checking, written in Rust. The compiler generates
C and builds native executables.

## Goals

- Familiar C syntax
- Compile-time ownership and borrow checking
- Automatic resource cleanup with destructors
- Integration with C libraries and headers

## Example

```c
int main() {
    BoxInt first = box_int(42);
    BoxInt second = move(first);
    print(*second);
    return 0;
}
```

Crust supports structs, typedefs, static and instance methods, shared and exclusive borrows, and
`~Type()` destructors. See [examples/](examples/) for runnable programs.

Fixed-size arrays support checked indexing and resource cleanup. String literals
use UTF-8 byte arrays.
The [C owner example](examples/c_owner.cr) wraps C allocation with private fields,
checked borrows, and a destructor.
The [generic buffer example](examples/generic_buffer.cr) uses the same container
with integers, characters, and structs.

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
| `build` | Build a native executable |
| `check` | Check names, types, ownership, and borrows |
| `emit-c` | Write generated C to stdout |
| `tokens` | Print tokens and source locations |

Passing a source filename directly also compiles and runs it:

```powershell
cargo run -- examples/ownership.cr
```

## Documentation

- [Language reference](docs/language.md)
- [Projects](docs/projects.md)
- [Editor setup](docs/editors.md)
- [Compiler and development](docs/compiler.md)
- [Roadmap](docs/roadmap.md)
