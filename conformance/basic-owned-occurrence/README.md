# Owned occurrence-position corpus

This self-authored corpus checks owned source iteration with `OwnedSourceField`
and `OwnedPosition` through the default lowering and the explicit
`_with_basic_owned_sources` emitters, in the interpreter, generated Rust and
generated C#.

- [`projects.rs`](projects.rs) builds the two models. `primary` iterates the
  primary source; `named` iterates the named `Ref` source beside a primary
  source whose colliding `Value` and `Ref` fields must not be read.
- [`cases.json`](cases.json) lists, for each model, three endpoints and the
  complete expected outcome of the `All` and `Primary` calls. Six cases and
  twelve calls run in each executor, 36 in total.

| Endpoint | Expected outcome |
|---|---|
| `mixed` | Null and JSON-null items are skipped; the two rows keep their raw positions 2 and 4. |
| `empty` | An empty repeated output. |
| `singleton` | A nonrepeating endpoint has no active occurrence: node 2 (`OwnedPosition`) fails with `InactiveSourceFrame` for the iterated owner and the present-empty collection. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). Each error recipe
also lists the C# runtime exception's outer and inner messages.

The CLI harness
[`basic_owned_occurrence.rs`](../../crates/cli/tests/code_generation/basic_owned_occurrence.rs)
checks the case inventory, lowering, validation and emission, the typed error
causes, and every interpreter call. Its ignored test builds the generated Rust
and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests occurrence640 -- --include-ignored
```
