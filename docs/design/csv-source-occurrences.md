# Physical CSV source occurrences

`format_csv::CsvSourceDocument` and `FerruleCsvSourceDocument` parse one complete
physical CSV document with the existing bounded reader and an explicit byte
decoding policy. A failed parse returns no document or partial row collection.
The host supplies logical and optional resolved paths; the API does not perform
filesystem access, normalization or filename evaluation.

Each parse establishes a distinct identity, even for equal paths and bytes.
Cloning a Rust document keeps its identity. Rows retain their document, exact
value and one-based data-row position (excluding a skipped header). Retaining or
regrouping a row does not substitute the first group's document for its origin.
Equal values alone never establish row or document identity.

`CsvSourceRows` and `FerruleCsvSourceRows` explicitly enumerate rows across host
requests. They request and parse documents lazily in host order, skip empty row
collections, and count empty documents against the selected document limit.
They stop permanently at the first reached enumeration, loading or parsing
failure. An additional request is needed to distinguish exhaustion from a count
overflow, but the overflowing request is never loaded. Per-document byte and
row limits and the document-count limit do not bound retained process memory.

This is a separate occurrence API, not a change to empty-path `DocumentSet`
traversal. It does not enable importer admission, generated mapping orchestration,
target-document ownership or filesystem publication. Consumers must retain row
occurrences through their own mapping and grouping rather than flattening them
into bare values and losing the physical origin.
