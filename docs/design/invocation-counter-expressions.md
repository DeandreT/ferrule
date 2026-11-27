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

Evaluate and check the integer start, integer step and String-or-missing
restart in that order. An empty restart differs from missing. Only then
apply the [counter table's exact-key,
wrapping and budget rules](invocation-counters.md). Failure does not advance
that counter or roll back prior successful calls. Diagnostics preserve the
node, operand or underlying limit cause.

Generated Rust and C# use the same root/child lifetime and ordered operand checks.
The initial generated profiles refuse this expression in user-function bodies
and explicit owned-source companions. MFD import is unchanged, and MFD export
refuses counter expressions in all stored graphs, including unused nodes and
function bodies, before selecting any specialized export path. Complete original
mapping support also requires its evaluation, document and publication contracts;
this expression does not infer them from a saved preview or a row position.
