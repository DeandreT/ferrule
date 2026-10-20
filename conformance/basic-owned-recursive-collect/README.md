# Owned recursive collection corpus

This corpus qualifies the explicit `_with_basic_owned_recursive_collect`
profile described in [the design](../../docs/design/owned-recursive-collect.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema, so every owned root path also exists under the other owner;
`Tree` is a named endpoint whose root is one directory group and `Forest` a
named repeated root of directories. Every Project binds SequenceItemAt over owned
recursive walks, and its `audit` target reads the first Lookup value; one
model roots an owned adjacency tree over Primary rows at an owned-walk value,
qualifying the composition with the adjacency-tree companion. `cases.rs`
authors inputs whose Primary and Lookup trees differ and records the complete
expected outputs or typed failure for the all-output, Primary and `audit` calls
of each case. `closed.rs` lists refused Projects, directly constructed Programs
and the earlier generation profiles, including the path-hierarchy,
adjacency-tree and fixed-block companions, that must keep refusing owned
producers.

The CLI `basic_owned_recursive_collect` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches, that the legacy form keeps its wire and ordinary
generation, and that the refusals hold. The ignored host test runs the same
calls through generated Rust and C# libraries.
