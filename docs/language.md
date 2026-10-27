# Language reference

## Imports

```c
import "containers/buffer.cr";
```

Import paths are relative to the importing file. The compiler loads each file
once, including dependencies reached through multiple paths or cycles.
Declarations share the target's global namespace and can refer forward across
files. Duplicate declarations follow the same rules as declarations in one file.

Imports are top-level declarations. Each file contains complete declarations,
and file-based commands report errors with the original filename and location.

## Functions and control flow

- Types: integers, `bool`, `void`, `BoxInt`, struct names, and typedef names
- Functions support named parameters, forward calls, and recursion
- Executable targets use `int main()` or `int main(void)` as their entry point
- Local declarations require initializers
- Blocks support nested scopes and variable shadowing
- `if`, `else`, `while`, `do`, and `for` bodies accept blocks or single statements
- `else if` chains test successive conditions and give each branch its own scope
- `break` exits the innermost loop, and `continue` advances to its next iteration
- `for` scopes its initializer and runs its step after each continuing iteration, including `continue`
- Loop control destroys locals in the scopes being exited
- Compound assignments: `+= -= *= /= %= &= |= ^= <<= >>=`
- Prefix and postfix `++` and `--` update mutable integers or raw pointers
- Non-void functions must return a value on every path
- Operands and call arguments evaluate left to right
- `&&` and `||` short-circuit
- `condition ? left : right` evaluates its condition once and then its selected branch
- `print(value)` writes integers, booleans, or byte-array text followed by a newline

Signed arithmetic overflow and division or remainder by zero terminate the program
with a source-location diagnostic. Normal scope exits and
returns run cleanup. Runtime termination ends the process immediately.
File-based compilation reports `path:line:column: runtime error: message`,
including failures in imported files.

```c
int main() {
    int total = 0;
    for (int i = 0; i < 5; i += 1) {
        if (i == 2) { continue; }
        total += i;
    }
    print(total);
    return 0;
}
```

For-loop clauses may be empty. An omitted condition continues until a `break`
or return. Header variables are scoped to the loop.

A single-statement body can contain an expression, return, loop control, or
another control-flow statement. An `else` belongs to the nearest unmatched
`if`. Local declarations in control-flow bodies require braces.

```c
if (value < 0) return 0;
for (int index = 0; index < 3; index++) print(index);
```

`do { ... } while (condition);` runs its body before testing the condition.
`continue` cleans up body locals and advances to that test. `break` exits without
evaluating the condition. Body-local names are scoped to the braces.

```c
int count = 0;
do {
    count += 1;
} while (count < 3);
```

Every path reaching the next iteration must restore any moved outer owners.
The condition can borrow outer values after body-local borrows have ended.
See [the do-while example](../examples/do_while.cr).

Compound assignments capture their destination before evaluating the right side.
The right side can read the destination, while mutation or moves of its parent
are rejected until assignment completes.

Increment and decrement evaluate their destination once. Prefix forms return
the updated value, and postfix forms return the previous value. Integer updates
use the same arithmetic and conversion checks as compound assignments.
Raw pointer updates require `unsafe` and a complete object type.

```c
int value = 40;
print(value++); // 40
print(++value); // 42
```

See [the increment and decrement example](../examples/updates.cr).

Conditional expressions bind below binary operators and associate to the right.
Integer branches use the usual common integer type. Other branches require the
same value type or compatible raw pointers, including added `const` and
`volatile` qualifiers. Both branches can return `void`. Borrowed results require
an explicit `if` and separate use of each borrow.

```c
BoxInt selected = condition ? move(first) : move(second);
```

Ownership states merge after both branches are checked. The example transfers
one owner to `selected`, and scope cleanup releases the remaining owner.
Neither `first` nor `second` can be used afterward because each may have moved.
See [the conditional expression example](../examples/conditional.cr).

## Generics

Type parameters follow the struct or function name. Uses supply explicit type
arguments, including calls to generic functions.

```c
struct Pair<T, U> {
    T first;
    U second;
    T get_first() const { return this->first; }
};

T identity<T>(T value) { return value; }

int main() {
    Pair<int, char> pair = { .first = 42, .second = 'a' };
    print(identity<int>(pair.get_first()));
    return 0;
}
```

Each used combination produces a concrete type or function. The compiler checks
its body with those types, applying the ordinary ownership and borrow rules.
For example, copying a field requires a copyable field type. A generic function
that transfers ownership can return `move(value)`.

- Generic structs support fields, instance methods, static methods, and `~Type()` destructors
- `Self` denotes the current concrete struct, and destructors use the base name, such as `~Pair()`
- Typedefs can name concrete instances, and equivalent type arguments share an instance
- Type arguments can nest, such as `Pair<Pair<int, int>, char>`
- Generic function instances can supply C callbacks through their function addresses
- Type parameters belong to their declaration and do not capture names from the caller
- Bodies are checked when instantiated, and recursive instantiation has a compiler limit

