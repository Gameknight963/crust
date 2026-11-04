# Mixed C and Crust

Crust preprocesses mixed files with the selected `CC`, discovers C declarations
through libclang, checks the expanded Crust program, and emits C for final
compilation by `CC`. Compiler archives bundle the libclang runtime. GCC can
preprocess and compile mixed programs without an installed Clang executable.

## Example

```c
#define TAKE(value) move(value)
#define FACTOR 3

#pragma crust begin_c
#include <stdint.h>

static int triple(int value) {
    return value * FACTOR;
}
#pragma crust end_c

int main() {
    print(triple(14));
    return 0;
}
```

Run [the mixed-source example](../examples/mixed.cr) with:

```text
crust run examples/mixed.cr --release
```

Direct C calls follow the same explicit caller-contract rule as Crust calls.
A C declaration does not automatically require an `unsafe` caller. Crust raw
pointer operations and indirect calls still require `unsafe`. Native C bodies
follow C rules and are checked by the C compiler.

## Standard C headers

Use the complete markers `#pragma crust begin_c` and `#pragma crust end_c`.
The compiler diagnoses shorter markers such as `#pragma begin_c` at the pragma.

[The stdio example](../examples/stdio.cr) imports `printf` from `<stdio.h>`:

```c
#pragma crust begin_c
#include <stdio.h>
#pragma crust end_c

int main()
{
    printf("hello\n");
    return 0;
}
```

Crust string literals have type `const char[N]` and convert to `const char*`
for C string parameters. The call above uses the `printf` declaration discovered
from the header. Use `char text[] = "hello"` when a C API needs a writable copy.

## Preprocessing

`#define` works at file scope outside native regions too. Definitions, token
pasting, stringification, conditional compilation, and includes are handled by
the C preprocessor. An include inside a C region supplies C declarations and
native definitions. Crust discovers native types from the expanded C regions.

Crust macros can expand to `Box<int>`, `move(value)`, or other Crust syntax.
The ownership checker sees the expansion. For example, `TAKE(owner)` moves
`owner`, and later use produces the ordinary moved-value diagnostic.

Project include directories, macro definitions, optimization profiles, sanitizer
settings, and target-kind flags apply during preprocessing. Crust imports are
included once in a shared preprocessing environment. Definitions in an imported
file affect subsequent source in the importer, following translation-unit order.

The compiler snapshots project source and header inputs. It first expands the
mixed translation unit for declaration discovery and Crust checking. It then
replays that same input with generated C appended, verifying that the original
expansion is identical. This restores header guards and macro state before the
generated runtime is processed. The final artifact contains frozen native C
and generated C, compiled as preprocessed input. Native C is never expanded a
third time, so `__COUNTER__` and other stateful macros keep the checked values.
External system headers are validated for changes between passes.

`emit-c` output from a preprocessed source begins with an explanatory marker.
To compile it directly, use `CC -x cpp-output output.c -o program`. Additional
native source files use ordinary C compilation. The Crust driver selects these
input modes automatically.

Native regions currently require file scope. To call an exported Crust function
from C, declare its ordinary C prototype in a native region and define it with
`extern` in Crust. Existing native declaration restrictions still apply to values
crossing into Crust. Export headers for native aggregate or enum signatures
currently need an explicit `extern header "native.h";` declaration so their
public header includes the original layout. See [C compatibility](c-compatibility.md).

## Editor behavior

The language server preprocesses unsaved Crust documents. Diagnostics and bound
Crust names map back to the original document. Hover and navigation can follow
imported native declarations. Renaming text produced by a macro expansion is
rejected because changing that occurrence could change its macro definition or
other expansions. Edit the macro definition explicitly.

Directive completion and hover help work independently of program analysis.
Completing `#pragma b` inserts `#pragma crust begin_c`. Header completion searches
the source directory for quoted includes, project include directories, and the
selected C compiler's system include directories. These features remain available
while the program has syntax or type errors.

Clients that forward C document changes to `crust-lsp` use unsaved included-header
snapshots. Header changes refresh the dependent Crust diagnostics.
C bodies do not yet have C-specific completion, diagnostics while typing,
or refactoring through Crust's
language server. The native compiler diagnoses C bodies when building.

Files without directives or project definitions retain the existing frontend
path and can be checked without invoking a native compiler or loading libclang.
