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
| `result.cr` | `Result<T, E>` with success and error payloads, borrowed access, and consuming extraction |
| `allocation.cr` | Allocation errors, rejected values, and `Unit` for successful operations without a payload |
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