See [the generic buffer](../examples/generic_buffer.cr) for a C-backed container
with private storage, checked borrows, and automatic deallocation.

## Interfaces

Interfaces declare method requirements. A constraint such as `T: Readable`
accepts a struct with matching signatures. Parameter names can differ.

```c
interface Readable {
    int read() const;
};

static interface Factory {
    Self create(int value);
};

T make<T: Factory>(int value) {
    return T::create(value);
}

int read_value<T: Readable>(const T^ value) {
    return value->read();
}
```

- Constraints apply to generic functions and structs, with `+` joining multiple requirements: `T: Readable + Factory`
- Matching includes the return type, parameter types, receiver kind, and receiver mutability
- `Self` denotes the implementing struct in interface signatures
- Instance requirements support shared, exclusive, and explicit `Self this` value receivers
- An ordinary interface can include individual `static` method requirements
- A `static interface` makes every member requirement static, and ordinary structs can satisfy it
- Requirements are checked for each used concrete type, including members the generic body never calls
- Interface calls use the concrete methods, with the ordinary ownership and borrow rules

Interface declarations describe signatures. Generic bodies are checked with
their concrete types when instantiated. See [the interface example](../examples/interfaces.cr)
for a struct satisfying both instance and static requirements.

## Integers and conversions

| Names | Representation |
| --- | --- |
| `int`, `int32_t` | Signed 32-bit |
| `signed`, `signed int` | Signed 32-bit |
| `unsigned`, `unsigned int` | Unsigned 32-bit |
| `char` | Native C character type with the host's signedness |
| `signed char`, `unsigned char` | Signed or unsigned 8-bit |
| `short`, `short int`, `unsigned short` | Signed or unsigned 16-bit |
| `long`, `unsigned long` | Native C long integer type |
| `long long`, `unsigned long long` | Native C 64-bit long long integer type |
| `int8_t`, `int16_t`, `int64_t` | Signed 8-, 16-, and 64-bit |
| `uint8_t`, `uint16_t`, `uint32_t`, `uint64_t` | Unsigned 8-, 16-, 32-, and 64-bit |
| `size_t`, `uintptr_t` | Unsigned pointer-sized integer |
| `ptrdiff_t`, `intptr_t` | Signed pointer-sized integer |

- Unsuffixed integer literals use `int`, then `int64_t`, then `uint64_t`, according to their value
- C suffixes `u`, `l`, `ll`, `ul`, and `ull` select a native integer type before conversion to a destination
- A literal assigned or passed directly to an integer type must fit that type
- Boolean and 8- or 16-bit operands promote to `int` before arithmetic
- Integer conversion ranks follow C: `int`, then `long`, then `long long`, with fixed-width names using their native typedef's rank
- Operands with the same signedness use the higher-ranked promoted type
- Mixed signed and unsigned operands use the unsigned type if its rank is at least the signed type's rank
- Otherwise they use the signed type if it represents the unsigned range, or the unsigned counterpart of that signed type
- Integer conversions check that the value fits, including conversions for mixed arithmetic
- Unsigned arithmetic wraps within the promoted result type
- `<<` and `>>` promote each operand independently and use the promoted left operand's type for the result
- Shift counts must be nonnegative and smaller than the promoted left operand's width
- Signed left shifts multiply by a power of two with overflow checking
- Signed right shifts extend the sign, rounding negative values down
- `<<=` and `>>=` evaluate the destination once and check the result when converting back to its type
- Compound assignments check conversion of the result back to the destination type

Suffixes accept uppercase letters and either order, such as `42UL` or `42lu`.
The long-long suffix uses `ll` or `LL`. Each literal takes the first type in the
following list that can hold its value:

| Suffix | Decimal | Hexadecimal, octal, or binary |
|---|---|---|
| `u` | `unsigned int`, `unsigned long`, `unsigned long long` | Same |
| `l` | `long`, `long long` | `long`, `unsigned long`, `long long`, `unsigned long long` |
| `ul` | `unsigned long`, `unsigned long long` | Same |
| `ll` | `long long` | `long long`, `unsigned long long` |
| `ull` | `unsigned long long` | Same |

For example, `42L` keeps its native `long` type in a variadic C call.
`0xffffffffffffffffLL` has type `unsigned long long`. The decimal form
`18446744073709551615LL` exceeds its allowed signed type and is rejected.
Write the signed 64-bit minimum as `-9223372036854775807LL - 1LL`.

```c
uint64_t count = 18446744073709551615;
count += 1;
print(count); // 0

int64_t wide = 42;
uint8_t byte = (uint8_t)wide;
print(~byte);
```

C-style casts convert between integer and boolean types using the same range
checks as implicit conversions. A cast to `void` discards a value and destroys
an owning result. Borrow types require exact matches.

