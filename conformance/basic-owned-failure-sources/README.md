# Owned failure-rule iteration corpus

This corpus qualifies the explicit `_with_basic_owned_failure_sources`
profile described in [the design](../../docs/design/owned-failure-sources.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema, so every owned path also exists under the other owner;
`Batch` is a repeated named root. `cases.rs` authors inputs whose Primary and
`Lookup` rows carry opposite flags, and records the complete expected outputs or
typed failure for the all-output, Primary and `audit` calls of each case.
`closed.rs` lists refused Projects, directly constructed Programs and the
earlier generation profiles that must keep refusing owned walks.

The CLI `basic_owned_failure_sources` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches, and that the refusals hold. The ignored host test runs
the same calls through generated Rust and C# libraries.
