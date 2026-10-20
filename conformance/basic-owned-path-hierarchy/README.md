# Owned path-hierarchy corpus

This self-authored corpus checks `ScopeConstruction::OwnedPathHierarchy` in the
interpreter, generated Rust and generated C#. See
[the code-generation profile](../../docs/code-generation.md#explicit-owned-path-hierarchy)
and [the ownership design](../../docs/design/source-ownership.md#owned-path-hierarchy-input-collections).

- [`projects.rs`](projects.rs) builds each model and the legacy control. The
  primary endpoint and the named `audit` and `File` endpoints share one schema,
  so only the owner distinguishes their values.
- [`cases.json`](cases.json) lists each case's primary input, supplied named
  inputs and the complete expected outcome of every call. `All` returns the
  primary tree followed by named targets; a target name selects one output.

Trees use a compact JSON encoding: an object is an ordered group, an array is a
repeated collection, and strings, integers, booleans and `null` are scalars.
Errors are typed recipes such as
`{"kind": "PathHierarchyRootCount", "count": 0}` or
`{"kind": "SourceUnavailable", "owner": "audit"}`. C# value-type spellings are
mapped to the shared lower-case scalar names.

| Model | Coverage |
|---|---|
| `P01-endpoints` | Primary and named targets with disjoint values, a named endpoint spelled like a primary field, selected empty/missing collections, an unsupplied endpoint, value types, nulls and top-level files, root counts, depth, Unicode and ordering |
| `P02-nested` | Owned batch iteration pins its own owner while the other endpoint's tree is complete, including empty iterations and empty pinned collections |
| `P03-separator` | A multi-character separator over a collection crossing repetition, with empty directory and file segments |
| `P04-root-collection` | A named endpoint whose root is the repeating scalar collection |
| `P05-failure-walk` | An owned failure-rule walk over the named `File` endpoint, evaluated before an owned `audit` tree; the walk never reads primary rows |

The CLI harness
[`basic_owned_path_hierarchy.rs`](../../crates/cli/tests/code_generation/basic_owned_path_hierarchy.rs)
runs every call through the interpreter, checks that the models lower, validate
and emit with the explicit profile, and checks the refusals of older profiles,
closed contexts and directly constructed Programs. Its ignored test builds the
generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests basic_path_hierarchy465
cargo test -p cli --test code_generation --features codegen-tests \
  basic_path_hierarchy465_generated -- --ignored
```