`sizeof(type)` and `sizeof(expression)` return the object's size in bytes.
The expression is checked without being evaluated or moving its owners.
`alignof(type)` and `_Alignof(type)` return its alignment. These queries return
`size_t` and require an object type.

Pointer-sized names follow the compiler host. Generated C checks that the native
compiler uses the same pointer width.
`char` uses the host C ABI, and generated C also checks its signedness.
Native integer widths and typedef identities are checked against the compiler's
host ABI. Specifiers can use C ordering, such as `unsigned long int` or
`long unsigned int`. `long` retains its C type when it has the same width as `int`.

## Arrays

```c
typedef int Row[3];

int sum(const Row^ values) {
    return (*values)[0] + (*values)[1] + (*values)[2];
}

int main() {
    int values[3] = { 10, 20, 30 };
    values[1] += 2;
    print(sum(^const values));
    return 0;
}
```

- Declarations attach dimensions to the name, as in `int matrix[2][3]`
- Type expressions can use `int[3]`, including `sizeof(int[3])` and `(int[3]){ 1, 2, 3 }`
- Lengths are positive integer constant expressions that fit `size_t`
- Length expressions can use enum values and integer arithmetic, including in typedefs and generic type arguments
- Brace initializers provide every element and evaluate left to right
- Nested arrays use nested brace initializers
- Arrays support locals, fields, typedefs, parameters, returns, and whole-array assignment
- Passing an array by value copies it or transfers ownership with `move`
- Shared and mutable array borrows preserve the length in their type
- Indexing checks bounds and evaluates each index once
- A constant index outside the bounds produces a compiler diagnostic
- Element borrows lock the whole parent array
- Arrays of owners require whole-array moves and destroy elements in reverse index order
- Moving an individual owning element requires further partial-move support

Indexed access starts from a named array, field, or borrowed pointee. Ordinary
assignment evaluates its right side before the destination's index expressions.
Compound assignment captures its destination and old value before evaluating the
right side. Index evaluation prevents mutation or moves of the array whose
address has been captured.

## Strings

```c
typedef uint8_t Message[16];

int main() {
    Message text = "Hello, Crust!";
    text[0] = 'h';
    print(^const text);
    print("Adjacent " "literals");
    return 0;
}
```

- String literals contain UTF-8 bytes followed by a terminating zero
- Their type is a fixed-size `uint8_t` array, including the terminating byte
- A `char` or `uint8_t` array destination may provide extra capacity, which is filled with zeros
- String literals support the same byte escapes as character literals
- Adjacent literals concatenate, including across whitespace and comments
- Array copies, indexing, mutation, borrows, fields, parameters, and returns use ordinary array rules
- `print` accepts a `char` or `uint8_t` array or a shared or mutable borrow of one
- Text output stops at the first zero or the array's capacity and appends a newline
- `sizeof` includes the terminating zero and any destination padding

Array declarations specify their capacity. For example, `uint8_t text[3] = "hi"`
holds two text bytes and the terminating zero.

## Enums

```c
enum Status : uint8_t {
    Pending,
    Ready = 4,
    Failed,
};

int main() {
    Status status = Status::Ready;
    print(status == Ready);
    print((int)status);
    return 0;
}
```

- Enum definitions appear at the top level
- `Status` and `enum Status` name the type
- Each enum has a distinct type and supports locals, fields, arrays, parameters, returns, and borrows
- The underlying integer type defaults to `int` and can be specified after `:`
- The first value defaults to zero, and subsequent values increment by one
- Explicit values use integer constant expressions with arithmetic, shifts, bitwise and logical operators, comparisons, and integer, boolean, or enum casts
- Initializers can use earlier members of the same enum and values from other enum declarations, including imported files
- Constant arithmetic uses the ordinary integer promotion, conversion, overflow, and short-circuit rules
- Multiple names can have the same value
- Values fit the underlying type, including automatically incremented values
- Values can be named directly or through the enum type or a typedef, such as `Status::Ready`
- Unscoped value names share a namespace across enum definitions and free functions
- Local variables and parameters can shadow an unscoped value name
- Comparison operands must have the same enum type
- Arithmetic and integer conversions require explicit casts
- Integer-to-enum casts check the underlying range and allow unnamed values
- `sizeof` and `alignof` follow the underlying integer type

Native C enums use `extern enum`, with a header reference providing the C tag:

```c
extern header "native.h";
extern enum NativeStatus : int32_t {
    NativeReady = 1,
    NativeError = -1,
};
```

`extern header` paths resolve relative to the declaring source file and become
C includes. Native compilation checks the enum's compatible integer type, size,
and alignment. A renamed tag uses `extern enum Status = "NativeStatus" : int32_t`.
See [C header bindings](c-bindings.md) for generated declarations.

