# Owned recursive filter corpus

This corpus qualifies the explicit `_with_basic_owned_recursive_filter`
profile described in [the design](../../docs/design/owned-recursive-filter.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema, so every owned collection path also exists under the other
owner; `Tree` is a named endpoint whose root is one directory group and `Forest`
a named repeated root of directories. Every Project's `audit` target filters
Lookup's directories by Lookup's own suffix, while its Primary target filters
the profile's owner: cross-owner predicate reads, item, group and latest
positions, nested batch frames, a legacy fallback read and the composed
recursive-walk and failure companions. `cases.rs` authors inputs whose Primary
and Lookup trees differ and records the complete expected outputs or typed
failure for the all-output, Primary and `audit` calls of each case, including
the ordered XML child stream and both sides of the 256-group depth limit. Deep
cases carry `{"deep": levels}` and every engine builds that input itself.
`closed.rs` lists refused Projects, directly constructed Programs and the
earlier generation profiles, including the recursive-collect, adjacency-tree,
fixed-block and SequenceExists companions, that must keep refusing owned
filters.

The CLI `basic_owned_recursive_filter` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches, that the legacy filter keeps its wire and ordinary
generation, and that the refusals hold. The ignored host test runs the same
calls through generated Rust and C# libraries.
