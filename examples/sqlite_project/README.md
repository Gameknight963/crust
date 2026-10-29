# SQLite example

This project compiles SQLite 3.50.4 alongside a Crust application. It creates an
in-memory table, binds and inserts integer values, handles a uniqueness error
and malformed SQL, and reads the row count and sum.

```powershell
python examples/sqlite_project/prepare.py
cargo run -- run --project examples/sqlite_project
```

Expected output:

```text
1
1
3
33
```

Preparation downloads the [official SQLite amalgamation](https://www.sqlite.org/2025/sqlite-amalgamation-3500400.zip),
checks its SHA-256 digest, and extracts its source and header under `target/sqlite`.
`prepare.py --offline` reuses the cached archive. SQLite is in the
[public domain](https://www.sqlite.org/copyright.html). The generated bindings
are committed in [bindings.cr](bindings.cr).

`Database` owns a connection. `Statement` owns a prepared statement and retains
a shared reference to its database. Moving either owner transfers its resource.
The statement destructor finalizes its handle, and the database destructor closes
its connection. Failed opens also close any handle returned by SQLite.

The wrapper returns native SQLite result codes through `Result`. `step()` returns
`true` for a row and `false` when execution finishes. Completion and errors remain
cached until `reset()`. Reset preserves parameter bindings and reports the previous
execution error using SQLite's reset rules. `integer()` requires a current row,
a valid column index, and an integer column.

Paths and SQL use zero-terminated byte-array views. Preparation compiles the first
SQL statement in the view. SQLite copies the SQL during preparation, so statements
retain their database reference without retaining the SQL view. The native calls
and raw handles are contained in [database.cr](database.cr). Its unsafe `adopt`
factory requires an owned statement handle associated with the supplied database.

This example uses integer parameters and columns. Its configuration disables
SQLite threading and extension loading. Database methods take shared references
because multiple statements may use the same connection. SQL can still modify
the database through these references. Connections stay on one thread.

Run the integration checks with:

```powershell
python tools/test_sqlite.py --offline
```

The checks execute the real library, verify handle cleanup and statement lifetime
errors, and inject allocation failures through SQLite's allocator API. CI runs
them with each native compiler and under sanitizers. Ordinary `cargo test` checks
the Crust wrapper without downloading SQLite. Set `CRUST_TEST_SQLITE=1` after
preparation to include native execution in a Cargo test run.

The wrapper follows SQLite's [open](https://www.sqlite.org/c3ref/open.html),
[close](https://www.sqlite.org/c3ref/close.html),
[prepare](https://www.sqlite.org/c3ref/prepare.html), and
[finalize](https://www.sqlite.org/c3ref/finalize.html) contracts.
