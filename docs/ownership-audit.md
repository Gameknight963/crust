# Ownership audit

This audit reviews ownership transfer, checked references, cleanup, and the
boundary between safe code and raw C access. The compiler enforces these rules
before generating C. Native sanitizers and allocation tracking exercise the
generated program.

## Finding

Reference-retention inference could overflow the frontend worker's stack when
a returned field borrow referred into a recursive record. The traversal passed
through raw pointers to find reference-carrying storage, but entering a struct
field restarted its cycle guard. This affected ordinary calls that store a
reference through an accessor, even when the recursive pointer was null and
never dereferenced at runtime.

The [retention traversal](../src/check/retention.rs) now shares its active
type/projection set across struct fields.
It removes entries on return so separate fields remain independently visible.
Regression tests cover self-recursive and mutually recursive records, compiled
execution, rejection of shorter-lived sources, and rejection of mutation while
the stored reference remains live.

## Reviewed paths

| Area | Review and regression coverage |
| --- | --- |
| Whole-owner moves and replacement | Named owner state, conditional merges, loop backedges, assignment ordering, and moved-value cleanup |
| Checked borrows | Shared/exclusive access, reborrow ancestry, disjoint fields, multi-source returned references, and stored reference origins |
| Escape checks | Nested heap owners, returned field accessors, branches, loops, switch cases, and temporary owners |
| Cleanup | Reverse scope order, destructors containing references, replacement, discarded results, returns, breaks, and continues |
| Generic containers | Box construction/extraction, vector growth, relocation, removal, allocation failure, and aligned allocation |
| Raw and native boundaries | Raw address creation permissions, explicit unsafe dereference/indexing/arithmetic/casts/calls, and union reads |
| Library resources | Byte/string bounds and allocation arithmetic, file handle detachment before close, and SQLite statement/connection cleanup |

The new [audit tests](../tests/ownership_audit.rs) check unsafe boundaries and
reference escapes. The [cleanup model](../tests/ownership_model.rs) independently
predicts destructor and free events for control-flow paths and checks them
against native execution. Its allocation tracker also rejects invalid frees
and live allocations at program exit. The
[stored-reference tests](../tests/stored_borrows.rs) exercise the recursive-record
finding and propagation through calls.

## Validation

The 2026-11-01 Windows audit run passed 736 regression tests, 218 tests in the
focused suites with native ASan/UBSan instrumentation, formatting, and Clippy.
Two mutation runs completed 25,000 cases each with seeds 42 and 6197.
Optimized MinGW GCC execution matched Clang's output and free events for the
recursive-retention and control-flow cleanup cases. These are local host
results. The updated revision still requires its Linux and macOS CI runs.

Run formatting, the full regression suite, and Clippy:

```text
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```

Set `CRUST_TEST_SANITIZERS=address,undefined` to instrument native executions
using the test harness. CI runs the ownership, borrow, container, resource, and
new audit tests under ASan/UBSan. The ordinary regression jobs run on Windows
Clang, Linux Clang/GCC, and macOS Clang.

The frontend mutation corpus includes recursive retention, reference-carrying
destructors at loop exits, and unevaluated reference expressions. Every corpus
seed must pass checking before mutations begin. Reproduce a run with:

```text
cargo run --offline --example fuzz_frontend -- 25000 42
```

`tools/package.py --offline` builds and extracts the host compiler archive,
then executes its examples and the release application. Enable
`CRUST_TEST_SQLITE=1` with the prepared SQLite source to include the real-library
package checks.

## Boundaries

Raw addresses retain no checked loan and transfer no ownership. Taking an
address checks the required access at creation. Dereferencing it or calling C
requires an unsafe boundary whose author establishes validity, lifetime,
alignment, aliasing, allocation ownership, and any retention by C. A const
pointer constrains writes through that pointer.

The audit and tests establish the behaviors covered above. They do not prove
the compiler sound for every program or verify arbitrary native wrapper
implementations. Cross-platform CI remains the release check for each new
compiler revision.