## Booleans

```c
bool enabled = true;
bool empty = false;
```

- `bool` stores false or true, represented numerically as zero or one
- Conversion from an integer treats zero as false and nonzero values as true
- Conversion to an integer yields zero or one
- Arithmetic promotes boolean operands to `int`
- Comparisons and logical operators produce the existing integer zero/one results
- Boolean values support locals, parameters, returns, fields, typedefs, and borrows
- Borrow types match exactly, including their pointee type and access permission

## Ownership

```c
int main() {
    BoxInt first = box_int(42);
    BoxInt second = move(first);
    print(*second);
    return 0;
}
```

- `box_int(int)` allocates an owned integer
- `move(variable)` transfers a whole owning value and invalidates its source
- For ordinary copyable values, `move(variable)` copies the value and leaves its source available, allowing generic transfer functions to handle both kinds of value
- Moved variables can be reinitialized
- Owning parameters and return values transfer ownership
- Scope exits, returns, replacement, and discarded results run cleanup
- Local owners are destroyed in reverse declaration order
- Replacement evaluates the right side before destroying the previous value
- After a conditional move, further use requires reinitialization
- Loop conditions preserve outer owners, and continuing iterations restore moved outer owners

## Structs

```c
struct Item {
    int id;
    BoxInt value;
};

int main() {
    struct Item first = { .id = 1, .value = box_int(42) };
    struct Item second = move(first);
    print(second.id);
    print(*second.value);
    return 0;
}
```

- Struct definitions appear at the top level
- A struct's name can be used directly as a type. `struct Name` remains an explicit spelling
- Fields may contain other structs, including types defined later in the file
- By-value layouts must be acyclic
- Named initializers specify every field exactly once and evaluate in written order
- Nested brace initializers and `(struct Item){ ... }` compound literals are supported
- `item.id` accesses a field, and `borrowed_item->id` accesses one through a borrow
- Fields are public by default
- `private` fields can be read, written, borrowed, and initialized by methods, static factories, and destructors of their own struct
- Private fields protect the invariants used by safe wrappers around raw pointers
- Structs with copyable fields are copyable unless they define a destructor
- Owning fields and custom destructors make the enclosing struct move-only
- Ownership transfers use whole-struct moves
- Owning fields are destroyed in reverse declaration order

Field access and borrowed method receivers use a named variable, field, or
pointee, including a returned borrow. Empty structs, borrowed fields, partial initialization, and
partial field moves need further language support.

### Opaque structs

```c
struct NativeHandle;
extern NativeHandle* create_handle();
extern void destroy_handle(NativeHandle* handle);
```

- A forward declaration introduces a type whose layout is supplied by C
- `typedef struct NativeHandle Handle;` can introduce an alias for an opaque tag
- Raw pointers and borrows can refer to an incomplete struct
- A later definition completes an ordinary forward declaration across the compilation unit
- Values, fields, array elements, size queries, and pointer arithmetic require a complete object type
- Generated C preserves an opaque tag's C name for native declarations

Native tag aliases give a type a separate Crust name:

```c
struct Handle = "init";
union Storage = "move";
```

These aliases remain opaque. Their quoted names are C identifiers. Repeated
declarations of an alias must use the same native tag, and native struct, union,
and enum declarations share C's tag namespace.

The [opaque handle example](../examples/opaque_project/) wraps a C-owned counter
with private storage, methods, and move-only destruction.

## Typedefs

```c
typedef int Count;
typedef Resource Handle;
typedef const Count^ ReadCount;
```

- Typedefs introduce names for existing types and preserve their ownership rules
- Declarations are resolved across the whole file, including forward references
- Identical typedef declarations can repeat, and cyclic or conflicting declarations produce diagnostics
- A typedef takes precedence over an automatically introduced struct type name
- `struct Name` always selects the struct tag when a typedef has the same name
- Type names and value names have separate lookup rules
- Typedef declarations use `typedef type Name;` at the top level

## Borrows

```c
void increment(int^ value) {
    *value = *value + 1;
}

int read(const int^ value) {
    return *value;
}

int main() {
    int number = 41;
    increment(^number);
    print(read(^const number));
    return 0;
}
```

| Type | Creation | Access |
| --- | --- | --- |
| `int^` | `^number` | Exclusive, mutable |
| `const int^` | `^const number` | Shared, read-only |
| `BoxInt^` | `^owner` | Exclusive access to the owner slot |
| `const BoxInt^` | `^const owner` | Shared access through the owner |
| `struct Name^` | `^value` | Exclusive struct access |
| `const struct Name^` | `^const value` | Shared struct access |

