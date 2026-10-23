# Owned window corpus

This self-authored corpus checks owned-source sequence windows through
`codegen::lower_with_basic_owned_windows` and the
`emit_with_basic_owned_windows` emitters, in the interpreter, generated Rust
and generated C#.

- [`projects.rs`](projects.rs) builds the ten models over one primary shape.
  All phase-sensitive scopes iterate an explicit owned source; the nested
  identity child has proved owned ancestry.
- [`cases.json`](cases.json) lists twelve cases with one call each: one `All`
  call per model plus `Primary` and `SelectedFirst` selections of the operator
  model, twelve calls per executor and 36 in total.

The expected trees derive from the source contract, not from an executor:

- Null and JSON-null rows are omitted but keep raw ordinal gaps.
  Filter-then-sort predicates and keys see raw ordinals; sort-then-filter
  predicates see sorted ordinals. Target bindings see final compact positions;
  active identity walks keep enclosing positions.
- Every explicit bound evaluates left to right in the incoming parent, even for
  empty or emptied cohorts. `FromTo` evaluates its first bound before its last.
  Negative counts clamp to zero, finite floats truncate and trimmed integral
  strings parse. Windows compose in declaration order; an implicit `First` on a
  nonrepeating Group target follows them and yields an empty Group when nothing
  remains. Scalar cardinality `First` is not admitted.

| Case | Model | Call | Expected outcome |
|---|---|---|---|
| W01 | `operators` | `All` | Rows A30, B10, C20, D40 with root `Count` 2 (rows carry `Count` 999). Skip 1/First 2 = B, C; First 2/Skip 1 = B; From 3 = C, D; 2..3 = B, C; reversed 3..2 is empty. Float 2.9 truncates to 2, string `" 3 "` selects 3, negative selects 0. A lazily false predicate never reads the unsupplied named bound, and First 0 suppresses targets. |
| W02 | `filter_then_sort` | `All` | Survivors of the raw-position predicate sort stably to B, C, D, A; Skip 1/Last 2 gives D, A. `RawKey` keeps raw positions > 3 and sorts them descending to D, C, B; From 2 gives C, B. |
| W03 | `sort_then_filter` | `All` | Stable ascending B, C, D, A; sorted position > 1 leaves C, D, A; Skip 1/First 2 gives D, A. |
| W04 | `nested_parent_bounds` | `All` | Last 2 selects C, D at compact parent positions 1, 2. Child `First(parent position)` selects [9] and [8, 6], not a row or child `Count`. Identity First 1 keeps the current row and its position. |
| W05 | `empty_bounds_eager` | `All` | An empty input still evaluates First 0, then the `FromTo` first bound raises at node 0; the last bound and the target are unreached. |
| W06 | `zero_window_non_bool` | `All` | First 0 does not suppress the complete predicate batch: the Int predicate at node 0 fails with `NotABool` before any target. |
| W07 | `invalid_bound` | `All` | First 0 does not skip the Bool `Last` bound at node `u32::MAX`: `NotAnItemCount` precedes the predicate and the target. |
| W08 | `fts_predicate_priority` | `All` | Filter-then-sort completes every predicate before keys, bounds and targets: the second predicate's raise at node 22 wins. |
| W09 | `stf_key_priority` | `All` | Sort-then-filter completes every key before bounds, predicates and targets: the second key's raise at node 22 wins. |
| W10 | `first_predicate_priority` | `All` | An implicit `First` still completes every predicate: the second predicate's raise wins over the first target's. |
| W11 | `operators` | `Primary` | The selected primary output of W01. |
| W12 | `operators` | `SelectedFirst` | The selected nonrepeating `SelectedFirst` output of W01. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). Each error recipe
also lists the C# runtime exception's message.

The CLI harness
[`basic_owned_windows.rs`](../../crates/cli/tests/code_generation/basic_owned_windows.rs)
checks the case inventory, validation, owned admission, lowering and emission,
and every interpreter call. Its ignored test builds the generated Rust and C#
hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests windows704 -- --include-ignored
```
