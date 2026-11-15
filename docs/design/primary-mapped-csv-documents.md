# Primary acquisition and mapped CSV documents

`emit_with_primary_mapped_csv_documents` composes direct primary CSV acquisition
with the strict mapped-document output profile. It accepts a lowered
`DocumentProgram` and `PrimaryMappedCsvDocumentPolicy { input, output }`.
Configured Rust and C# `Generator` methods use the same profile; an included
compatibility manifest records `primary_mapped_csv_document`.

The source is a canonical nonrepeating physical CSV row schema. Acquisition
assembles its parsed rows as the mapper's repeated input. The target is the
existing closed mapped-document wrapper with exactly one explicitly selected
repeating flat row field. Input and output schemas may differ.

## Saved-model CLI access

Select this profile explicitly and name the actual repeating target row field:

```text
ferrule generate --project authored.document.json --language rust --rust-runtime-path C:/dev/ferrule/crates/codegen-runtime --out generated-rust --primary-mapped-csv-documents Entries --compatibility-manifest
ferrule generate --project authored.document.json --language csharp --out generated-csharp --primary-mapped-csv-documents Entries --compatibility-manifest
```

`--project` here reads saved **`mapping::DocumentProject` JSON**, not a serialized
`codegen::DocumentProgram`, ordinary `Project` JSON or MFD XML. Author the existing
mapping with the mapping SDK, retain its filename graph node and save it with the
existing document codec:

```rust,ignore
let document = mapping::DocumentProject::new(
    project,
    mapping::PrimarySourcePlan::new(filename_node),
);
std::fs::write(
    "authored.document.json",
    mapping::document_project_file::encode_pretty(&document)?,
)?;
```

The embedded project supplies the canonical CSV source, closed target wrapper,
dynamic-document root and constructed repeating row child described above. Its
filename node is a supported string literal or global string parameter. The
codec preserves existing file envelopes and the `document_project` kind when an
envelope is needed; there is no new file format. The default selection is Direct,
and this profile requires the global output filename to be absent.

The selector is exact, nonempty field text. `Entries` is only an example; quote
field names containing spaces and pass their whitespace unchanged. No trimming,
normalization, conventional-name fallback or suffix inference is performed. All
other companion selectors, `--primary-documents` (including its
`--primary-csv-documents` alias) and `--raw-delimited-crlf` conflict with this flag.
The older primary-document profile keeps its flat CSV boundary.

Existing Rust package and C# assembly/namespace/class flags and their backend
checks apply unchanged, as does `--compatibility-manifest`. Generated trees use
the existing atomic publication path and require a new destination directory.
Generation lowers the saved model and emits source; it does not read preview
CSV inputs, bind runtime filename/business parameters or publish business
documents. Generated consumers still supply the context, source host, decoding
policy and input-document limit.

Library callers can select
`cli::GenerateProfile::PrimaryMappedCsvDocument { rows_field: String }` with
`plan_generate` or `generate_project_with_settings`. Planning and generation use
the same saved-document preparation and refusal order; planning writes nothing.
`check_generate_project` and `plan_generate_project` accept ordinary projects,
so they refuse this document-required variant.

**Rust API ownership migration:** `cli::GenerateProfile` now owns the selector
and implements `Clone`, not `Copy`. Clone a profile before passing it by value if
the caller needs to reuse it. The codegen compatibility `GenerationProfile`
enum and serialized profile identity are unchanged.

## Admission and calls

Emission refuses a global output filename, then a non-Direct selection, then
invalid input settings/schema, then unsupported mapped output. Lowering owns
filename-expression classification. Explicit policies govern this profile; no
format is inferred from a path suffix or the older saved-format route.

Generated Rust adds `execute_primary_mapped_csv_documents` and
`execute_primary_mapped_csv_documents_bytes`; C# adds
`ExecutePrimaryMappedCsvDocuments` and `ExecutePrimaryMappedCsvDocumentsBytes`.
Each requires an execution context, source host, byte-decoding policy and maximum
input-document count. Existing mapped-only methods remain available.

Each invocation:

1. Validates the input-document limit before filename evaluation or host calls.
2. Evaluates the primary filename once, without a preview/default fallback.
3. Asks the host to select that exact text with Direct selection. Zero or multiple
   requests fail before loading; path interpretation belongs to the host.
4. Loads and completely parses the one selected document, preserving row origins,
   then assembles the input rows.
5. Calls the contextual mapped-output adapter once with that same context.
6. Returns all computed paths and text/bytes in document order, or one typed
   failure without a partial returned vector.

The host owns selection, access authorization and loading. Returned logical paths
are not normalized or published. A library failure does not undo host callbacks.

## Errors and limits

Rust's generated `PrimaryMappedCsvDocumentError<E>` distinguishes acquisition,
assembly and output; the existing output cause distinguishes execution from
serialization. C# retains the existing primary-document, mapping-runtime and CSV
document exceptions and their causes. Null C# context/host arguments remain
argument precondition errors. Filename parameter coercion follows the existing
runtime contract; for example Int can become String, while Null and a missing
parameter retain their distinct typed errors.

Input parsing retains the existing physical/decoded/cell/row/column/node budgets
and caller-selected decoding policy. Output retains 4,096 documents, 4,096 UTF-8
path bytes, 64 MiB per document and 256 MiB total. Strict unquoted output still
refuses delimiter/newline-bearing values; raw-delimited mode is not admitted.
Earlier input failures prevent mapping. Mapping and serialization failures occur
after acquisition and retain the original stage, document/path and nested cause.

This profile composes existing Ferrule contracts. Its independent fixtures and
generated Release consumers qualify that composition, not untouched MFD import,
active integration membership or MapForce parity.
The CLI route provides product access to that existing composition. Authored
saved-model fixtures do not establish new captured-MFD admission or reference
parity; CLI-source and consumer qualification must be run separately for the
exact source revision being released.
