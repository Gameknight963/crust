# Crust library

Import the files you need using paths relative to your source file:

```c
import "../lib/box.cr";
import "../lib/vector.cr";

int main() {
    Vector<Box<int>> values = Vector<Box<int>>::create();
    values.push(Box<int>::create(42));
    print(values.at(0).get());
    return 0;
}
```

| File | Provides |
| --- | --- |
| `box.cr` | `Box<T>`, an owning heap allocation with borrowed access and consuming extraction |
| `buffer.cr` | `Buffer<T>`, a fixed-length heap buffer initialized from a copyable value |
| `vector.cr` | `Vector<T>`, a growable sequence supporting owning elements |
| `sort.cr` | `Ordered` and stable `sort<T>` for vectors |
| `result.cr` | `Result<T, E>`, borrowed and consuming payload access, and `Unit` for success without a payload |
| `allocation.cr` | Allocation errors and rejected values |
| `bytes.cr` | `ByteBuffer`, an owning byte sequence with shared and mutable views |
| `string.cr` | UTF-8 `String`, validation, checked substrings, and fallible text operations |
| `file.cr` | Owned binary streams with fallible creation, byte I/O, synchronization, and closing |
| `memory.cr` | Native allocation and termination declarations used by the containers |

Containers release their allocations on scope exit. Mutable and shared access
follow the compiler's borrow rules. Bounds errors terminate the process.

Use fallible operations to handle allocation failures:

- `Box<T>::try_create(value)` and `Buffer<T>::try_create(length, initial)`
- `Vector<T>::try_reserve(capacity)`, `try_push(value)`, and `try_insert(index, value)`

Factories return `Result<Container, AllocationFailure<T>>`. Insertion returns
`Result<Unit, AllocationFailure<T>>`. The error owns the rejected value.
Call `reason()` for `AllocError::OutOfMemory` or `AllocError::CapacityOverflow`,
then consume the error with `take_value()` to recover the input. Dropping the
error releases its value. A failed operation preserves the container's contents
and capacity. `try_reserve` returns `Result<Unit, AllocError>`.

The corresponding `create`, `reserve`, `push`, and `insert` operations terminate
on allocation failure. Both insertion forms require a valid index.

Buffers and vectors expose `view()` as `const T[]&` and `view_mut()` as `T[]&`.
Slices provide checked indexing and a read-only `length` field. Keep views in
a nested scope when the container needs to be changed afterward.

The [container project](../examples/container_project/) and
[sorting example](../examples/sorting.cr) use these implementations.

## Text and bytes

`ByteBuffer::try_copy(bytes)` copies a borrowed byte slice. Its `try_append`
preserves the buffer on allocation failure. Use `view()` for shared access and
`view_mut()` to edit arbitrary bytes.

`String::try_copy(bytes)` validates and copies counted UTF-8 bytes.
`String::try_from_bytes(move(buffer))` validates an owned buffer and returns it
on failure. `try_from_c_string(text)` copies through the first zero in a bounded
`const char[]&`, requiring a terminator within the slice.

String lengths and indices count bytes. `byte_at(index)` returns `uint8_t`.
`is_boundary(index)`, `try_truncate(length)`, and `try_substring(start, length)`
check UTF-8 character boundaries. `try_append(bytes)` validates its input before
changing the string. `view()` exposes shared bytes. Print a view with
`print(text.view())` to write its full length, including embedded zeroes.

See [the text example](../examples/text.cr).

## Files

`File::try_temporary()` returns `Result<File, FileError>`. Byte I/O, flushing,
rewinding, and closing have `try_` methods that return operation errors.
`try_read_byte()` returns `Result<FileRead, FileError>`, with `byte` and `end`
variants distinguishing data from end-of-file. Moving a file transfers ownership,
and its destructor closes the stream.

Compile `native/file.c` as a native source in the consuming target.
The [file project](../examples/file_project/) shows the configuration and the
status-based convenience methods.
