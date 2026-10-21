# Physical row provenance

`ir::SourceDocumentOrigin` and C# `FerruleSourceDocumentOrigin` identify one
host-created physical input. Paths are opaque labels: constructing another
origin with the same paths never makes it the same document. Clones retain the
identity. A row origin adds a positive, one-based local data-row position.

The explicit CSV source readers attach row origins to their parsed groups.
`CsvRowOccurrence` / `FerruleCsvRowOccurrence` still carry their document handle;
copying the value also retains its origin through whole-group copies and
grouping. Header skipping does not consume a data-row position. Ordinary CSV
parsing without a physical document remains unannotated.

Provenance is runtime metadata, not data or endpoint ownership. It does not
participate in ordinary value equality, appear in serialized documents or
legacy saved-project data, or establish schema admission. Rust debug output for
groups remains data-only. A host can explicitly attach provenance, so it is not
an attestation mechanism.

Exact copies retain provenance. Rust mutable field access clears it before
exposing the fields; reconstructing a group from its fields also clears it.
C# groups are immutable: general reconstruction clears provenance, while the
internal exact-copy path preserves it. Adding XML type metadata does not change
the data and preserves physical provenance. Both kinds of metadata share Rust's
optional boxed storage to preserve the existing instance size.

This does not change ordinary `DocumentSet` traversal or legacy document-path
resolution. Explicit endpoint-owned metadata reads, dynamic loaders, XML
document boundaries, multi-document mapping admission and output publication
require their own contracts. A group's first member does not establish the
physical origin of its other members.
