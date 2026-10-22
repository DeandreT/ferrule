# Owned dynamic driver loading corpus

This corpus qualifies the explicit `_with_basic_owned_dynamic_loading` profile
described in [the design](../../docs/design/owned-dynamic-loading.md).

`projects.rs` defines genuine Projects. Primary and the static `Catalog`
endpoint share one driver shape, `Files/{Path, Label}`; `Docs` loads one
document per row of its exact driver collection and `Parts` loads one document
per `Parts` row of the current `Docs` document. The models cover a Primary
driver, the same-shaped named driver, filters and sorts and windows over
loaded documents, a named target that alone loads, and a read of the loaded
owner outside its loading scope. Every Project has an `audit` target.

`cases.rs` authors inputs and host documents and records, for the all-output,
Primary and `audit` calls of each case, the complete expected outputs or typed
failure, every attempted loader callback and the physical reads left after a
success-only host cache. Cases cover nested loading with a repeated document,
absent, null and non-string paths, host failures, a missing loader, empty and
unsupplied drivers and unselected targets. `closed.rs` lists the older
generation profiles that keep refusing loading scopes, refused Projects and
directly constructed Programs.

The CLI `basic_owned_dynamic_loading` tests check that every profile lowers,
validates, keeps its named-input catalogue and emits both backends, that every
interpreter call matches and that the refusals hold. The ignored host test runs
the same calls through generated Rust and C# libraries with recording loaders.