- `^*owner` borrows a boxed integer's contents
- Multiple shared borrows may coexist
- Exclusive borrows prevent conflicting reads, writes, moves, and borrows
- Borrow permissions apply through fields and owning pointees
- Argument borrows remain active through the call, including later argument evaluation
- Separate struct fields can be borrowed independently
- Borrowing a field prevents conflicting access to that field or any containing value
- Borrow parameters can be forwarded or reborrowed for nested calls
- Parameters require matching borrow types. Use `^const *reference` to explicitly create a shared reborrow
- Borrow parameters and local references keep their binding and permit writes through mutable pointees
- Borrows live in function parameters, local references, and temporary call arguments

### Local references

```c
int number = 40;
{
    int^ value = ^number;
    *value += 1;
    {
        const int^ view = ^const *value;
        print(*view);
    }
    *value += 1;
}
print(number);
```

- A reference's borrow lasts until its binding's block ends
- Local references can borrow stack values, owner slots, pointees, fields, and array elements
- Initializing a reference from another reference creates a reborrow
- A mutable reborrow blocks conflicting use of its parent reference until the inner block ends
- Shared reborrows permit reads and block mutation through their parent
- Leaving an inner block restores access through the parent reference
- Calls temporarily reborrow a local reference and release that call's borrow on return
- A live reference prevents moves and replacement of its source owner
- Reference bindings cannot be rebound

Borrows distinguish separate struct fields. Array element borrows lock the whole
array. Local references remain active to the end of the block, including after
their last use. Borrowed fields require further lifetime rules.

### Borrowed returns

```c
struct Item {
    BoxInt owner;

    const int^ value() const {
        return ^const *this->owner;
    }
};
```

- A function returning a borrow requires exactly one borrowed parameter
- An instance method's implicit receiver counts as a borrowed parameter
- The returned reference must originate from that parameter or one of its fields or pointees
- A mutable borrowed return requires an exclusive borrowed input
- Return statements can forward the input through calls and local reborrows
- The caller's returned reference keeps the source borrowed
- A returned loan covers the full input region passed by the caller, even when the function returns one of its subfields
- Returned references support local bindings, call arguments, dereferencing, field access, and method chaining
- A local binding retains the returned loan until its block ends
- Unbound returned loans last through the expression statement or initializer
- Condition temporaries end before entering an `if` or loop body
- Temporary returned loans from the left operand of `&&` or `||` end before the right operand
- Temporary returned loans from a `?:` condition end before either branch
- Borrowed return contracts with multiple borrowed inputs require further lifetime syntax

Returning a reference to a local or an owned value parameter produces a compiler
diagnostic. Borrowing a temporary owner requires further temporary-lifetime rules.

## Methods

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
```

- `Self` names the enclosing struct inside its definition
- A trailing `const` gives the method a shared `const Self^ this` receiver
- An ordinary method receives an exclusive `Self^ this`
- `counter.get()` automatically borrows the receiver
- Through a borrow, use `reference->get()`
- Within borrowed methods, use `this->field` and `this->method()`
- Receiver evaluation precedes argument evaluation
- Forward calls and recursion are supported
- Method names are scoped to their struct, with one method per name
- Field and method names within a struct must be distinct
- `this` is reserved for the receiver

Receiver borrows follow the call rules above. An exclusive call such as
`counter.set(counter.get())` conflicts with the shared access in its argument.
Read the argument into a local before calling `set`.

### Value receivers

An explicit `Self this` first parameter passes the receiver by value:

```c
int finish(Self this) {
    return *this.value;
}
```

- Owning receivers use `move(resource).finish()`
- Copyable receivers pass a copy
- Value receivers use `this.field`
- Returning `move(this)` transfers the receiver to the caller
- Consuming calls can accept temporary struct results
- A value receiver uses ownership transfer instead of a trailing `const` qualifier

### Static methods

```c
struct Counter {
    int value;

    static Self create(int value) {
        return (Self){ .value = value };
    }
};

int main() {
    Counter counter = Counter::create(42);
    print(counter.value);
    return 0;
}
```

- Static methods are declared with `static` and called through `Type::method()`
- Calls can use a struct's name, a typedef naming the struct, or `Self`
- Static methods have ordinary parameters and use the same ownership and borrowing rules as free functions
- Each static method has no receiver and therefore uses neither `this` nor a trailing `const`
- Static and instance methods share the struct's member-name space
- Static factories can return owning values, including values with destructors

## Raw pointers and C calls

```c
extern int puts(const char* text);

