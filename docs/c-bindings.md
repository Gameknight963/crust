# C headers and native APIs

Include C headers inside a [C region](mixed-source.md). The selected `CC` handles
ordinary preprocessing, and bundled libclang supplies the declarations used by
Crust code.

```c
#pragma crust begin_c
#include "native.h"
#pragma crust end_c

int main() {
    print(native_add(20, 22));
    return 0;
}
```

The compiler discovers declarations during each build. A shared `bindings.cr` file can contain the
C region above and be imported by other Crust files. The [header example](../examples/header_project/)
wraps an opaque C handle in a move-only struct with a destructor.

## Declarations

Discovery follows names referenced by Crust and their type dependencies:

- Native integer, boolean, floating-point, and void types
- Raw pointers with const and volatile qualifiers and multiple pointer levels
- Scalar, pointer, callback, and fixed-array typedefs
- Opaque struct and union tags and complete native records
- Named enums with their native representations and values
- Functions, callbacks, callback returns, variadic arguments, and native calling conventions
- Static functions, inline definitions, and header-local storage
- Mutable and const external variables, including Windows DLL imports
- Packed and aligned records, arrays, qualified fields, anonymous groups, bitfields, and flexible tails

The native compiler supplies layouts and calling conventions. Emitted assertions
check the types and layouts described to Crust. Names reserved by Crust receive
a `c_` prefix. Collisions append underscores, preserving the original C symbol.
For example, a native function named `destroy` is available as `c_destroy`.

Direct C calls follow explicit caller contracts. A C declaration does not
implicitly require `unsafe`. Raw pointer operations, external storage access,
and indirect calls still require `unsafe`. Native bodies follow C rules.

Crust function declarations take precedence over automatic function discovery.
An explicit `unsafe extern` declaration can give a native function an unsafe
caller contract.

C array parameters adjust to pointers. Their minimum lengths, aliasing rules,
and allocation contracts remain the API author's responsibility. Unsupported
declaration types require a C wrapper with a representable signature.

## Macros

Macros expand in both Crust and C regions, following ordinary C preprocessing:

```c
#pragma crust begin_c
#include "native.h"
#pragma crust end_c

#define TAKE(value) move(value)

int main() {
    int values[API_CAPACITY] = {0};
    print(values[0]);
    return 0;
}
```

Object and function macros can supply Crust expressions, types, and constant
expressions when their expansion is valid Crust syntax. Macro names also expand
in field names and type names. Use `#undef` when the header's macro conflicts
with the spelling needed in subsequent source. Stateful macros such as
`__COUNTER__` retain their values through checking and final compilation.

Compiler-specific macro expansions must also be valid Crust. For example, Darwin's
fortified `memcpy` macro uses compiler intrinsics. `(memcpy)(destination, source,
length)` selects the ordinary C function, following C macro rules. A wrapper in
a C region can use the fortified macro and expose a typed function to Crust.

## Linking and exports

Configure C sources, include directories, definitions, and libraries through
[project native options](projects.md). Each target has one translation unit for
its Crust imports. Additional native C source files have their own translation
units and header-local storage, following C linkage rules.

For an existing DLL, put its import library in `link-inputs` and its DLL in
`runtime-libraries`. Runtime files propagate through linked targets.

To expose native records or enums through an exported Crust function, add
`extern header "native.h";`. The generated public header includes the original
layout. Handwritten native declarations and aliases remain available for
explicit interfaces. For example:

```c
extern header "native.h";
extern struct Local = "NativeRecord" { int c_init = "init"; };
```

## Native records

The original header supplies size, alignment, packing, and by-value ABI.
Fixed arrays support copies, assignment, checked indexing, and array references.
Const fields permit initialization and shared borrows. A record with const
fields supports fresh copies and factory returns, while whole-value assignment
is rejected. Other fields remain writable. Volatile fields preserve their
access qualifiers through reads, writes, and borrows.

Const and volatile fields require copyable values. Source initialization of a
const native array is limited to 4096 elements. Larger arrays can come from a
native factory. Zero-length C arrays use native accessors.

## Flexible native arrays

A final flexible member such as `int values[]` preserves the record's native
prefix layout. Reading the member produces a raw element pointer. A const record
reference produces a const element pointer. Multidimensional tails preserve the
native row size and stride.

The pointer carries no allocation length. Raw indexing and `slice(pointer, length)`
require `unsafe` and valid storage for the requested range. Slice construction
also requires aligned, non-volatile element storage. The caller supplies the
actual allocated length. A count field is ordinary data and is not validated
against the allocation by the compiler.

