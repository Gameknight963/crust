# File statistics

`crustwc` prints newline and byte counts for one or more files. `--utf8` validates
the contents as UTF-8 and adds a Unicode scalar-value count. Output columns are
`lines bytes [characters] path`.

```text
cargo run -- run --project examples/crustwc -- README.md
cargo run -- run --project examples/crustwc -- --utf8 README.md docs/language.md
```

After building, run the executable directly:

```text
crust build --project examples/crustwc
examples/crustwc/target/crust/crustwc/crustwc --help
```

On Windows, the executable has an `.exe` suffix. Use `--` before filenames that
start with `-`.

- Exit `0` after successful processing or `--help`
- Exit `1` after a file, UTF-8, allocation, or output error
- Exit `2` after an option or input-list error

The program reports file errors on stderr and continues with the remaining
files. Newline counts include each `\n` byte. Binary mode preserves bytes on
every platform. Default counting streams the input, and UTF-8 counting keeps
the current file in an owned byte buffer.

## Implementation

- `src/options.cr` parses arguments into a fallible vector
- `src/stats.cr` reads owned file resources and validates owned UTF-8 buffers
- `src/main.cr` handles results and continues after file errors
- `native/console.c` formats counts and reports output failures

The target uses the shared library's file, vector, byte-buffer, string, and
result types. Its native configuration compiles the C file wrapper and console
helpers. Paths use the host C runtime's argument and filename encoding.
