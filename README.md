# Crust

An experimental C-like language with ownership checking, implemented in Rust.

Crust currently compiles a small language through C and runs the resulting
native executable. It supports functions, integer expressions, control flow,
user-defined structs with instance methods and owning fields, a built-in owning heap type, and
shared/exclusive function-argument borrows.
This is a prototype of the language design, not a complete C-compatible language.

## Getting started

Open this folder in VS Code and install the recommended **rust-analyzer**
extension (`rust-lang.rust-analyzer`). Rust and Cargo must be installed.
Building and running Crust programs also needs a C11 compiler and its standard
library/development tools. The driver defaults to Clang on Windows and `cc`
elsewhere. It accepts Clang/GCC-compatible command-line options.

Run these commands from the project folder:

```powershell
cargo run -- examples/hello.cr
cargo run -- examples/factorial.cr
cargo run -- examples/ownership.cr
cargo run -- examples/borrowing.cr
cargo run -- examples/structs.cr
cargo run -- examples/methods.cr
cargo check
cargo test
cargo fmt
```

`cargo run` builds the Rust program and runs it. Everything after `--` is passed
to the program as an argument. A bare source filename means compile and run.

| Example | Output, one value per line |
| --- | --- |
| `hello.cr` | `42` |
| `factorial.cr` | `120`, `15` |
| `ownership.cr` | `42`, `42`, `7` |
| `borrowing.cr` | `42`, `8`, `99` |
| `structs.cr` | `1`, `42`, `2`, `43` |
| `methods.cr` | `42`, `7`, `7` |

The CLI also exposes individual stages:

```powershell
cargo run -- tokens examples/hello.cr
cargo run -- check examples/ownership.cr
cargo run -- emit-c examples/borrowing.cr
cargo run -- build examples/factorial.cr
cargo run -- run examples/factorial.cr
```

`check` validates a program without a C compiler. `emit-c` writes C to stdout.
`build` writes generated C and an executable under `target/crust/`, named after
the source file. `run` builds and executes it, forwarding the low eight bits of
the program's exit code. The prototype builds one source file at a time.

Set `CC` to a compiler executable path to use another Clang/GCC-compatible driver:

```powershell
$env:CC = 'clang'
```

`CC` is an executable path, not a command string containing flags. Generated C
can also be compiled separately. On Windows, the driver uses the active MSVC
installation on `PATH` if Clang otherwise selects an obsolete installation;
this changes only the child compiler's environment.

Use `cargo run -- --help` for usage information. Missing arguments, unreadable
files, syntax errors, and semantic errors produce an error and a nonzero exit
code. Diagnostics include a source location, for example
`example.cr:2:3: unexpected character '@'`.

In VS Code, **Ctrl+Shift+B** builds the project. **Terminal > Run Task** also
offers **Crust: check** and **Crust: run example**.

## Files

- `Cargo.toml`: project name, Rust edition, and dependencies. There are no external dependencies.
- `Cargo.lock`: exact dependency resolution, kept with the executable project.
- `src/main.rs`: commands, file loading, build artifacts, and program execution.
- `src/lib.rs`: the source-to-C pipeline and frontend tests.
- `src/lexer.rs`: tokens and scanning.
- `src/diagnostic.rs`: source spans and diagnostics.
- `src/ast.rs`: the syntax tree.
- `src/parser.rs`: recursive-descent parsing and operator precedence.
- `src/check.rs`: types, scopes, ownership states, and call-argument loans.
- `src/types.rs`: struct layouts, fields, recursive-layout checks, and ownership propagation.
- `src/emit_c.rs`: C generation, checked arithmetic, and owner cleanup.
- `src/native.rs`: the external C compiler driver and Windows toolchain discovery.
- `tests/`: native execution, error cases, borrowing, and allocator-instrumented cleanup tests.
- `examples/`: runnable Crust programs.
- `.vscode/`: extension recommendations and build/run tasks.
- `target/`: generated build output, ignored by Git.

## Lexer support

- Keywords: `int`, `void`, `return`, `if`, `else`, `while`, `const`, `struct`, and `BoxInt`.
  Built-in function names such as `print`, `box_int`, and `move` tokenize as identifiers.
