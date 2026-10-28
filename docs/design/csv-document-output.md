# Path-bearing CSV output documents

`ScopeConstruction::CopyCurrentRows` copies one fixed block of flat primary
rows into a document member. Its schema is a closed, non-repeating group of
scalar columns, while each document value is `Repeated<Group>`. The source and
target column definitions must match. This does not change
`CopyCurrentSource`, which still copies exactly one group and rejects grouping.

The initial construction is primary-output-only: one primary row source,
ordinary `DynamicDocuments` iteration at the row root, fixed-block grouping,
and no named endpoints, bindings or child scopes. Filename expressions run in
the selected block's context. Empty input produces no documents; an uneven
last block retains every remaining row. Invalid row shapes, block sizes and
filenames fail rather than dropping data.

## Native and generated boundaries

Native CSV document publication and the generated libraries use bounded
serialization before publication or return. The limits are 4,096 documents,
4,096 UTF-8 bytes per logical path, 64 MiB per serialized document and 256 MiB
across the complete list. Headers, BOMs, delimiters and quoting count toward
the byte limits. These bounds do not limit the already materialized input and
mapping result.

`format_csv::to_document_bytes` and `to_document_text` preserve ordered logical
paths and return nothing on failure. A document error retains its zero-based
index, path and original row/field error. Pure serializers preserve duplicate
logical paths; the filesystem publisher separately refuses duplicate,
overlapping, escaping or protected destinations.

Use `generate --csv-document-output` for this boundary. Rust provides
`execute_csv_documents` and `execute_csv_documents_bytes`, with `_with_context`
variants. C# provides `ExecuteCsvDocuments` and `ExecuteCsvDocumentsBytes`,
each with an execution-context overload. Calls accept typed row instances,
execute the mapping once and then serialize the whole list. This profile does
not select an implicit CSV input reader or JSON/XML companions. The singular
`--csv-output` and `--csv-document-adapters` profiles remain distinct.

## Saved-design admission

MFD import recognizes a whole-row copy from one flat CSV source and a filename
concatenation containing that source's fixed-block position and string
literals. It checks the actual port declarations and refuses ambiguous
ownership, unsupported metadata and unrelated drivers. Existing filename-input
and unsupported output warnings remain in place. Native MFD export of this
construction remains refused until a saved-design export representation is
qualified separately. The GUI treats its construction as read-only.
