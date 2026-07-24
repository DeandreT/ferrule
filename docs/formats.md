# Supported Formats

ferrule converts every supported input into the shared `ir::Instance` tree and
writes target instances through a separate adapter. Format selection normally
comes from the input or output extension; embedded `FormatOptions` provide the
layout and dialect details that an extension cannot express.

| Format | Source | Target | Current scope |
| --- | :---: | :---: | --- |
| XML | Yes | Yes | Hierarchical instance I/O; namespace-aware element and attribute names; XSD-lite with local import graphs; bounded DTD import with internal content-model parameter entities; attributes, `xsi:nil`, generic elements, and ordered mixed content; external DTD identifiers are never loaded |
| JSON | Yes | Yes | Hierarchical instance I/O and JSON Lines; JSON Schema local references, compatible object alternatives, nullable scalar/object/array shapes, and typed or unconstrained dynamic properties |
| CSV | Yes | Yes | Delimited flat rows with configurable delimiter and headers |
| Fixed-width | Yes | Yes | Validated Unicode-scalar column layouts, configurable fill, record separators, and empty-value handling |
| XLSX | Yes | Yes | Typed worksheets, flat and selected composite/grid source shapes, hierarchical targets, and update-existing writes |
| SQLite | Yes | Yes | Table introspection, typed reads, imported relational query shapes, validated declared relations, structured XML text columns, and idempotent full-replace writes |
| X12 / EDIFACT | Yes | Yes | Schema-guided interchange I/O, custom syntax separators, repetitions, qualifier loops, retained field lengths/code lists, and optional lenient parsing |
| HL7 v2 / TRADACOMS | Yes | Yes | Bounded schema-guided message I/O, retained field lengths/code lists, HL7 escapes/subcomponents, and TRADACOMS release escaping |
| IDoc / SWIFT MT | Yes | No | Input through embedded imported layouts |
| FlexText | Yes | Yes | Embedded recursive split/store/switch layouts, including fixed-width and delimited records |
| Protocol Buffers | Yes | Yes | Bounded proto2/proto3 binary I/O with self-contained local import graphs, public imports, nested messages, enums, repeated fields, and packed scalars |
| XBRL | Yes | Yes | Typed instance facts, contexts, dimensions, units, and namespace-qualified concepts |
| PDF | Yes | No | Layout-driven visual extraction from positioned text and painted edges, typed BasicVisual text capture, and visible inherited CropBox/page-rotation normalization |

## Important Boundaries

- PDF targets are not supported.
- XBRL taxonomy formula, presentation, calculation, and linkbase execution are
  outside the current runtime.
- XML preserves declared expanded-name identity for elements and attributes.
  MFD export partitions foreign declarations into deterministic local XSD
  siblings and publishes the complete graph atomically. General `xsi:type`,
  substitution-group, and namespace-dependent wildcard semantics remain outside
  the supported schema subset. Because mapping paths use local field names,
  sibling declarations cannot differ only by namespace; XSD import rejects that
  ambiguous shape explicitly.
- JSON Schema supports selected object alternatives and exact nullable
  scalar/object/array wrappers, not general multi-type scalar/array union
  composition. Shape-neutral validation keywords are accepted for schema
  recovery but are not enforced by the mapping runtime.
- Database execution is SQLite-specific and does not yet provide a general SQL
  mutation or multi-database connector model.
- Complex XLSX, PDF, EDI, and FlexText layouts depend on an embedded validated
  configuration; unsupported imported commands remain explicit warnings.
- EDI output validates every present configured field after wire lexical
  formatting. The bounded report includes all length and code-list violations
  it finds, and validation failures do not replace the destination.

The [workflow-parity roadmap](../ROADMAP.md) tracks the remaining format and
connector work.