- ASCII identifiers starting with a letter or underscore.
- Unsuffixed decimal integer literals from `0` through `9223372036854775807`.
  The checker subsequently restricts compiled `int` values to signed 32-bit.
  A minus sign is a separate token. Leading zeros on multi-digit literals,
  integer suffixes, other bases, and floating-point literals are rejected.
- Parentheses, braces, brackets, semicolons, commas, dots, and colons.
- Operators: `= + - * / % ! < > & | ^ == != <= >= && || ->`.
- Whitespace, `//` comments, and non-nesting `/* ... */` comments.
- UTF-8 byte spans, one-based character columns, and LF/CRLF/CR line endings.
  Tabs count as one character. Unicode is accepted in comments; identifiers are
  currently ASCII only. Each successful token stream ends with `EndOfFile`.

Strings, character literals, preprocessing, and the remaining C syntax are not
implemented yet. Unsupported compound operators such as `++`, `+=`, and `<<`
are rejected instead of being split into shorter operators. This is a prototype
subset, not a complete C frontend. Source spans let the next stages report
errors without rescanning the file.

## Implemented language

- Explicit `int`, `void`, `BoxInt`, and `struct Name` function return types, named parameters,
  forward calls, and recursion. An executable needs `int main(void)` or `int main()`.
- Initialized local declarations, assignment, nested scopes, and shadowing.
  Uninitialized reads, duplicate declarations, unknown names, and wrong argument
  types/counts are rejected before emitting C.
- `return`, `if`/`else`, and `while`. Branches and loop bodies currently require
  braces. Non-void functions must return on every path. Loop analysis is conservative.
- Arithmetic, comparisons, bitwise operators, and short-circuit `&&`/`||`.
  Expression operands and call arguments evaluate left to right.
- `print(int)` writes a decimal integer and newline.
- Signed 32-bit integers, including `-2147483648`. Arithmetic overflow and
  division/remainder by zero terminate with a runtime diagnostic. Runtime
  termination does not unwind scopes; normal block exits and returns do.

## Ownership prototype

`BoxInt` temporarily stands in for a future user-defined generic owning type:

```c
int main(void)
{
    BoxInt first = box_int(42);
    BoxInt second = move(first);
    print(*second);
    return 0;
}
```

`box_int(int)` allocates an owned integer. `move(variable)` transfers ownership
and invalidates its source. Owners cannot be copied; moved owners may be
reinitialized. Owning parameters consume their argument and owning returns
transfer it to the caller. Discarded owning results are freed automatically.

Owners are dropped in reverse declaration order on normal scope exit or return.
Replacing an owner drops its previous allocation after evaluating the right
side. Internal null drop flags prevent double frees; null is not a usable
`BoxInt` value in the source language.

An owner used after a conditional move is rejected unless reinitialized. Loop
conditions cannot consume outer owners, and continuing iterations must restore
outer ownership states. These rules deliberately reject some safe programs in
this initial implementation.

## Structs and automatic field cleanup

Structs use C-style declarations and named initializers:

```c
struct Item {
    int id;
    BoxInt value;
};

int main()
{
    struct Item first = { .id = 1, .value = box_int(42) };
    struct Item second = move(first);
    print(second.id);
    print(*second.value);
    return 0;
}
```

- Struct definitions are top-level. Nested structs and forward uses of a struct
  defined later in the file are supported. Recursive by-value layouts are rejected.
- Initializers must name every field exactly once. Their expressions evaluate in
  written order. Nested brace initializers are supported, as are typed compound
  literals such as `(struct Item){ .id = 1, .value = box_int(42) }`.
- `item.id` accesses a field; `borrowed_item->id` accesses one through a borrow.
  Fields can be assigned, passed by value when copyable, and borrowed directly.
- A struct containing only copyable fields is itself copyable. Ownership
  propagates through nested structs: a struct containing `BoxInt`, directly or
  indirectly, requires an explicit whole-value move.
- Owning parameters and returns transfer whole structs. Reading any field after
  its parent has moved is rejected. Moving individual fields is not supported yet.
- Owning fields drop in reverse declaration order on normal scope exit, return,
  discard, or replacement. This cleanup is generated automatically, including
  nested owners; no user-defined cleanup hook is required.
