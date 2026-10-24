# Owned XML document-set corpus

This self-authored corpus checks the owned XML document-set entry points of
the explicit `_with_basic_owned_xml_document_sets` lowering and emitters in
the interpreter, generated Rust and generated C#. Every endpoint arrives as an
ordered list of XML members instead of a typed instance.

- [`projects.rs`](projects.rs) reuses the `nested`, `cross` and `sorted` models
  of the [owned document-path corpus](../basic-owned-document-path/README.md)
  and gives the primary, the named `Ref` endpoint and the target ordinary XML
  format identity.
- [`cases.json`](cases.json) lists twelve cases and the complete expected
  outcome of their `All` and `Primary` calls: twenty-four calls. Each source
  and named input is a member list of `[path, resolved location or null, XML
  text]`.

The interpreter admits the supplied names and member shapes as the generated
boundaries do, then reads every member as `format_xml::read_local_file_set`
reads a matched file and runs the engine. Each generated host calls every
case through both the text and the byte entry points.

| Case | Expected outcome |
|---|---|
| `nested-sets`, `cross-sets`, `sorted-sets`, `nested-empty`, `cross-empty`, `nested-unsupplied` | The outcomes of the same document-path cases over typed sets: member paths and resolved locations, positions restarting in each document, the typed `MissingDocumentPath` and `SourceUnavailable` failures and empty sets. |
| `nested-primary-member-invalid`, `nested-named-member-invalid` | A malformed member fails as an `Input` failure of its own source, index and path. |
| `nested-unknown-name`, `nested-duplicate-name` | An undeclared or repeated named source fails name admission before a malformed member is parsed. |
| `nested-empty-path`, `nested-empty-resolved` | An empty member path or resolved location fails before any member is parsed. |

Successful trees use the shared lossless recipe codec described in the
[conformance README](../README.md#portable-basic-cohorts). Failures are
portable recipes that every executor derives from its own typed error:
`{"kind": "Member", "owner", "index", "path", "boundary"}` for a per-member
failure, and `{"kind": "Mapping", "error"}` for name admission and mapping
failures, whose `error` names the runtime error kind with its owner, cause and
node.

The CLI harness
[`basic_owned_xml_document_set.rs`](../../crates/cli/tests/code_generation/basic_owned_xml_document_set.rs)
checks the case inventory, lowering, validation and emission, the typed
refusals, that boundary-parsed members equal the local file-set reader's
documents, and every interpreter call. Its ignored test builds the generated
Rust and C# hosts and compares every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests xmldocset995 -- --include-ignored
```
