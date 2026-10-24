# Owned loaded-document path corpus

This self-authored corpus checks `OwnedSourceDocumentPath` over owned dynamic
loading through the explicit `_with_basic_owned_dynamic_document_paths`
lowering and emitters, in the interpreter, generated Rust and generated C#.
Each call runs with a fresh recording loader that separates attempted
callbacks from physical reads behind a success-only cache.

- [`projects.rs`](projects.rs) builds seven models. `Docs` loads one document
  per `Files` row of its driver (the primary source, or the static `Catalog`
  in `named_driver`), and `Parts` loads one document per `Parts` row of the
  current `Docs` document. `members` walks a primary document set and loads
  `Docs` from each member's own rows.
- [`cases.json`](cases.json) lists the host documents and eleven cases with
  the complete expected outcome, callbacks and reads of their `All` and
  `Primary` calls: twenty-two calls in each executor.

| Case | Expected outcome |
|---|---|
| `nested-paths` | Each Docs document reports its logical path; each Part reports its own path and, beside it, the enclosing document's path. |
| `nested-skipped-and-repeated` | Absent and null driver paths load nothing; a path requested twice yields two documents with the same path and one physical read. |
| `nested-failed-load` | A failed load stops the scope with its typed load failure after every request. |
| `nested-no-loader` | Without a host loader the loading scope fails before any request. |
| `named-driver-paths` | A named driver's rows load and report their own paths. |
| `sorted-paths` | Sorting reorders the documents; each keeps its own path. |
| `outside-read` | Outside its loading scope the loaded owner is unavailable at the read. |
| `unloaded-parts-read` | Inside Docs, the not-yet-loaded Parts owner is unavailable at the read. |
| `primary-path-read` | Inside Docs, the plain primary endpoint has no document path (`MissingDocumentPath`). |
| `members-paths` | Each primary member reports its location, and each document it loads reports its logical path beside the member's. |
| `members-plain-primary` | A plain primary endpoint has no member location. |

Trees and errors use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). The CLI harness
[`basic_owned_dynamic_document_path.rs`](../../crates/cli/tests/code_generation/basic_owned_dynamic_document_path.rs)
checks the case inventory, lowering, validation and emission, the closed
presets and legacy dynamic owner, and every interpreter call. Its ignored test
builds the generated Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests dyndocpath994 -- --include-ignored
```