- Fields are currently public. Field access requires a named local, parameter,
  or its pointee; accessing a field directly on a temporary return value is not
  supported yet. Empty structs, borrowed fields, and partial initialization are
  also unsupported.

A custom cleanup hook would be a user-defined destructor for resources the
compiler cannot infer, such as a file handle. Those hooks remain a future feature.

## Instance methods

Methods are defined inside structs and use an implicit receiver named `this`:

```c
struct Counter {
    int value;

    int get() const {
        return this->value;
    }

    void increment() {
        this->value = this->value + 1;
    }
};

int main() {
    struct Counter counter = { .value = 41 };
    counter.increment();
    print(counter.get());
    return 0;
}
```

- A trailing `const` gives the method a shared `const Self^ this` receiver.
  An ordinary method receives an exclusive mutable `Self^ this` instead.
  The compiler checks these permissions even if a method never writes anything.
- `Self` denotes the enclosing struct in its fields, method signatures, and
  method bodies. Use `this->field` and `this->method()` for borrowed receivers.
  Through a struct-borrow parameter, calls also use `borrow->method()`.
- Method calls automatically borrow their receiver. Receiver access is checked
  before the arguments, which evaluate left to right. The borrow lasts through
  the call, including evaluation of later arguments. Consequently,
  `counter.set(counter.get())` is rejected if `set` is exclusive; read the value
  into a local before calling `set`. Multiple shared receiver borrows may coexist.
- To take the receiver by value, explicitly declare `Self this` as the first
  parameter. For example, `int finish(Self this) { return *this.value; }` in a
  struct with a `BoxInt value` field consumes the receiver and cleans up its fields
  on return. Call it with `move(resource).finish()` for an owning struct.
  Copyable structs can be passed by value without a move, preserving the caller's
  copy. An explicit value receiver cannot also have a trailing `const` qualifier.
- Consuming methods may return `Self`, forward ownership with `return move(this);`,
  and consume temporary struct results. Their value receiver uses `this.field`.
  Borrowed methods currently require a named variable, field, or pointee as their
  receiver; borrowing temporary struct results needs temporary-lifetime support.
- Methods use static dispatch and lower to ordinary C functions. Forward method
  calls and recursion work. Method names are separate from free functions and
  from methods on other structs. Each struct permits one method per name; method
  overloading and field/method name collisions are rejected.
- Borrowed returns and borrowed local variables remain unsupported. `this` is
  reserved for receivers and cannot be shadowed by local variables or parameters.
  There is no `readonly` or `constexpr` keyword in this prototype.

## Borrowing prototype

Borrows currently exist only as function parameters and temporary call arguments:

```c
void increment(int^ value)
{
    *value = *value + 1;
}

int main(void)
{
    int value = 41;
    increment(^value);
    print(value);
    return 0;
}
```

- `int^` and `BoxInt^` are exclusive mutable borrows, created with `^place`.
- `const int^` and `const BoxInt^` are shared borrows, created with `^const place`.
- `struct Name^` and `const struct Name^` borrow structs using the same rules.
  For example, `increment(^item.id)` borrows a field and `inspect(^const item)`
  borrows its whole parent. Field loans currently lock the whole parent for the
  call, so simultaneous mutable borrows of distinct fields are conservatively rejected.
- `^*owner` borrows a boxed integer's contents; `^owner` borrows the owner slot.
- Borrow parameters can be reborrowed for nested calls. Exact parameter types
  are currently required; there is no implicit mutable-to-shared conversion.
- Multiple shared borrows may coexist. Exclusive borrows prevent conflicting
  access for the entire call, including evaluation of subsequent arguments.
- Borrowed parameters cannot be rebound, but mutable pointees can be assigned.
  Borrowing an owner slot permits replacing its allocation while preserving ownership.
- Borrowed locals and borrowed return values are rejected until lifetime
  contracts and longer-lived references are implemented.

## Not implemented yet

Custom cleanup hooks, generics, interfaces, choice types,
arrays/slices, strings, raw pointers, unsafe blocks, modules, and C header imports.
The generated C backend does not by itself provide arbitrary C interop.

Future work can build on struct ownership to add custom cleanup, general
resource wrappers, and C interop.
