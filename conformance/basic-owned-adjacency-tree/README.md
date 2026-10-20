# Owned adjacency-tree corpus

This self-authored corpus checks `ScopeConstruction::OwnedAdjacencyTree` in the
interpreter, generated Rust and generated C#. See
[the code-generation profile](../../docs/code-generation.md#explicit-owned-adjacency-tree)
and [the ownership design](../../docs/design/source-ownership.md#owned-adjacency-tree-row-collections).

- [`projects.rs`](projects.rs) builds each model and the legacy control. The
  primary endpoint and the named `audit` and `Rows` endpoints share one schema,
  so only the owner distinguishes their rows.
- [`cases.json`](cases.json) lists each case's primary input, supplied named
  inputs and the complete expected outcome of every call. `All` returns the
  primary tree followed by named targets; a target name selects one output.

Trees use a compact JSON encoding: an object is an ordered group, an array is a
repeated collection, and strings, integers, booleans and `null` are scalars.
Errors are typed recipes such as `{"kind": "AdjacencyRootCount", "count": 0}`
or `{"kind": "SourceUnavailable", "owner": "audit"}`. C# value-kind spellings
are mapped to the shared lower-case scalar names.

| Model | Coverage |
|---|---|
| `A01-endpoints` | Primary and named targets over rows with the same keys, a named endpoint spelled like a primary field, an unsupplied endpoint, source order, unreachable rows, Unicode keys, empty and missing collections, duplicate keys, row-relative key and parent errors, root counts and depth |
| `A02-root` | Root expressions reading another endpoint or a legacy field in the receiving context; absent, null and non-string roots, a root reaching a cycle, an unsupplied root endpoint, and rows read before the root |
| `A03-nested` | Owned batch iteration pins its own owner's rows and roots each tree at the batch's own field, while the other endpoint's complete rows are flattened across batches |
| `A04-root-collection` | A named endpoint whose root is the repeating row collection |

The CLI harness
[`basic_owned_adjacency_tree.rs`](../../crates/cli/tests/code_generation/basic_owned_adjacency_tree.rs)
runs every call through the interpreter, checks that the models lower, validate
and emit with the explicit profile, and checks the refusals of older profiles,
closed contexts and directly constructed Programs. Its ignored test builds the
generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests basic_adjacency_tree466
cargo test -p cli --test code_generation --features codegen-tests \
  basic_adjacency_tree466_generated -- --ignored
```
