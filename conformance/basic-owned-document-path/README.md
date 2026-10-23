# Owned document-set path corpus

This self-authored corpus checks owned scopes over document-set endpoints and
`OwnedSourceDocumentPath` through the explicit `_with_basic_owned_document_paths`
lowering and emitters, in the interpreter, generated Rust and generated C#.

- [`projects.rs`](projects.rs) builds four models over a primary endpoint and a
  named `Ref` endpoint with one shared schema, so every path collides across
  owners. `nested` walks each primary document, its rows and, inside every
  primary row, each named document's rows; `cross` walks only named documents;
  `root` reads the primary path with no iteration; `sorted` walks the rows of
  every primary document as one sorted collection.
- [`cases.json`](cases.json) lists ten cases and the complete expected outcome
  of their `All` and `Primary` calls: twenty calls in each executor.

The primary set has members `p1.xml` (resolved `/in/p1.xml`) and `p2.xml`
(no resolved location); the named set has `n1.xml` (resolved `/ref/n1.xml`)
and `n2.xml`.

| Case | Expected outcome |
|---|---|
| `nested-sets` | Each read returns its own owner's document: resolved locations where present, otherwise member paths. Titles read through an explicit root pin and without a pin come from the current document; row positions restart in each document. |
| `nested-named-plain` | A named endpoint that is not a document set has no document path: node 2 fails with `MissingDocumentPath` for `Ref`. |
| `nested-primary-plain` | The primary endpoint is one plain document: node 1 fails with `MissingDocumentPath` for the primary owner. |
| `nested-empty` | An empty primary set yields no documents. |
| `nested-unsupplied` | The unsupplied named owner fails its scope walk with `SourceUnavailable`. |
| `cross-sets` | An active named document never answers the primary path: node 1 fails with `MissingDocumentPath`. |
| `cross-empty` | No named document is walked, so the primary path is never read. |
| `root-set`, `root-plain` | Without an active document the primary path fails, whether or not the endpoint is a set. |
| `sorted-sets` | Rows of both primary documents sort as one collection; each keeps its own document path and the sorted positions are renumbered. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts); a document set is
`["DocumentSet", [[path, resolved or null, tree], ...]]`. Each error recipe
also lists the C# runtime exception's outer and inner messages.

The CLI harness
[`basic_owned_document_path.rs`](../../crates/cli/tests/code_generation/basic_owned_document_path.rs)
checks the case inventory, lowering, validation and emission, the strict wire
form, the closed presets, dynamic-owner and grouped routes, and every
interpreter call. Its ignored test builds the generated Rust and C# hosts and
compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests docpath451 -- --include-ignored
```
