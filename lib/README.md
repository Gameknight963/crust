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
| `memory.cr` | Native allocation and termination declarations used by the containers |

Containers release their allocations on scope exit. Bounds errors and allocation
failures terminate the process. Mutable and shared access follow the compiler's
borrow rules.

The [container project](../examples/container_project/) and
[sorting example](../examples/sorting.cr) use these implementations.
