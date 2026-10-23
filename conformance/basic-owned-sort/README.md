# Owned sort corpus

This self-authored corpus checks sorted owned-source scopes through
`codegen::lower_with_basic_owned_sort` and the `emit_with_basic_owned_sort`
emitters, in the interpreter, generated Rust and generated C#.

- [`projects.rs`](projects.rs) builds the sixteen models over one primary
  shape. Two models also declare a named `Ref` source with the same shape.
- [`cases.json`](cases.json) lists sixteen cases with one call each, sixteen
  calls per executor and 48 in total.

| Case | Model | Call | Expected outcome |
|---|---|---|---|
| S01 | `ascending_scalar` | `All` | A sorted Scalar construction with an ascending numeric key. |
| S02 | `filter_then_sort` | `All` | Original-position predicate > 1 before any key; compact output positions 1, 2. |
| S03 | `sort_then_filter` | `Primary` | Sorted-position predicate > 1 and compact accepted output. |
| S04 | `stable_total_order` | `All` | Total scalar key order: stable Null/JSON-null and mixed numeric ties, an integer beyond 2^53, infinities, NaNs and Unicode order, with exact Float bits. |
| S05 | `descending_secondary` | `All` | Descending primary and ascending secondary keys; fully equal tuples keep source order. |
| S06 | `named_collision` | `All` | The named owner's sorted rows stay separate from the colliding primary `Ref` and `Rows`; rooted owner fields are exact. |
| S07 | `raw_position_keys` | `Primary` | Original positions 2, 4, 5 keep Null/JSON-null gaps; predicate > 2 and descending original-position keys. |
| S08 | `nested_outer_phases` | `All` | A sorted, filtered parent phase survives a sorted nested child; ancestor positions are exact. |
| S09 | `fts_predicate_before_key` | `All` | Every original predicate runs first: the later predicate's raise beats an earlier key's. |
| S10 | `fts_predicate_before_target` | `All` | The later predicate's raise beats an earlier target's. |
| S11 | `stf_key_before_target` | `All` | Every key runs before targets: the second key's raise beats the first target's. |
| S12 | `stf_target_before_predicate` | `All` | The first sorted accepted target raises before the later sorted predicate. |
| S13 | `candidate_major_secondary` | `All` | Keys evaluate candidate-major: the first candidate's secondary key at node 0 raises before the second primary key. |
| S14 | `rejected_and_empty` | `All` | False predicates skip the raising key and every target; a zero-candidate extra target evaluates no key. |
| S15 | `typed_owned_key` | `Primary` | The first secondary key at node `u32::MAX` reads the unsupplied named source: `SourceUnavailable` with no scope. |
| S16 | `plain_identity_positions` | `All` | A same-owner identity child keeps the sorted parent phase; the `Plain` target pins `Details.Value` and its legacy position 1. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). Each error recipe
also lists the C# runtime exception's message.

The CLI harness
[`basic_owned_sort.rs`](../../crates/cli/tests/code_generation/basic_owned_sort.rs)
checks the case inventory, validation, owned admission, lowering and emission,
the S15 typed cause, and every interpreter call. Its ignored test builds the
generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests sort670 -- --include-ignored
```
