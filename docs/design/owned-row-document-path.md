# Owned physical row document paths

`OwnedRowDocumentPath` reads a physical source path from one explicitly selected
primary or named endpoint. It reads the runtime row origin, not a schema field,
saved filename, filesystem path or preview value.

The optional collection selector has three distinct meanings:

- Omitted: select the latest active frame of that owner, or its root when no
  frame is active.
- Empty `[]`: select the owner's root, even inside nested rows: its active
  [document-set member](owned-document-path.md) inside a document walk,
  otherwise the immutable registered root.
- Nonempty: require an active frame with that exact endpoint-local collection.

The selected instance must be an annotated group. Its origin returns the
host-supplied resolved path when present, otherwise its original path. Missing
metadata raises `MissingRowDocumentPath`; an inactive explicit collection raises
`InactiveSourceFrame`. A missing named input retains its source-unavailable
failure. No case searches another owner, an ancestor or a group representative.
In particular, a grouped member container has no row path of its own: iterate
its members before asking for their paths.

Physical origins are attached by explicit readers or host code and preserved by
exact copies. Their presence does not authenticate a caller or grant endpoint
ownership. Reconstructed or mutated groups may have no origin and must not
recover one from equal values or path labels.

The operation performs no I/O and does not change legacy `SourceDocumentPath`
resolution, source acquisition, dynamic-loader admission or XML boundaries.
Saved projects without this node retain their existing serialization. A saved
new node preserves the difference between an omitted and an empty selector.
