# Owned filter corpus

This self-authored corpus checks owned-source scope filters through
`codegen::lower_with_basic_owned_filters` and the
`emit_with_basic_owned_filters` emitters, in the interpreter, generated Rust
and generated C#.

- [`projects.rs`](projects.rs) builds the six models. Every model declares the
  same primary shape, a named `Ref` source and an unsupplied named source whose
  name (`" Ref 雪 "`) matches only exactly. The primary shape embeds a `Ref`
  group whose names collide with the named owner.
- [`cases.json`](cases.json) lists twelve cases with one call each, twelve
  calls per executor and 36 in total.

| Case | Model | Call | Expected outcome |
|---|---|---|---|
| F01 | `phases` | `All` | Primary rows with owned position > 1 and the named `Filtered` rows with owned position > 2 skip Null/JSON-null items; targets bind compact owned and legacy positions and their owner's root value. |
| F02 | `phases` | `Primary` | The primary output of F01 without supplying `Ref`. |
| F03 | `phases` | `Filtered` | The selected named output of F01. |
| F04 | `phases` | `Primary` | Null and JSON-null primary items keep their raw gaps. |
| F05 | `phases` | `Primary` | An empty repetition. |
| F06 | `nested_identity` | `All` | Filtered nested `Kids` and a same-row `Identity` child keep their parent's compact position. |
| F07 | `lazy_false` | `All` | A false predicate guards an unsupplied named read and a raising target lazily: an empty output. |
| F08 | `non_bool` | `All` | The Int predicate at node 0 fails with `NotABool`. |
| F09 | `order` | `All` | The first accepted row's target raises before the second row's predicate. |
| F10 | `order` | `All` | With the first row rejected, the second row's predicate raises. |
| F11 | `global_rules` | `All` | The second legacy failure rule fires on the second flagged row, after a lazy false first rule. |
| F12 | `global_rules` | `All` | Neither rule fires. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). Each error recipe
also lists the C# runtime exception's message.

The CLI harness
[`basic_owned_filter.rs`](../../crates/cli/tests/code_generation/basic_owned_filter.rs)
checks the case inventory, validation, owned admission, lowering and emission,
and every interpreter call. Its ignored test builds the generated Rust and C#
hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests filter662 -- --include-ignored
```
