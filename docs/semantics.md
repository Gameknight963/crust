# Evaluation and lifetimes

This document collects the rules used by ownership checking and generated
cleanup. Syntax and type-specific operations are in the [language reference](language.md).
Native layout and linkage are in the [ABI reference](abi.md).

## Evaluation order

- Function arguments and initializer elements evaluate from left to right.
- Named struct initializer fields evaluate in written order.
- `&&`, `||`, and `?:` evaluate their selected operands only.
- Ordinary assignment evaluates the value before the destination's address and indices.
- Compound assignment captures the destination and its old value before evaluating the right operand.
- Prefix and postfix updates capture their destination once.
- A return expression is evaluated and retained before cleanup of the returning function's locals.

The compiler uses C temporaries to preserve these rules across native compilers.

## Initialization and ownership

Locals require an initializer. A struct initializer provides every field once,
an array initializer provides every element, and a tagged-union initializer
selects exactly one payload. String literals include their terminating zero and
can pad a larger destination byte array.

Copyable values can be copied through assignment, arguments, and returns.
A type owns resources when it has a destructor or contains owning fields or
elements. Owning values transfer through `move`, consuming calls, and returned
values. A moved binding becomes available again after reinitialization.
`move` on a copyable value leaves its source available.

Control-flow checking requires a value to be available on every path reaching
its use. Replacing an owner evaluates the replacement first, destroys the old
value if it remains live, and installs the new value.

Global and static objects have program lifetime and constant initialization.
Omitting their initializer zero-initializes the object. Zero initialization of a
tagged union selects its first variant. Types with private fields require an
explicit initializer. Static storage requires complete copyable values.
See [global and static storage](language.md#global-and-static-storage).

## Local borrows

A shared borrow permits shared reads. An exclusive borrow permits mutation
through that borrow and blocks competing access to the borrowed region.
References act as aliases, and assignment through a reference changes its
referent. Reference bindings retain their original source.

Local loans remain live until their binding's block ends. Moving, replacing, or
destroying their source requires those loans to have ended. Separate ordinary
struct fields can be borrowed independently. Array element loans cover the
array, and union field loans overlap.

Reborrowing preserves the original source and its guards. A mutable reborrow
temporarily suspends conflicting access through the parent. An inner shared
reborrow permits shared reads and suspends mutation. The parent regains access
when the inner loan ends.

Call-argument loans cover the call and its remaining argument evaluation.
Returned-reference temporaries used in an expression remain live until that
expression statement or initializer ends. Condition temporaries end before
entering the chosen control-flow body. The left operand's temporary loans end
before the right operand of `&&` or `||`, and condition loans end before either
branch of `?:`.

## Returned borrows

One borrowed parameter or reference-containing value parameter supplies the
inferred source of a borrowed return.
With several possible sources, `borrows(a, b)` names the parameters whose
regions the result may reference. Instance methods can name `this`.
Every return path must originate from a declared source. Forwarding through
another call preserves every possible origin.

The caller retains loans on every declared possible source while the result
is live. A returned loan covers the input region passed by the caller, including
when the callee selects one subfield. An exclusive result requires exclusive
access to every declared source.

References to local storage and value parameters' own fields cannot escape.
References stored inside a value parameter can be returned through that
parameter's borrow contract.

Struct reference fields retain their loans through copies, moves, calls,
returns, nested values, and tagged payloads. Mutable reference fields make the
containing value move-only. Shared access to a holder freezes those fields.
Local replacement retains every possible source until the holder's block ends.
See [reference fields](language.md#reference-fields) for storage restrictions.

Temporary struct values can supply borrows to calls and value computations.
Temporary owners are destroyed in reverse construction order at the end of
the enclosing statement expression, initializer, return expression, or control
condition. Short-circuit operands and conditional branches share that cleanup
boundary. Saving a reference to a temporary requires a named owner.

Heap-backed reference storage remains in the [roadmap](roadmap.md).

## Cleanup

Normal block exits destroy live owners in reverse declaration order. Function
returns and loop control destroy owners in each exited scope. A `for` continue
runs the step after body cleanup, and a do-while continue reaches the condition.
Unused owning expression results are destroyed after evaluation.

A struct's destructor runs before its owning fields are destroyed in reverse
declaration order. Arrays destroy owning elements in reverse index order.
A holder's owning reference sources must be declared earlier in the same block,
so they survive its cleanup. Ending local reborrows release before destructors
run. Returned reference values stay protected while cleanup runs, including
references inside returned aggregates. A destructor that could invalidate a
returned value causes a diagnostic.

Tagged unions destroy their active payload. Moving a whole value transfers its
cleanup responsibility. Consuming a tagged payload transfers that payload and
disables its former container's cleanup.

Runtime check failures abort the process. Native termination functions also
bypass Crust stack cleanup. Recoverable library operations return `Result` values. Allocation
failures return rejected owned inputs inside their error payloads, which callers
can recover or leave to ordinary cleanup.

## Unsafe operations

Unsafe code supplies the validity, alignment, initialization, aliasing, lifetime,
and synchronization guarantees required by raw memory and native interfaces.
Creating a raw address alone does not access its pointee. Dereferencing raw
pointers, operating on raw storage, calling native functions, and accessing
mutable global storage require `unsafe`.

Integer overflow, numeric conversion, bounds, and tagged-payload checks retain
their language rules inside unsafe blocks. See [integer conversions](language.md#integers-and-conversions)
and [floating-point numbers](language.md#floating-point-numbers).
