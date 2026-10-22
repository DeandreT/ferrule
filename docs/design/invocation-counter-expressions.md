# Invocation counter expressions

`Node::InvocationCounter` is an explicit effectful expression with a literal
key and ordered `start`, `step` and optional `restart` input nodes. Every reached
evaluation is a call; the expression is not memoized by NodeId or replaced with
row-position arithmetic.

Hosts opt in with immutable `InvocationCounterLimits` on their execution
context. Without that configuration, evaluation refuses before reading operands.
Each mapping entry call creates a fresh bounded table. Primary and named targets,
and child/grouped contexts within that invocation, share the table. Reusing host
parameters for another entry call does not reuse counter state.

Start and step must produce integers. Restart accepts String or the missing Null
value; a present empty string differs from missing. Inputs are evaluated and
checked in that order. Only after all three succeed does the table advance, using
the standalone counter contract: first use or a changed restart returns start;
otherwise the current step is added with signed 64-bit wrapping. A failed call
does not advance its counter, but it does not roll back earlier successful calls.

Keys and restart text preserve exact identity, including whitespace and Unicode.
Count and retained-text limits are explicit. Runtime failures preserve the node,
operand or underlying counter-limit cause.

Generated Rust and C# use the same root/child lifetime and ordered operand checks.
The initial generated profiles refuse this expression in user-function bodies
and explicit owned-source companions. MFD import is unchanged, and MFD export
refuses counter expressions in all stored graphs, including unused nodes and
function bodies, before selecting any specialized export path. Complete original
mapping support also requires its evaluation, document and publication contracts;
this expression does not infer them from a saved preview or a row position.
