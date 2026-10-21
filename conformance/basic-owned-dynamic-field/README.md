# Owned dynamic source-field corpus

This corpus qualifies the explicit `_with_basic_owned_dynamic_field` profile
described in [the design](../../docs/design/owned-dynamic-field.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema, so every owned open object and property name also exists
under the other owner; `Settings` is a named endpoint whose root is itself an
open object and `Tree` holds recursive directories. Every Project binds owned
dynamic reads, and its `audit` target reads Lookup's `size` property; the
models cover root objects, explicit root pins, keys read from either owner,
row frames of each owner, a foreign row frame, lazy branches, grouped member
containers, an owned read inside a composed owned recursive filter and owned
reads as the needle of composed owned lookups. `cases.rs` authors inputs
whose Primary and Lookup objects differ and records the complete expected
outputs or typed failure for the all-output, Primary and `audit` calls of each
case. `closed.rs` lists refused Projects, directly constructed Programs and the
other generation profiles, including the lookup/find, recursive-filter,
recursive-collect and SequenceExists companions, that must keep refusing owned
dynamic reads.

The CLI `basic_owned_dynamic_field` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches, that the legacy form keeps its wire and ordinary
generation, and that the refusals hold. The ignored host test runs the same
calls through generated Rust and C# libraries.
