# Owned document-set paths

A document-set endpoint supplies several documents, each with a portable
member path and an optional resolved location, such as an XML file set read
through a wildcard. An owned scope can walk the members of exactly one such
endpoint, and `OwnedSourceDocumentPath` reads the location of exactly one
owner's active member. The legacy `SourceDocumentPath` node, its nearest-frame
selection and its first-member fallback are unchanged.

## Wire form

```json
{"kind":"owned_source_document_path","owner":{"kind":"named","name":"Ref"}}
```

The body is strict: unknown keys, a missing owner and an empty named owner are
refused. Readers that predate the kind refuse it rather than reinterpreting it
as the legacy node. Projects without it keep their existing serialization.

## Document occurrences

An ungrouped owned scope whose selector names an endpoint that is a document
set, with no active document of that owner, walks each member in set order.
The member's document roots its owner, and the selector path is walked from it
exactly as from a supplied root: `[]` yields one candidate per document and a
repeating collection yields its rows from every document in one cohort.

- A document is not a position or a frame. Row positions restart in each
  document, and filters, sort keys and windows run over the complete cohort
  with the ordinary renumbering.
- Inside, the owner is an ordinary endpoint rooted at its current document:
  owned fields, explicit root pins, positions and nested scopes of the same
  owner read that document and never re-enter the set.
- Another owner keeps its own root and documents. A nested scope of a second
  document-set owner walks that owner's members beside the active document of
  the first.
- Grouped scopes, joins, searches, aggregates, failure walks, tree operators
  and recursive walkers keep refusing a document-set endpoint, as does any
  owned read of an uniterated set.

## Reading the location

`OwnedSourceDocumentPath` returns the resolved location of its owner's latest
active document, or its portable member path when no location was resolved.
With no active document of that owner it fails with a typed
`MissingDocumentPath`; neither another owner's document, an uniterated set's
first member nor an ancestor answers it. An unsupplied named owner keeps its
`SourceUnavailable` failure. The read is a Basic parent-context expression and
creates no frame. Failure rules, generated items, functions and every closed
capability context refuse it.

## Loaded documents

Inside an [owned dynamic loading scope](owned-dynamic-loading.md), each loaded
document roots its owner with the logical path its driver row computed, the
exact path the host loader was called with. The read naming that owner returns
it; a host's resolved or cached location is never reported. Rows whose path is
absent or null load nothing and so have no document, a failed load stops the
scope before any document is walked, and outside its loading scope the owner
is unavailable (`SourceUnavailable`). Nested loading keeps each owner's own
document, so a `Parts` read inside a `Docs` document returns the part's path
and a `Docs` read there still returns the enclosing document's path.

## Excluded routes

A legacy dynamic named owner has no owned loading scope: the read naming it is
refused before input acquisition.

## Generation profile

The `DocumentPaths` preset (`*_with_basic_owned_document_paths`) admits owned
document-path reads and lets every ungrouped owned scope walk document-set
members, with filter, sort and window controls. Every other preset, the
ordinary emitters, `.mfd` export and GUI authoring refuse the read; the GUI
shows a saved one read-only. Generated libraries receive document sets through
the typed instance entry points. The `DynamicDocumentPaths` preset
(`*_with_basic_owned_dynamic_document_paths`) composes owned dynamic loading
with document sets and also admits the read of a loaded owner. The
`XmlDocumentSets` preset (`*_with_basic_owned_xml_document_sets`) composes the
document-path profile with XML entry points that read each endpoint as an
ordered member list, keeping every member's path and resolved location as the
local XML file-set reader records them
([portable corpus](../../conformance/basic-owned-xml-document-set/README.md)).

## Qualification

The [portable corpus](../../conformance/basic-owned-document-path/README.md)
pairs same-shaped primary and named document sets with distinct locations and
records complete expected outputs and typed failures. The CLI
`code_generation` tests run every call through the interpreter; the ignored
host test builds generated Rust and C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_docpath451_generated_rust_and_csharp_match_expected -- --ignored
```

The [loaded-document corpus](../../conformance/basic-owned-dynamic-document-path/README.md)
records loaded paths, nested loading, primary document-set members driving
loads, and the attempted callbacks and physical reads of each call:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_dyndocpath994_generated_rust_and_csharp_match_expected -- --ignored
```
