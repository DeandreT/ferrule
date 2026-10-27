# Generated source positions

`Node::GeneratedPosition { item }` reads the original one-based ordinal of one
active generated sequence. `item` is the sequence's declared item node, not a
value expression or a collection path. The selector does not evaluate that node.
It cannot fall back to the innermost unrelated sequence.

The interpreter and generated Rust/C# contexts retain this ordinal separately
from their current iteration index. Sorting, filtering and windows may change
the current index; they never change the original ordinal. Nested sequences
retain their ancestors' origins, and each invocation starts its own ordinal at
one. Isolated scalar functions receive positions as explicit arguments rather
than acquiring source-frame access.

`Node::Position` is unchanged. It still reads the selected current iteration
frame, including compact output positions. For a generated range `3..=5`
filtered to values greater than `3`, original positions are `2, 3`, while compact
output positions are `1, 2`.

Validation requires a declared, dominating sequence owner. Bounds and other
parent-context expressions cannot read the sequence they are about to create.
A grouped output has no unique original member ordinal, so a selector for that
grouped sequence is refused there; item-wise controls retain their item context.
An unavailable origin at runtime produces a typed error, never a default `1`.

MFD import uses this selector for Position wired directly to a supported core
Generate or tokenizer producer. Position connected to other stages keeps its
existing lowering; this does not infer intermediate filtered/sorted sequence
identities. Export connects an explicit generated-source selector to its
original producer, before scope controls.

This expression does not qualify `filter_map_v1` interchange or replace the
separate source/output owners and evaluation contract of that descriptor.
