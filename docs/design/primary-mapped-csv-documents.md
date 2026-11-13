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
