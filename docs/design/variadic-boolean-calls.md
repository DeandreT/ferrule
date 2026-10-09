# Variadic Boolean calls

The standard `and` and `or` functions accept two or more operands. Each operand
must be a Bool. `and` returns true when every operand is true; `or` returns true
when at least one operand is true. Calls with zero or one operand fail arity
validation, and direct runtime calls report an expected minimum of two.

The engine and generated Rust/C# evaluate arguments eagerly from left to right
before calling the reducer. A decisive Boolean does not skip later arguments.
An error while evaluating a later argument therefore precedes type checking of
previously evaluated arguments. After evaluation succeeds, the reducer checks
every operand in declaration order before reducing and reports the first
non-Bool operand with the existing typed function error.

MFD logical components retain their connected input pins by declared position.
The importer preserves a complete three-, four-, or five-input logical call as
one call; ordinary executable admission and lowering use the shared variadic
catalog. The existing importer pin limits still apply.

Null, String, and numeric operands remain type errors, including an operand
after a decisive Boolean. This is Ferrule's strict Boolean subset. Nullable
logical behavior in an external mapping application requires separate
qualification; accepting additional Boolean inputs does not establish broader
application equivalence.

The authored fixtures in `crates/mfd/tests/fixtures/variadic_logical` reverse XML
pin declaration order to check positional preservation. The frozen
`crates/cli/tests/code_generation/variadic_logical/cases.tsv` contains independent
bit-mask truth expectations and literal type/error expectations. Native and
compiled generated hosts retain complete inputs and outcomes before comparison.
