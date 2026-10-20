# Owned Lookup and CollectionFind corpus

This corpus qualifies the explicit `_with_basic_owned_lookup_find` profile
described in [the design](../../docs/design/owned-lookup-find.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema with disjoint row codes, so every owned collection also exists
under the other owner; `Batch` is a repeated named root. The profile composes
the owned adjacency-tree and path-hierarchy companions and owned IntoBlocks
grouping, owned recursive ItemAt producers and owned recursive filters, so
models build an owned directory tree, search the members of owned fixed blocks,
select an owned recursive walk value and filter an owned recursive tree beside
owned searches. `cases.rs` authors the
inputs and records the complete expected outputs or typed failure for the
all-output, Primary and `audit` calls of each case. `closed.rs` lists refused
Projects, directly constructed Programs and the earlier generation profiles
that must keep refusing owned search.

The CLI `basic_owned_lookup_find` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches, and that the refusals hold. The ignored host test runs
the same calls through generated Rust and C# libraries.
