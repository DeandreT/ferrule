# Bounded document payload profile

`PayloadRunOptions::with_bounded_documents()` explicitly opts the native public
payload API into pure-document admission. It applies to
`run_project_value_payloads` and `run_project_payloads`; it does not change the
CLI, generated mappings, preview serializers or default payload callers.
`with_bounded_tabular()` retains its existing behavior. When both profile
builders are called, the last one selects the profile.

## Identity and selection

Each admitted boundary needs saved `FormatOptions`: `json_document`, `json5`,
`json_lines`, `xml_document`, `edi_kind` (X12 or EDIFACT), `tabular_kind` (CSV or
XLSX), `fixed_width`, `flextext`, `protobuf` or `xbrl`. TSV is CSV with an explicit
tab delimiter. XLSX requires explicit XLSX identity even if sheet options exist.
Uploaded filenames, saved design-time paths and output-path overrides do not
select or modify the codec. Internal codec paths are not returned as artifact
paths or diagnostic identities.

Missing identity, conflicting/unsupported options and unresolved format
dependencies refuse rather than falling back to filename dispatch.
EDI autocomplete may be absent; when present, its dialect must match `edi_kind`.
Only flat new-workbook XLSX is admitted: transposed, composite, grid, worksheet-set,
hierarchical and update-existing layouts refuse. PDF, SQLite, IDoc, SWIFT,
HL7/TRADACOMS, captured external-service boundaries, HTTP acquisition and local
file sets are outside this profile. WSDL metadata remains available on explicit
XML boundaries through the existing XML codec. The bounded XBRL codec enforces
its own closed instance/table profile; taxonomy locations are metadata, not
resource-loading instructions.

The normal engine selection analysis is retained without cloning or reindexing
the project. A selected run fully parses the primary document and each required
static secondary document, including failure-rule dependencies. It does not
decode unused supplied documents, require unused missing documents, or evaluate
unselected targets. All-target runs retain the native requirement for every
declared static source. Required dynamic sources and selected dynamic outputs
refuse; unselected dynamic branches do not affect a static selected run.

## Native limits

The profile reuses immutable native limit sets, not caller-adjustable limits.
Outer payload limits still apply: 64 MiB per document, 256 MiB per input set and
per output set, 4096 additional input documents, 4096 output artifacts,
4096-byte UTF-8 logical paths and 256-byte source names. Supplied unused inputs
still count toward the outer input budget.

| Codec | Admission and output policy |
| --- | --- |
| CSV/TSV, fixed-width | Existing bounded tabular readers: 8 MiB input, 1 MiB/cell, 100,000 records including headers, 256 columns, 1,000,000 nodes. Fixed-width fill counts toward cell bytes. Validated bounded writers use the outer document ceiling. |
| FlexText | Existing native codec: 8 MiB input/output, depth 64, 1,000,000 nodes/records, 1 MiB/value; embedded layout and regex limits remain in force. It is not translated into fixed-width. |
| Flat XLSX | `XlsxLimits::default()`: 16 MiB input/output; 256 ZIP entries; 32 MiB/expanded entry, 64 MiB aggregate expansion; XML depth 64, 2,000,000 XML nodes, 32 MiB XML text; 100,000 shared strings, 128 KiB/string; 100,000 physical rows, 1024 columns; 1,000,000 cells, dense cells and materialized cells; 64 MiB materialized text. The native ZIP/XML preflight rejects unsupported package content, including formulas and external relationships. |
| Protobuf | Existing decoder: 64 MiB/message, depth 128, 1,000,000 wire fields/decoded values. `EncodeLimits::default()`: 64 MiB encoded output and input names/string bytes, 1,000,000 supplied values/fields, depth 128. Embedded descriptor parsing retains its native schema/import limits. |
| XBRL | `XbrlLimits::default()`: 16 MiB input/output/XML text; XML/model depth 64; 1,000,000 XML nodes, attributes and model nodes; 100,000 facts/dimensions; 10,000 contexts/units; 64 MiB model text; 1 MiB/string. Native XML and supported-profile validation occur before projection. |
| JSON, XML, X12/EDIFACT | Existing native schema/format validation and limits, plus the outer payload byte ceilings. No alternate parser or additional process-memory guarantee is introduced. |

See the [XLSX](../../crates/format-xlsx/README.md),
[protobuf](../../crates/format-protobuf/README.md) and
[XBRL](../../crates/format-xbrl/README.md) codec contracts for precisely what
their limits measure and which format structures they admit.

## Failures and empty documents

Global engine failure rules still run before target evaluation/serialization.
Native typed causes, row/field positions and original target/source ownership
remain available through the error chain. Bounded writer resource refusals are
deferred until selected output rendering completes, preserving later format or
destination errors over earlier size refusals. The existing per-artifact,
aggregate-byte and final path-check ordering is unchanged. A refused writer
contributes no partial bytes, and the API returns either the complete selected
artifact set or an error.

Empty CSV/fixed-width inputs are present zero-row documents; an empty valid
protobuf message is a present zero-byte document. Missing secondary inputs are
still missing, not synthesized from emptiness. An empty physical output remains
an artifact, with its original logical path and native record count.

These are document/data budgets. They do not bound all interpreter work,
project size, intermediate mapping values, allocator overhead or process RSS.
Callers needing an execution or process budget must enforce that separately.