int main() {
    char text[6] = "Crust";
    unsafe {
        puts(&text[0]);
    }
    return 0;
}
```

- `T*` is a mutable raw pointer, and `const T*` points to a read-only value
- Raw pointers support copying, fields, arrays, parameters, returns, null tests, and equality comparisons
- Integer literal zero converts to a null pointer
- `&place` creates a raw pointer inside an `unsafe` block
- Raw pointer dereferencing, casts, and external C calls require `unsafe`
- Raw pointer indexing, addition, subtraction, `+=`, and `-=` require `unsafe`
- Pointer offsets count elements and must fit `ptrdiff_t`
- Pointer subtraction requires matching object types and returns `ptrdiff_t`
- Raw pointer indexing relies on the unsafe caller to keep access within the allocation
- Pointers can be cast to other raw pointer types, booleans, or pointer-sized integers
- Borrows can be explicitly cast to raw pointers inside `unsafe`
- An implicit pointer conversion can add read-only permission or convert to a compatible `void*`
- `void*` requires a typed cast before dereferencing
- Raw pointers use ordinary value semantics and explicit resource management
- `unsafe` blocks retain ordinary scope cleanup, ownership moves, and borrow checking
- Unevaluated `sizeof` expressions check types without accessing raw memory or calling C

Unsafe code is responsible for pointer validity, alignment, aliasing, and resource
lifetimes. External declarations must match the linked C function's ABI.

Pointer-slot borrows use the full pointer type, such as `Item*^ slot = ^pointer;`
or `Item* const^ view = ^const pointer;`. The first can replace the pointer value.
The second shares access to it. Dereferencing the stored raw pointer requires
`unsafe`.

Matching `extern` declarations can repeat across imported files. Parameter names
can differ, and conflicting types or variadic signatures produce diagnostics.

An import can use a different Crust name from its C symbol:

```c
extern int native_destroy(int value) = "destroy";
```

Calls and function addresses use the named C symbol directly. Multiple Crust
names can refer to the same symbol when their signatures match. The symbol uses
a C function identifier, and the import retains its unsafe caller contract.
See [renamed imports](../examples/renamed_imports.cr) for direct and callback use.

Functions and methods can declare an unsafe caller contract:

```c
unsafe int read(int* value) {
    return *value;
}
```

Calling `read` requires an unsafe context. The function body permits unsafe
operations and retains ownership and borrow checking. Methods support the same
modifier, including `static unsafe` factories. Generic instances retain their
declaration's unsafe contract.

Safe interface requirements require safe implementations. An unsafe requirement
accepts either a safe or unsafe method. Calls use the concrete method's contract.
The entry point has a safe signature.

### Volatile access

`volatile` qualifies external C objects, raw pointer targets, and borrowed
pointees. Accesses retain the C compiler's volatile semantics through field
access, array indexing, reborrows, and returned borrows. `print` reads volatile
byte-array views one byte at a time, stopping at a zero byte or the array bound.

```c
extern volatile int status;

