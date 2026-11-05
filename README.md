Crust (C-Rust get it) is to Rust (the language) as Crust is to Rust (the game)

# Crust
[![CI](https://github.com/Gameknight963/crust/actions/workflows/ci.yml/badge.svg)](https://github.com/Gameknight963/crust/actions/workflows/ci.yml)
![GitHub Repo Views](https://gitviews.com/repo/gameknight963/crust.svg?style=flat&label-color=orange)

A C-like language with ownership checking, written in Rust. The compiler generates
C and builds native executables, static libraries, and shared libraries.

## Goals

- Familiar to C, easy to pick up for C/C++ programmers, 
- Simple like C
- Safe like Rust
- Automatic resource cleanup with destructors (RAII)
- Very good C interop

## Example

```cpp
import <std/box.cr>;

void increment(int& value) {
    value += 1;
}

int main() {
    Box<int> first = Box<int>::create(42);
    Box<int> second = move(first);
    increment(second.get_mut());
    print(second.get());
    return 0;
}
```

Crust supports structs, typedefs, static and instance methods, shared and exclusive borrows, and
`~Type()` destructors. Safe `T&` references act as aliases and use `&value` to borrow.
See [examples/](examples/) for runnable programs.

Fixed-size arrays support checked indexing and resource cleanup. String literals
use const native `char` arrays containing UTF-8 and work with C string APIs.

[Floating-point numbers](examples/floats.cr) support `float`, `double`, and
`long double`, including C calls and checked numeric conversions.

## Try it now

Get the latest nightly releases from here. These are all zipped for now, Linux downloads might use tar in the future.

[Windows compiler](https://nightly.link/Gameknight963/crust/workflows/ci/main/compiler-windows-latest-clang.zip) <br>
[macOS compiler](https://nightly.link/Gameknight963/crust/workflows/ci/main/compiler-macos-latest-clang.zip) <br>
[Linux compiler, Clang](https://nightly.link/Gameknight963/crust/workflows/ci/main/compiler-ubuntu-latest-clang.zip) <br>
[Linux compiler, GCC](https://nightly.link/Gameknight963/crust/workflows/ci/main/compiler-ubuntu-latest-gcc.zip) <br>

These zips will contain everything you need to get started:

```
crust-0.1.0-x86_64-pc-windows-msvc/
├── bin/                 # Add bin/ to PATH
│   ├── crust.exe
│   ├── crustc.exe
│   ├── crust-lsp.exe
│   └── libclang.dll
├── lib/                 # Standard library
├── examples/
├── docs/
├── editors/
├── crustc/              # Compiler source
├── tools/
├── third-party/
├── Cargo.toml
├── Cargo.lock
├── README.md
└── manifest.json        # Version and file checksums
```

Additionally, for editor support:

[Visual Studio extension](https://nightly.link/Gameknight963/crust/workflows/ci/main/visualstudio-extension.zip)
<br>
[VS Code extension](https://nightly.link/Gameknight963/crust/workflows/ci/main/vscode-extension.zip)

## Read some examples

#### Ownership and containers

- [Library](lib/): reusable boxes, buffers, vectors, and sorting
- [Slices](examples/slices.cr): borrowed views of arrays and containers
- [Results](examples/results.cr): scoped enums, tagged unions, and explicit error handling
- [Text](examples/text.cr): UTF-8 strings, byte views, and checked substrings
- [C allocation](examples/c_owner.cr): private storage, checked borrows, and a destructor
- [Generic buffer](examples/generic_buffer.cr): one container for integers, characters, and structs
- [Generic box](examples/generic_box.cr): heap values, including other owners
- [Container project](examples/container_project/): buffers, boxes, and vectors across imported files
- [Vector edits](examples/vector_edits.cr): insertion, removal, and cleanup
- [Sorting](examples/sorting.cr): a comparison interface and a vector of heap owners
- [File resource](examples/file_project/): binary I/O, ownership transfer, and automatic closing
- [File statistics](examples/crustwc/): a multi-file CLI with UTF-8 validation and recoverable errors

#### C integration

- [Headers](examples/header_project/): direct header inclusion and an opaque handle wrapper
- [Header-only C](examples/header_only_project/): static functions, packed records, and bitfields
- [Native records](examples/records_project/): complete C types, packed-field borrows, and by-value calls
- [SQLite](examples/sqlite_project/): native database owners, prepared statements, and recoverable SQL errors
- [C algorithms](examples/c_algorithms.cr): sorting and searching through standard library callbacks
- [Volatile access](examples/volatile.cr): qualified pointers and borrowed views
- [Mixed C and Crust](examples/mixed.cr): ordinary preprocessing and inline C functions
- [Exports](examples/export_project/): a C source file calling a named Crust function
- [Static library](examples/library_project/): a native archive linked into a C consumer
- [Shared library](examples/shared_project/): a C export linked into a Crust executable
- [Target links](examples/linked_project/): automatic library builds and native linking
- [Calling conventions](examples/calling_convention_project/): `__vectorcall` callbacks on Windows x86 and x64

## Development

Install Rust and Cargo, plus a C11 compiler.
The compiler driver defaults to Clang on Windows and `cc` elsewhere.
For [mixed-source builds](docs/mixed-source.md) from a checkout, install libclang
or set `CRUST_LIBCLANG` to its library file. Compiler archives bundle it.

```
cargo build --bins
cargo run --bin crustc -- examples/hello.cr -o target/hello
cargo run -- examples/methods.cr
cargo run -- examples/destructors.cr
cargo run -- run --project examples/project
```

To select a C compiler, set `CC` to its executable path:

```
$env:CC = 'clang'
```

## Commands

Use `--release` with `build` or `run` to optimize generated programs with native
`-O2`. Builds default to `-O0`. `--debug` selects `-O0` with debug symbols.
Project flags apply to linked project dependencies too.

`crust` (not `crustc`) is the project driver. Use `crust` for your projects.

| Command | Action |
| --- | --- |
| `run` | Compile and run |
| `build` | Build executables and libraries |
| `check` | Check names, types, ownership, and borrows |
| `emit-c` | Write generated C to stdout |
| `emit-header` | Write C declarations for named exports to stdout |
| `tokens` | Print tokens and source locations |
| `metadata` | Write project targets and output paths as JSON |

`Crust` can also compile and runs individual source files:

```
crust run examples/ownership.cr
```

For direct compilation, use `crustc`:

```
crustc examples/hello.cr -o target/hello
```

`crustc` builds the output without running it. It also supports several options:

```
crustc --check hello.cr
crustc --emit-c hello.cr
crustc --emit-header library.cr --kind static-library
crustc --tokens hello.cr
```

## Documentation

- [Language reference](docs/language.md)
- [Evaluation and lifetimes](docs/semantics.md)
- [Native ABI](docs/abi.md)
- [Projects](docs/projects.md)
- [C header bindings](docs/c-bindings.md)
- [C compatibility](docs/c-compatibility.md)
- [Editor setup](docs/editors.md)
- [Compiler and development](docs/compiler.md)
- [Roadmap](docs/roadmap.md)
