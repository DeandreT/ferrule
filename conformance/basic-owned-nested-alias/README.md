# Owned nested-alias corpus

This self-authored corpus checks owned-source iteration nested inside or beside
legacy paths that name the same collections, through
`codegen::lower_with_basic_owned_filters` and the
`emit_with_basic_owned_filters` emitters, in the interpreter, generated Rust
and generated C#.

- [`projects.rs`](projects.rs) builds six static Group models with a named
  `Ref` source. They bind exact owned positions beside legacy positions and
  fields whose suffixes resolve in opposite directions.
- [`cases.json`](cases.json) lists six cases with one `All` call each, six
  calls per executor and 18 in total.

| Case | Model | Expected outcome |
|---|---|---|
| C01 | `outer_suffix` | Primary `Ref.Rows` parents contain named `Rows` filtered by legacy position > 1; outer, inner and nearest legacy positions and the exact owned positions stay distinct. |
| C02 | `nested_rows` | Named nested `Rows` filtered by the short legacy position; short and long legacy pins read the outer and inner rows. |
| C03 | `empty_root` | A repeated named root with Null and JSON-null items: the two rows keep compact positions while the alias position stays 1. |
| C04 | `same_owner` | A child re-iterating the parent's named `Rows` adds no cursor: it sees only the current row and its filter result. |
| C05 | `singleton` | A scope over the nonrepeating named root reads the owned singleton, the relative and framed primary rows and the outer owned position. |
| C06 | `raw_compact` | The predicate sees raw positions with Null/JSON-null gaps; the target binds compact positions. |

Trees use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts).

The CLI harness
[`basic_owned_nested_alias.rs`](../../crates/cli/tests/code_generation/basic_owned_nested_alias.rs)
checks the case inventory, validation, owned admission, lowering and emission,
and every interpreter call. Its ignored test builds the generated Rust and C#
hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests nestedalias688 -- --include-ignored
```