int read_status(const volatile int^ value) {
    return *value;
}
```

- Raw pointer conversions can add `const` and `volatile`, and removing them uses an explicit unsafe cast
- Compatible declarations and aliases of a C variable retain the same qualifiers
- Borrowed views preserve their pointee's qualifiers and follow ordinary loan rules
- Volatile storage assignment, initialization, destruction, and raw moves work on copyable values
- Qualifiers can appear before or after a type, including between C integer type words

Use a volatile pointer or borrowed view to access local storage. See
[the volatile example](../examples/volatile.cr).

### C exports

An `extern` function with a body defines a named C symbol. An optional alias
chooses its C name, using the same syntax as imports.

```c
extern int add(int left, int right) = "example_add" {
    BoxInt saved = box_int(left);
    return *saved + right;
}
```

C code can declare `int32_t example_add(int32_t left, int32_t right);` and call
it directly. Local owners receive normal automatic cleanup before the function
returns. See [the export project](../examples/export_project/) for a separate C
source file calling a Crust definition.

- Export parameters are named and use a fixed parameter list
- Parameters and results follow the C import rules for copyable values and raw pointers
- Compatible prototypes and renamed imports can refer to the same definition
- Each C symbol has one definition, with matching signatures and calling conventions across aliases
- `extern` establishes an unsafe boundary, including calls from Crust and raw-pointer operations in the body
- Rename preserves the exported C name through an explicit alias

## Raw storage

`init(pointer, value)` initializes storage through a mutable raw pointer, transferring
the value into that location. It evaluates the pointer before the value and skips
cleanup of any previous contents. The caller provides valid, aligned, writable
storage with no live owning value to replace.

`destroy(pointer)` runs the pointed-to value's destructor and field or array cleanup.
It leaves the storage allocated. The caller provides a valid initialized value
and treats it as uninitialized afterward. Copyable values have trivial cleanup.

Both operations require an unsafe context and a pointer to a complete object type.
`move(*pointer)` and `move(pointer[index])` can transfer a whole value out of raw
storage in an unsafe context. Moving an owner requires a mutable pointer and
leaves that location uninitialized. The caller updates any initialization count
and avoids destroying that location until it is initialized again. Moving a
copyable value reads a copy and leaves the source initialized.

They let a generic owner pair C allocation with Crust cleanup:

```c
T* data = (T*)malloc(sizeof(T));
if (data == 0) { abort(); }
init(data, move(value));
// The owner's destructor later calls:
destroy(data);
free(data);
```

See [the generic box](../examples/generic_box.cr) for the complete owner, including
private storage and safe borrowed access.
The [growable vector](../examples/container_project/src/vector.cr) transfers
elements between allocations during growth and destroys its live elements in
reverse order. Its methods use checked borrows.

- `insert(index, value)` accepts positions from zero through `count()`, moves later elements to the right, and consumes an owning argument
- `remove(index)` requires an existing element, moves it out, and closes the gap while preserving order
- `clear()` destroys elements in reverse order and retains the allocation for reuse
- `swap(first, second)` exchanges two existing elements through whole-value moves
- Element borrows prevent insertion, removal, clearing, growth, and moves of the vector

Insertion and removal take time proportional to the number of shifted elements.
The [vector edit example](../examples/vector_edits.cr) uses these operations with
heap owners.

The [sorting example](../examples/sorting.cr) uses a generic stable insertion sort
from [sort.cr](../examples/container_project/src/sort.cr). Its `Ordered` interface
requires `int compare(const Self^ other) const`. The sort borrows vector elements
for comparison and uses `swap` to exchange owners. It uses constant extra storage,
takes quadratic time in the worst case, and runs in linear time on sorted input.

`Box<T>::take(Self this)` consumes a box and returns its stored value.

`extern` declares a C function under its native symbol name. Parameters can be
named or unnamed. Signatures use C values and raw pointers. Arrays cross the
boundary by pointer. The [C owner example](../examples/c_owner.cr) uses `malloc`
and `free` behind private fields, checked getters, and a destructor.
The [integer buffer example](../examples/int_buffer.cr) adds checked allocation
sizes, initialized elements, bounds-checked getters, and move-only cleanup.

For a shared borrow of a raw pointer binding, use a typedef such as
`typedef int* IntPointer;` followed by `const IntPointer^`.

## External variables

```c
extern int count;
extern const int limit;
extern int local_name = "native_name";
```

- External variables refer to storage provided by C
- Reads, writes, addresses, and borrows require `unsafe`
- `sizeof` can inspect an external variable without evaluating it
- `const` qualifies the imported object, including `extern int* const pointer`
- External variables use complete copyable C values or raw pointers
- Compatible declarations can repeat, and renamed imports can share a native symbol
- Borrows through aliases of the same C symbol overlap
- Function-local declarations can shadow an imported variable

The [native globals project](../examples/global_project/) imports mutable storage,
a const object, and a callback variable from a separate C file.

On Windows, DLL declarations can use `__declspec(dllimport)`:

```c
extern __declspec(dllimport) int count;
extern __declspec(dllimport) const int limit;
extern __declspec(dllimport) int increment();
```

The attribute can precede `extern` or follow it. Compatible declarations and aliases
of a native symbol share its DLL import storage. Add the DLL's import library to
the target's `native.link-inputs` and place the DLL beside the executable.
This uses the native [DLL data import convention](https://learn.microsoft.com/en-us/cpp/build/importing-data-using-declspec-dllimport).

## Function pointers and callbacks

```c
typedef int (*Callback)(int value);

int increment(int value) { return value + 1; }

int main() {
    Callback callback = increment;
    unsafe { print(callback(41)); }
    return 0;
}
```

- `typedef int (*Callback)(int);` names a function pointer type
- `typedef int Operation(int);` names a function type, and `Operation*` points to it
- Callback signatures use copyable C values and raw pointers
- A function name or `&function` supplies its address
- Static methods supply addresses through `Type::method`, typedefs, and `Self`
- Function addresses can be copied and passed through locals, fields, arrays, parameters, and returns
- Calls through function pointers require `unsafe`
- Calls accept `callback(args)`, `(*callback)(args)`, and callback fields such as `handler.apply(args)`
- The callee evaluates before its arguments, and every part evaluates once
- Literal zero creates a null function pointer, and equality and null tests use ordinary pointer rules
- Function types have signatures, while pointer values have object size and alignment

The [native callback example](../examples/callback_project/) passes Crust functions
to C and calls a function pointer returned by C. Callers of indirect functions
are responsible for a valid function pointer and the target's unsafe requirements.

## Calling conventions

```c
typedef int (__vectorcall *Callback)(int);
extern int __vectorcall apply_native(Callback callback, int value);
int __vectorcall increment(int value) { return value + 1; }
```

- Place a calling convention after the return type or inside a function-pointer declarator before `*`
- `__cdecl` selects the ordinary C convention and is accepted on every target
- Windows targets also support `__stdcall`, `__fastcall`, and `__vectorcall`
- `__vectorcall` requires x86 or x64 and a fixed parameter list
- On x64 and ARM, `__stdcall` and `__fastcall` share the ordinary C ABI
- Variadic `__stdcall` and `__fastcall` declarations use the C variadic ABI
- Function pointers, repeated C declarations, and interface requirements compare the effective convention
- Ordinary functions, generic functions, and methods can declare a convention

The Windows [calling-convention project](../examples/calling_convention_project/)
passes callbacks in both directions using separate Crust and C sources.
These rules follow the native conventions documented for
[`__stdcall`](https://learn.microsoft.com/en-us/cpp/cpp/stdcall) and
[`__vectorcall`](https://learn.microsoft.com/en-us/cpp/cpp/vectorcall).

## Destructors

```c
struct Resource {
    int handle;

