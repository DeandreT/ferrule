# Owned collection aggregate corpus

This corpus qualifies the explicit `_with_basic_owned_collection_aggregates`
profile described in [the design](../../docs/design/owned-collection-aggregates.md).

`projects.rs` defines genuine Projects. Primary and the named `Ref` endpoint
share one schema with disjoint values, so every owned collection also exists
under the other owner; `Batch` is a repeated named root. Models exercise every
reducer over an entire endpoint root, nested and scalar collections, computed
item values beside parent-context arguments, owned By groups, ordinary owned
rows and owned fixed blocks, and the evaluation order of walks, item values,
arguments and typed reducer failures. `cases.rs` authors the inputs and records
the complete expected outputs or typed failure for the all-output, Primary and
`audit` calls of each case. `closed.rs` lists refused Projects, directly
constructed Programs and the earlier generation profiles, including the owned
search profile, that must keep refusing owned aggregates.

The CLI `basic_owned_collection_aggregates` tests check that every profile
lowers, validates, keeps its named-input catalogue and emits both backends, that
every interpreter call matches, and that the refusals hold. The ignored host
test runs the same calls through generated Rust and C# libraries.
