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

## Preprocessing

`#define` works at file scope outside native regions too. Definitions, token
pasting, stringification, conditional compilation, and includes are handled by
the C preprocessor. An include inside a C region supplies C declarations and
native definitions. The compiler preserves native source and directives for
final C compilation. It uses the preprocessed C regions for declaration analysis.

Crust macros can expand to `Box<int>`, `move(value)`, or other Crust syntax.
The ownership checker sees the expansion. For example, `TAKE(owner)` moves
`owner`, and later use produces the ordinary moved-value diagnostic.

Project include directories, macro definitions, and debug/release selection
apply during preprocessing as well as native compilation. Macros have the
scope of one preprocessed source file. Crust `import` loads another source file
with its own preprocessing environment.

Native regions are preprocessed for declaration analysis and again during final
C compilation. Stateful extensions such as `__COUNTER__` can produce different
counts across those passes. Avoid depending on their counts across language regions.

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

Included C headers must be saved before analysis. The server reports an unsaved
header instead of analyzing its stale disk contents. C bodies do not yet have
C-specific completion, diagnostics while typing, or refactoring through Crust's
language server. The native compiler diagnoses C bodies when building.

Files without directives or project definitions retain the existing frontend
path and can be checked without invoking a native compiler or loading libclang.
The optional `import-c` binding-file generator still uses the Clang executable.