    ~Resource() {
        print(this->handle);
    }
};
```

- A destructor matches its enclosing struct's name and has an exclusive `Self^ this` receiver
- Each struct permits one destructor with an empty parameter list
- Destructors use the `~Type()` declaration syntax
- Normal cleanup invokes the destructor before destroying owning fields
- `return;` ends the destructor body, then automatic field cleanup continues
- Custom destructors make their struct and enclosing structs move-only
- Moves transfer responsibility for destruction
- Explicit destructor calls are rejected
- `drop` is an ordinary method name

## Unions

```c
union Value {
    int integer;
    uint64_t wide;
};

int main() {
    unsafe {
        Value value = { .integer = 42 };
        print(value.integer);
        value.wide = 7;
        print(value.wide);
    }
    return 0;
}
```

- Union fields share storage and use the native C union layout
- Initializers select exactly one field
- Initializing a union and accessing, assigning, or borrowing a field require an unsafe context
- The caller ensures the selected field contains a valid value of its type
- The borrow checker treats union fields and their nested fields as overlapping
- Fields contain copyable values or raw pointers
- Whole union values copy normally, and a custom destructor makes the union move-only
- Unions support typedefs, arrays, generics, methods, raw pointers, and C-compatible calls
- `union Name;` declares an opaque C union, and `struct` and `union` tags retain their distinct kinds

`sizeof` and `alignof` use the union's native layout. A wrapper can keep its union
private and expose safe methods that track the selected field. See
[the union example](../examples/unions.cr).

## Variadic C calls

External declarations and function pointer typedefs can end their parameter list
with `...`, following at least one fixed parameter:

```c
extern int snprintf(char* destination, size_t capacity, const char* format, ...);
typedef int (*Formatter)(char*, size_t, const char*, ...);
```

Calls check the fixed parameters and apply C's default promotions to additional
arguments. Booleans, `char`, and 8- or 16-bit integers promote to `int`. Enums
promote according to their underlying integer representation. Wider integers,
raw pointers, and copyable C structs retain their types.

Additional arguments must be C values or raw pointers. Arrays require an explicit
pointer, and owners and borrows require a suitable C representation. Calls retain
left-to-right evaluation and require an unsafe context. The caller supplies the
types expected by the C function's argument protocol after promotion.

Variadic and fixed function pointer signatures are distinct types. See
[the variadic example](../examples/variadic.cr) for direct and callback calls to
`snprintf`.

## Lexical rules

- Keywords: `int`, `char`, `signed`, `unsigned`, `short`, `long`, `bool`, `true`, `false`, `void`, `return`, `if`, `else`, `while`, `do`, `for`, `break`, `continue`, `const`, `volatile`, `struct`, `union`, `enum`, `interface`, `import`, `init`, `destroy`, `BoxInt`, `typedef`, `static`, `extern`, `__declspec`, `__cdecl`, `__stdcall`, `__fastcall`, `__vectorcall`, `unsafe`, `private`, `sizeof`, `alignof`, `_Alignof`
- Identifiers use ASCII letters, digits, and underscores, starting with a letter or underscore
- `Self` is recognized as a type name inside struct definitions
- `print`, `box_int`, and `move` tokenize as identifiers
- Integer literals are unsuffixed, with a separate minus token
- Literals support decimal, `0x` hexadecimal, `0b` binary, and leading-zero octal notation
- Character literals such as `'A'` and `'\n'` produce integer constants
- Characters use ASCII or byte escapes, including `\xFF`, up to three octal digits, and the standard C single-character escapes
- The lexer accepts magnitudes through `18446744073709551615`
- Signed minimum literals such as `-2147483648` and `-9223372036854775808` are supported
- Operators: `= + - ++ -- * / % ! < > & | ^ << >> == != <= >= && || ? : -> ::`
- `~` complements integer bits in expressions and introduces destructor declarations
- `...` marks the variable argument tail in C declarations and function pointer types
- Comments use `//` or non-nesting `/* ... */`
- Whitespace supports LF, CRLF, and CR line endings
- Unicode is accepted in comments and UTF-8 string literals

Unsupported literal forms produce diagnostics. Examples include `42u` and
floating-point literals.
