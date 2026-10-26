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
- `print(value)` writes any integer or boolean as a decimal integer and newline

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

- Unsuffixed decimal literals use `int`, then `int64_t`, then `uint64_t`, according to their value
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

Field access and borrowed method receivers currently require a named variable,
field, or pointee. Empty structs, borrowed fields, partial initialization, and
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
- Borrows remain active through the call, including later argument evaluation
- Field borrows currently lock the whole parent
- Borrow parameters can be forwarded or reborrowed for nested calls
- Parameters require matching borrow types. Use `^const *reference` to explicitly create a shared reborrow
- Borrow parameters keep their binding and permit writes through mutable pointees
- Borrows currently live in function parameters and temporary call arguments

Borrowed locals, fields, and return values require future lifetime rules.

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

- Keywords: `int`, `bool`, `true`, `false`, `void`, `return`, `if`, `else`, `while`, `for`, `break`, `continue`, `const`, `struct`, `BoxInt`, `typedef`, `static`, `sizeof`, `alignof`, `_Alignof`
- Identifiers use ASCII letters, digits, and underscores, starting with a letter or underscore
- `Self` is recognized as a type name inside struct definitions
- `print`, `box_int`, and `move` tokenize as identifiers
- Decimal literals are unsuffixed, with a separate minus token
- The lexer accepts magnitudes through `18446744073709551615`
- Signed minimum literals such as `-2147483648` and `-9223372036854775808` are supported
- Operators: `= + - * / % ! < > & | ^ == != <= >= && || -> ::`
- `~` complements integer bits in expressions and introduces destructor declarations
- Comments use `//` or non-nesting `/* ... */`
- Whitespace supports LF, CRLF, and CR line endings
- Unicode is accepted in comments

Unsupported literal forms and compound operators produce diagnostics. Examples
include leading-zero integers, `0xff`, `42u`, floating-point literals, `++`,
and `<<`.
