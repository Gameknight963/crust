# Language reference

## Functions and control flow

- Types: integers, `bool`, `void`, `BoxInt`, struct names, and typedef names
- Functions support named parameters, forward calls, and recursion
- The entry point is `int main()` or `int main(void)`
- Local declarations require initializers
- Blocks support nested scopes and variable shadowing
- `if`, `else`, `while`, and `for` bodies use braces
- `break` exits the innermost loop, and `continue` advances to its next iteration
- `for` scopes its initializer and runs its step after each continuing iteration, including `continue`
- Loop control destroys locals in the scopes being exited
- Compound assignments: `+= -= *= /= %= &= |= ^=`
- Non-void functions must return a value on every path
- Operands and call arguments evaluate left to right
- `&&` and `||` short-circuit
- `print(value)` writes integers, booleans, or byte-array text followed by a newline

Signed arithmetic overflow and division or remainder by zero terminate the program
with a source-location diagnostic. Normal scope exits and
returns run cleanup. Runtime termination ends the process immediately.

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

Compound assignments capture their destination before evaluating the right side.
The right side can read the destination, while mutation or moves of its parent
are rejected until assignment completes.

## Integers and conversions

| Names | Representation |
| --- | --- |
| `int`, `int32_t` | Signed 32-bit |
| `int8_t`, `int16_t`, `int64_t` | Signed 8-, 16-, and 64-bit |
| `uint8_t`, `uint16_t`, `uint32_t`, `uint64_t` | Unsigned 8-, 16-, 32-, and 64-bit |
| `size_t`, `uintptr_t` | Unsigned pointer-sized integer |
| `ptrdiff_t`, `intptr_t` | Signed pointer-sized integer |

- Unsuffixed integer literals use `int`, then `int64_t`, then `uint64_t`, according to their value
- A literal assigned or passed directly to an integer type must fit that type
- Boolean and 8- or 16-bit operands promote to `int` before arithmetic
- Operands with the same signedness use the wider promoted type
- Mixed signed and unsigned operands use the signed type if it is wider, otherwise the unsigned type
- Integer conversions check that the value fits, including conversions for mixed arithmetic
- Unsigned arithmetic wraps within the promoted result type
- Compound assignments check conversion of the result back to the destination type

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
- Lengths are positive integer literals
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
- An explicitly sized byte-array destination may provide extra capacity, which is filled with zeros
- String literals support the same byte escapes as character literals
- Adjacent literals concatenate, including across whitespace and comments
- Array copies, indexing, mutation, borrows, fields, parameters, and returns use ordinary array rules
- `print` accepts a byte array or a shared or mutable borrow of one
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
- Explicit values use integer literals with an optional sign
- Multiple names can have the same value
- Values fit the underlying type, including automatically incremented values
- Values can be named directly or through the enum type or a typedef, such as `Status::Ready`
- Unscoped value names share a namespace across enum definitions and free functions
- Local variables and parameters can shadow an unscoped value name
- Comparison operands must have the same enum type
- Arithmetic and integer conversions require explicit casts
- Integer-to-enum casts check the underlying range and allow unnamed values
- `sizeof` and `alignof` follow the underlying integer type

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
- Fields are public
- Structs with copyable fields are copyable unless they define a destructor
- Owning fields and custom destructors make the enclosing struct move-only
- Ownership transfers use whole-struct moves
- Owning fields are destroyed in reverse declaration order

Field access and borrowed method receivers use a named variable, field, or
pointee, including a returned borrow. Empty structs, borrowed fields, partial initialization, and
partial field moves need further language support.

## Typedefs

```c
typedef int Count;
typedef Resource Handle;
typedef const Count^ ReadCount;
```

- Typedefs introduce names for existing types and preserve their ownership rules
- Declarations are resolved across the whole file, including forward references
- Cyclic and duplicate typedefs produce diagnostics
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
- Field borrows currently lock the whole parent
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

Borrows lock the whole parent value and remain active to the end of the block,
including after their last use. Borrowed fields require further lifetime rules.

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
- Returned references support local bindings, call arguments, dereferencing, field access, and method chaining
- A local binding retains the returned loan until its block ends
- Unbound returned loans last through the expression statement or initializer
- Condition temporaries end before entering an `if` or loop body
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

## Lexical rules

- Keywords: `int`, `bool`, `true`, `false`, `void`, `return`, `if`, `else`, `while`, `for`, `break`, `continue`, `const`, `struct`, `enum`, `BoxInt`, `typedef`, `static`, `sizeof`, `alignof`, `_Alignof`
- Identifiers use ASCII letters, digits, and underscores, starting with a letter or underscore
- `Self` is recognized as a type name inside struct definitions
- `print`, `box_int`, and `move` tokenize as identifiers
- Integer literals are unsuffixed, with a separate minus token
- Literals support decimal, `0x` hexadecimal, `0b` binary, and leading-zero octal notation
- Character literals such as `'A'` and `'\n'` produce integer constants
- Characters use ASCII or byte escapes, including `\xFF`, up to three octal digits, and the standard C single-character escapes
- The lexer accepts magnitudes through `18446744073709551615`
- Signed minimum literals such as `-2147483648` and `-9223372036854775808` are supported
- Operators: `= + - * / % ! < > & | ^ == != <= >= && || -> ::`
- `~` complements integer bits in expressions and introduces destructor declarations
- Comments use `//` or non-nesting `/* ... */`
- Whitespace supports LF, CRLF, and CR line endings
- Unicode is accepted in comments and UTF-8 string literals

Unsupported literal forms and compound operators produce diagnostics. Examples
include `42u`, floating-point literals, `++`,
and `<<`.