`sizeof(Record)` reports the native prefix size. Copying a record copies its
prefix, and trailing storage remains managed by the allocation owner. These records
and unions containing them cannot be array elements or embedded struct members. Record
initializers initialize prefix members. They cannot initialize the flexible tail.
The tail has no pointer slot to assign or borrow, and `sizeof(record.values)` is
rejected. Use the raw member pointer directly to construct a view.

## Anonymous native groups

Included records preserve anonymous struct and union groups. Their named members
are accessible directly on the containing record, as in C. Initializers select
one variant of each anonymous union and initialize the fields of that variant.
Nested anonymous struct variants can initialize several members together.

Reading or borrowing a promoted union member requires `unsafe`. A borrow of one
member protects the entire anonymous union from conflicting access. Promoted
members of an anonymous struct retain ordinary disjoint-field borrowing rules.
Native union members require copyable values or raw pointers.

Named fields whose type is an anonymous aggregate still require a native wrapper.

## Native bitfields

Included records import named integer, boolean, and enum bitfields using C width
syntax, such as `unsigned int flags : 3;`. The original header supplies layout,
including unnamed padding fields. Reads, assignments, compound assignments, and
updates use native member access and preserve volatile qualifiers. Integer fields
narrow enough to fit `int` undergo integer promotion when read.

Stores check the declared width before writing. An out-of-range dynamic value
reports a runtime error. Static initializers require fitting integer constants.
Bitfields have no address, so raw addresses, reference bindings, and `sizeof` on
a bitfield expression are rejected. Header declarations are read again on each build, including bitfield widths and
signedness.

## Native macro wrappers

A typed header function can expose a function-like macro, string macro, or
floating macro. For example:

```c
#define API_DOUBLE(value) ((value) + (value))
#define API_NAME "example"
#define API_SCALE 1.5L

static inline int api_double(int value) { return API_DOUBLE(value); }
static inline const char *api_name(void) { return API_NAME; }
static inline long double api_scale(void) { return API_SCALE; }
```

Include the header in a C region. The native compiler expands its macros, and
Crust discovers the wrapper signatures. The same pattern exposes native atomic
and thread-local storage through accessor functions.

## Other native wrappers

The executable [wrapper fixture](../crustc/tests/fixtures/c-wrappers.h) contains the C
implementations used by `native_header_wrappers_cover_extended_c_storage_types_arrays_and_macros`
in the [binding tests](../crustc/tests/bindings.rs). Each public function has a signature
the importer can represent.

| C facility | Tested wrapper | Caller contract |
| --- | --- | --- |
| Atomic storage | `feature_atomic_add` | The native function chooses relaxed ordering. A real synchronization API must choose ordering for its protocol |
| Thread-local storage | `feature_thread_next` | Access belongs to the calling thread. Do not retain a TLS address across thread exit |
| Complex arithmetic | `feature_complex_sum` | Exchange real and imaginary components through scalar parameters |
| Extended integers | `feature_wide_carry` | Exchange fixed-width limbs or checked conversions. The fixture requires compiler support for `__int128` |
| Variable-length arrays | `feature_variable_array` | Supply a positive length that fits the native stack budget |
| Pointer-to-array parameters and returns | `feature_rows`, `feature_row` | Supply contiguous initialized rows with the stated width. Returned static storage is shared and must not be freed |
| Named fields with anonymous aggregate types | `feature_anonymous` | Keep the anonymous type inside the native function or an opaque owner |
| Assembly and compiler extensions | `feature_extension` | Compile the wrapper with the toolchain that implements the extension |
| Function, string, and floating macros | `feature_twice`, `feature_text`, `feature_scale` | Macro expansion happens in C with the selected header configuration. The returned string has static storage |

For example, a pointer-to-array API can expose a flat pointer with an explicit
row count. Keep the array cast inside C:

```c
static int feature_rows(const int *elements, size_t rows) {
    return feature_rows_native((const int (*)[3])elements, rows);
}
```

Crust calls this as `feature_rows(&elements[0], 2)` for an initialized six-element
array. The caller must establish sufficient storage and a valid row count before
the call. A const pointer controls writes through that pointer. It does
not establish ownership, lifetime, or a promise that a C function will not free
the allocation.

Wrappers around allocation should check size arithmetic and return allocation
failure through their public API. The fixture's small fixed test inputs use
abort on allocation failure. Production wrappers should preserve the owning
value on failure when retry is possible. See the [SQLite example](../examples/sqlite_project)
for an owning wrapper with explicit close and error paths.
