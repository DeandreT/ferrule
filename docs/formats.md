# Supported Formats

ferrule converts every supported input into the shared `ir::Instance` tree and
writes target instances through a separate adapter. Format selection normally
comes from the input or output extension; embedded `FormatOptions` provide the
layout and dialect details that an extension cannot express.

| Format | Source | Target | Current scope |
| --- | :---: | :---: | --- |
| XML | Yes | Yes | Hierarchical instance I/O; XSD-lite and bounded DTD import with internal content-model parameter entities; attributes, `xsi:nil`, generic elements, and ordered mixed content; external DTD identifiers are never loaded |
| JSON | Yes | Yes | Hierarchical instance I/O and JSON Lines; JSON Schema local references, compatible closed-object alternatives, and typed dynamic properties |
| CSV | Yes | Yes | Delimited flat rows with configurable delimiter and headers |
| Fixed-width | Yes | Yes | Validated Unicode-scalar column layouts, configurable fill, record separators, and empty-value handling |
| XLSX | Yes | Yes | Typed worksheets, flat and selected composite/grid source shapes, hierarchical targets, and update-existing writes |
| SQLite | Yes | Yes | Table introspection, typed reads, imported relational query shapes, and idempotent full-replace writes |
| X12 / EDIFACT | Yes | Yes | Schema-guided interchange I/O, custom syntax separators, repetitions, qualifier loops, and optional lenient parsing |
| HL7 v2 / TRADACOMS | Yes | Yes | Bounded schema-guided message I/O |
| IDoc / SWIFT MT | Yes | Yes | Runtime I/O through embedded imported layouts |
| FlexText | Yes | Yes | Embedded recursive split/store/switch layouts, including fixed-width and delimited records |
| Protocol Buffers | Yes | Yes | Bounded proto2/proto3 schemas and binary messages, including nested messages, enums, repeated fields, and packed scalars |
| XBRL | Yes | Yes | Typed instance facts, contexts, dimensions, units, and namespace-qualified concepts |
| PDF | Yes | No | Layout-driven visual extraction from positioned text and painted edges |

## Important Boundaries

- PDF targets are not supported.
- XBRL taxonomy formula, presentation, calculation, and linkbase execution are
  outside the current runtime.
- XML supports a practical schema and namespace subset, not arbitrary namespace
  identity or every `xsi:type` input shape.
- JSON Schema supports selected object alternatives, not general scalar, array,
  or nested union composition.
- Database execution is SQLite-specific and does not yet provide a general SQL
  mutation or multi-database connector model.
- Complex XLSX, PDF, EDI, and FlexText layouts depend on an embedded validated
  configuration; unsupported imported commands remain explicit warnings.

The [workflow-parity roadmap](../ROADMAP.md) tracks the remaining format and
connector work.
