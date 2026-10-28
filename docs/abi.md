# Native ABI

Crust generates C11 and uses the native C compiler's calling convention and
object layout. Generated code checks the host pointer width, C integer widths,
`char` signedness, and the native identities of fixed-width and pointer-sized
integer aliases. The native compiler target must match the Crust compiler host.

## Values and layout

| Crust value | Generated representation |
| --- | --- |
| Native and fixed-width integers | Corresponding C scalar type |
| `bool` | C `bool` |
| `float`, `double`, `long double` | Corresponding C floating type |
| Source enum | Typedef of its declared integer representation |
| Imported native enum | Native C enum tag, with representation and layout checks |
| Raw pointer | C pointer with preserved pointee qualifiers |
| `T&`, `const T&` | Pointer to the referent, with shared access represented by a const pointee |
| `T[N]` | Struct containing a contiguous C array of `N` elements |
| `T[]&`, `const T[]&` | Struct containing an element pointer and a `size_t` length |
| Ordinary struct | C struct with fields in declaration order |
| Ordinary union | C union with overlapping fields |
| Tagged union | C struct containing a discriminator and an anonymous payload union |
| Function pointer | Native C function pointer with the declared convention |

The native compiler determines padding and alignment. Generic instances have
concrete layouts after substituting their type arguments. Typedefs preserve the
aliased type's identity and layout.

Ownership and local initialization flags live in generated function state.
They add no fields to an ordinary struct. A tagged union stores its active
variant in the object. Its discriminator encoding and generated member names
belong to the compiler implementation.

Fixed arrays pass by value through their wrapper struct. References carry their
length in the referent type. Slices carry a runtime length, and their access
permissions are checked by Crust. Slice construction from raw pointers requires
the unsafe caller to provide valid, appropriately aligned storage of that length.

## Native interfaces

Quoted imports and exports choose a C-level name:

```c
extern int native_add(int left, int right) = "library_add";

extern int answer() = "crust_answer" {
    return 42;
}
```

The native compiler applies platform symbol decoration and calling conventions.
The [header generator](projects.md#c-headers) writes the exported signatures for
C and C++ consumers. It supports scalars, enums, callbacks, and pointers to
opaque C tags. Complete aggregate header layouts require further importer and
header-generation support.

Native imports and exports require copyable C values or raw pointers. Owners,
references, slices, and by-value fixed arrays use Crust's source-level interfaces.
Use pointer-based wrappers for native APIs that manage those values.

Native function calls require `unsafe`. Callback implementations must satisfy
the native caller's validity, aliasing, ownership, and synchronization contract.
Borrow checking covers the Crust side of the boundary.

Variadic calls apply C integer promotions and promote `float` to `double`.
Pointer-sized and same-width integer types retain their native C rank where
promotions leave them unchanged. See [calling conventions](language.md#calling-conventions).

## Methods and generic code

Borrowed instance methods take their receiver as the first pointer parameter.
A `const` method receives shared access. A value receiver passes the concrete
struct by value and transfers ownership when the type owns resources. Static
methods have their explicit parameters only.

Generic functions and structs are instantiated within the importing target.
Interface constraints resolve member calls during checking and code generation.
They introduce no runtime dispatch table.

## Linkage and generated names

Ordinary functions in a library target have internal C linkage. File-local
`static` functions also have internal linkage. Source globals and function-local
static objects use internal storage symbols. Explicit C exports provide the
native API between independently compiled targets.

| Declaration | Generated C name |
| --- | --- |
| Ordinary function `name` | `crust_fn_name` |
| File-local function | `crust_local_<file-index>_<name>` |
| Generic function instance | `crust_generic_<instance-index>_<name>` |
| Instance method | `crust_method_<struct-id>_<name>` |
| Static method | `crust_static_<struct-id>_<name>` |
| Destructor | `crust_destructor_<struct-id>` |
| Public Crust global `name` | `crust_global_name` |
| File-local global | `crust_global_local_<file-index>_<name>` |
| Function-local static | `crust_global_local_scope_<source-offset>_<name>` |
| Program entry | `main` |

Generic names include a file-local key when their declaration is static.
Generated type, pointer, array, callback, local, and temporary names also use
compiler-assigned IDs. These IDs describe one compilation and can change when
declarations or instantiations change. Quoted C names define the maintained
native interface.

Source `main()` and `main(void)` both emit a C `main(void)`. The supported
argument-taking entry point emits the native `argc` and `argv` signature.
