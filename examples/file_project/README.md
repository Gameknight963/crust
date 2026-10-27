# File resource

```powershell
cargo run -- run --project examples/file_project
```

`File::temporary()` opens a temporary binary stream. Check `is_open()` before
using it. Moving a `File` transfers ownership, and its destructor closes the
stream. The C library removes the temporary file when it closes.

- `write_byte(value)` writes a byte and reports success
- `read_byte()` returns a byte from 0 to 255, or -1 for EOF or an error
- `eof()` and `has_error()` distinguish the stream's status
- `rewind()` returns to the beginning and clears EOF and error indicators
- `flush()` flushes pending output
- `close()` reports close errors and leaves the object closed

The wrapper synchronizes the stream when switching between reading and writing.
Explicit closing allows the caller to handle flush errors before leaving scope.
Closing an already closed object succeeds. Other operations on a closed object
return failure, and status queries return false.
