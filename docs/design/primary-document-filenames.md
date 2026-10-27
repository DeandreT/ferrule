# Primary document filename plans

`mapping::DocumentProject` is a distinct, versioned artifact containing a
mapping project and its primary filename graph root. It retains acquisition
metadata without changing the legacy `Project` format or its saved preview
paths.

The serialized wrapper has three required fields:

- `format`: `"ferrule_document_project_v1"`.
- `project`: the embedded mapping project.
- `primary_source`: an object with a `filename` node ID from that project's
  graph and an optional `selection`.

`selection` states how the evaluated filename selects documents:

- Absent or `"direct"`: the filename names one document; `*` and `?` are
  ordinary characters.
- `{"file_name_wildcard": {"empty": "refuse" | "map_no_documents"}}`: `*` and
  `?` in the final path component select every matching file in one directory,
  in Unicode scalar order of their names; directory components never contain
  wildcards. `empty` states whether a wildcard that matches no file is a typed
  selection failure or a run over zero documents.

A direct plan is saved without `selection`, so its bytes are unchanged. A
saved wildcard plan is refused by readers that predate the field rather than
being read as a direct filename.

Unknown wrapper and primary-plan fields are rejected. A serialized wrapper
cannot be read as an ordinary project, because the project fields are nested.
The embedded project is exposed by shared reference, not by mutable reference.

For file persistence, use `mapping::document_project_file::{encode_pretty,
decode_str, decode_bytes}`. This reuses the bounded mapping-file codec with the
distinct `document_project` kind. When required, its version-2 outer envelope
retains every finite float's type and exact bits throughout the embedded project.
Ordinary model serde is not the lossless file contract. Non-finite model floats,
invalid UTF-8 and mismatched versioned artifact kinds are refused.

## Source-independent classification

`DocumentProject::global_filename` classifies the initial filename forms:

- A string constant retains its exact text, without path normalization.
- A string runtime parameter retains its exact name. Its name must meet the
  existing nonempty, NUL-free, 256-byte parameter-name limit.

The classifier does not use saved preview paths or parameter preview values.
It performs no filesystem access and does not decide wildcard, URI, decoding,
parsing or publication policy. Missing nodes, non-string values, invalid
parameter names and unsupported expressions have distinct typed errors.
Unsupported expressions remain in the artifact rather than being replaced.

`codegen_runtime::ExecutionContext::resolve_runtime_parameter` and C#
`FerruleExecutionContext.ResolveRuntimeParameter` resolve host scalars before
source acquisition. They share the existing scope-context coercion rules and
typed missing/type errors. Explicit null stays null, and source fields cannot
supply missing host parameters. Resolving a scalar does not itself validate a
filename, select a document or perform I/O.

## Graph retention

`DocumentProject::prune_unreachable_nodes` retains the primary filename node
and its dependencies in addition to the ordinary project's target, control,
failure and dynamic-source roots. This applies even when the filename
expression is unsupported by the classifier. Ordinary project pruning keeps
its existing roots and behavior.

`engine::evaluate_primary_filename` evaluates the classified literal or global
parameter without a source instance. It preserves the exact string, reuses
ordinary parameter coercion, and reports explicit null results separately from
missing parameters. Neither saved previews nor parameter previews supply a
filename, including in a preview execution context. Unrelated mapping nodes are
not evaluated.
Success validates only this filename expression, not the embedded mapping body.

Document loading, source ownership and output-document execution require their
own validated profiles. Filename evaluation does not interpret empty strings,
wildcards, URI schemes or filesystem syntax, and does not enable I/O.

## Acquisition ownership

The `document-source` crate owns the format-neutral `SourceData`,
`PrimarySourceHost`, `PrimaryDocument`, acquisition and assembly APIs. A host
returns bytes and an optional resolved path; the caller supplies the parser.
Acquisition evaluates the filename and selects once, then loads and parses each
request in host order before returning the complete document list. Its errors
retain the filename, selection, request, load and parse stages.

`LocalPrimaryFiles` is an explicit local-file policy in that crate. Its lexical
and symlink checks, direct-versus-wildcard behavior and per-document byte limit
are unchanged. They do not provide atomic confinement against concurrent file
replacement. Other hosts need not use local filesystem policy.

CSV parsing, lazy row acquisition and physical row occurrences remain in
`format-csv`. Existing `format_csv` host and acquisition exports are compatible
re-exports; `format_csv::CsvSourceData` names the same type as
`document_source::SourceData`. `codegen_runtime` keeps its existing public names
and also exposes the neutral type. Callers may migrate imports without adapting
host implementations or changing saved projects. Generated C# entry points and
their public host contracts are unchanged.

## Generated filename companions

`codegen::lower_document` produces a `DocumentProgram` that retains a separate
`PrimaryFilenamePlan` alongside the ordinary lowered mapping. It applies the
same closed filename classifier and ordinary mapping validation; it does not
silently lower an unsupported filename as if acquisition were absent.

The Rust and C# `emit_with_primary_filename` entry points add
`evaluate_primary_filename` / `EvaluatePrimaryFilename` to the ordinary typed
instance library. Each takes only the execution context, not a source instance.
The method preserves literal text and resolves parameter values without using
previews. Explicit nulls produce a typed `PrimaryFilenameType` error rather than
an empty path or a missing-parameter fallback.

These companions expose filename evaluation, not document execution. Ordinary
instance mapping remains a separate, explicitly pre-acquired-input operation.
Neither the companions nor the ordinary instance entry points perform source
acquisition or claim filesystem, wildcard, parser or publication support.

## Importing connected CSV filename metadata

`mfd::import_csv_document` and `import_csv_document_with_options` return an
`ImportedDocument` containing a `DocumentProject`, the ordinary import warnings
and the mapping path. They retain the connected primary CSV `FileInstance`
input expression before pruning its graph dependencies. The selected source's
data ports identify the owning declaration; a component label or declaration
order alone is insufficient. Ambiguous feeds, missing connections and unsupported
wrapper controls are refused.
An optional header containing an empty namespace list, or the native empty and
MapForce protocol namespace pair, is metadata rather than another file wrapper.
Declared wrapper namespace slots must resolve to the protocol domain. Other
header content, unresolved slots and multiple headers remain refused.

This is a repair-oriented metadata import, not an executable profile. All
ordinary warnings, including unsupported input/output filename behavior, remain.
A retained source-dependent expression still fails the filename classifier and
document lowering. The APIs neither read saved preview instances nor replace
the connected expression with a saved path. Ordinary import, pipeline import
and `ImportProfile::Executable` keep their existing behavior.

## Complete primary documents

Filename evaluation, selection, loading, parsing, mapping and publication are
separate stages:

1. The saved format identities decide the document route (below). An
   unrouted format, an unsupported filename expression and an invalid mapping
   are refused before the filename is evaluated or any host is called.
2. The filename plan is evaluated once, without a source instance or preview.
3. A `document_source::PrimarySourceHost` receives the filename text and the plan's
   selection mode, and returns logical requests in load order. The host
   chooses where requests load from; it does not choose the selection mode. A
   direct filename must select exactly one request.
4. The host loads each request's bytes, which are parsed as one document of
   the route's format. A request's own extension never selects a format.
5. Only after every selected document is parsed does the ordinary mapping run,
   once, over the source the route forms from them. Grouping, sorting and
   aggregation therefore span documents, and every document shares one
   execution context. The mapping never runs per discovered file.
6. The caller publishes the result.

The first failure ends the run before any later request is loaded and before
the mapping runs. `engine::run_primary_document` takes the per-document
parser and returns the target instance; `engine::run_primary_csv_document`
supplies the CSV parser. Failures keep their stage: `Route`, `Invalid`,
`Acquisition` (filename, selection, direct-selection count, request,
document limit, load or input), `Assembly` or `Mapping`.

### Document routes

`DocumentProject::primary_route` classifies the source by its explicit format
options, else by the extension of its saved source path, which is used only as
a format identity and never read. The target must name the same format.
Named sources and targets and incompatible format options are refused before
filename evaluation or any host call, including when a wildcard would be empty.

| Format | Identity | Source the mapping reads |
| --- | --- | --- |
| CSV | `tabular_kind: csv`, or a `.csv` saved path | Every document's rows, in selection then row order; each row keeps its own document origin and local position. |
| Fixed-width | An explicit `fixed_width` layout on both endpoints | Every selected document's rows, in selection then row order, in one combined row run. Each row retains its logical/resolved document identity and physical one-based position. |
| XML | `xml_document`, or a `.xml` saved path | A direct filename: its one document. A filename wildcard: a document set of every selected document in selection order, each member keeping its request as its path and the host's resolved location as its source path. |
| JSON | `json_document`, or a `.json` saved path | A direct filename: its one strict JSON document. A filename wildcard: an ordered document set, with the same member-path and shared-context semantics as XML. |

An XML document set is read through the engine's ordinary document-set
iteration, as a local XML file set is: a scope or aggregate path descends into
every member, so grouping spans documents. A source that declares
`local_xml_file_set` describes a document set and is refused with a direct
filename; an observed XML root view describes one document and is refused with
a filename wildcard. Every member is an ordinary XML document of the source
schema.

EDI plans select X12, EDIFACT, HL7 v2, TRADACOMS, or IDoc explicitly in their
saved options, or through an unambiguous `.x12`, `.edifact`, `.hl7`,
`.tradacoms`, or `.idoc` identity. A bare `.edi` never selects a dialect.
IDoc requires an embedded layout. Unresolved configurations, conflicting
options, and incompatible schemas/layouts fail before any host call. Wildcards
form a path-bearing document set, not a concatenated segment stream. Each
member is parsed in order and the complete set is mapped once. Native and
generated readers also retain explicitly selected lenient partial schemas:
unmentioned envelope segments may be skipped without using the schema to guess
the dialect. Strict sources must admit their selected family's opening segment.
The native CLI and generated Rust use the shared EDI boundary; generated C# uses package-free
schema-guided codecs. Input is capped at 64 MiB per document and 256 MiB per
run; output is capped at 64 MiB. IDoc also caps each document at 100,000 records.
SWIFT MT remains input-only and cannot provide a same-format output.

EDIFACT syntax is selected by UNB's S001 version component. Versions 1–3 keep
`*` as literal data; version 4 and later enable typed element repetitions,
using UNA's repetition character when supplied and `*` otherwise. Writers
normalize to the default separators and preserve repeated scalar/composite
instances. Standalone UNH input without a UNB version retains legacy syntax;
UNA's reserved/repetition slot alone does not choose version 4.
Service characters belong to each interchange: a UNA at the next closed
interchange boundary replaces the previous advice, while no UNA restores the
defaults. The same reset applies after a standalone UNT. A UNA-looking field
value never changes syntax. Schemas still decide which segments and repeating
interchange containers are admitted; parsing concatenated interchanges does not
turn a singular schema into a repeating one.

The shared C# codec is `FerruleEdi(schemaJson, optionsJson, kind, input: false,
strictEnvelope: false)`, with `Parse(byte[])` and
`Serialize(FerruleInstance, currentDateTime)`. Adapters include it through
`codegen-csharp::edi_runtime::add_runtime`; there is one codec inventory for
primary and static wrappers. `strictEnvelope: true` is opt-in for EDIFACT and
checks UNB/UNZ, UNG/UNE and UNH/UNT ownership, control counts and references on
input and on the completed output wire. `ValidateEnvelope(byte[])` exposes
the same check independently. Failures carry `FerruleEdiError.Envelope` and a
zero-based `SegmentIndex`. Native code exposes
`format_edi::edifact::validate_envelope(&str)`; Rust generated hosts can call
`codegen_runtime::validate_edifact_envelope(&[u8])`. Ordinary primary plans
retain their existing schema-guided, non-strict envelope behavior.

Other formats have no route yet and are refused with a typed `NotAdmitted`
error naming the format.

JSON5 and JSON Lines are not part of the strict-JSON route. Their explicit
options and saved-path identities are refused before any host call; their
syntax is never silently accepted as strict JSON. JSON document-set paths are
available only under a wildcard document plan, not ordinary single-document
projects. Named endpoints and dynamic JSON output documents are refused rather
than dropping outputs. JSON input is bounded to 64 MiB per document and 256 MiB
in total, charged before parsing each member. Output is measured before writing
its bounded 64 MiB buffer.

JSON wildcards preserve whole documents, including array roots; they do not
concatenate JSON arrays as if they were CSV rows. An empty-path iteration at
the document-set level visits documents, and a nested empty-path iteration
visits an array member's items. Nonempty collection paths descend through each
member using the ordinary document-set traversal, allowing grouping and
aggregation across those collections in one run.

### Generated entry points

Fixed-width plans use the native validated `FixedWidthLayout`: widths count
Unicode scalar values, one leading UTF-8 BOM is ignored, delimited input accepts
LF and CRLF, and undelimited input is split into exact-width contiguous records.
Only trailing fill is trimmed. Empty fields follow `treat_empty_as_absent`; writes
right-pad with the layout's fill and use LF or no record delimiter. Partial or
oversized records and typed scalar failures stop before any later load. Source
and target layouts may differ. Incompatible options, non-flat schemas and width
counts are refused before host calls. There is no implicit layout from a suffix.
Input is bounded to 64 MiB per document and 256 MiB in total; output is measured
before allocating its bounded 64 MiB document buffer. Physical row origins are
metadata, distinct from the mapping's ordinary combined-row `Position` values.

Generated libraries add the same entry point beside the format's existing
single-document adapter, and the target uses that adapter's writer. They take
the execution context, a host and a document limit, and return one complete
document. They accept no filename or document content from the caller; the
embedded plan alone names the documents, and `PRIMARY_FILE_SELECTION` /
`PrimaryFileSelection` records its selection mode.

- CSV: `codegen_rust::emit_with_primary_csv_documents` and
  `codegen_csharp::emit_with_primary_csv_documents` extend the CSV document
  adapters with `execute_primary_csv_document_bytes` /
  `ExecutePrimaryCsvDocumentBytes`, which also take a byte-decoding policy.
  Rust failures are `PrimaryCsvDocumentError::{Acquisition, Output}`; C#
  raises the acquisition exception, with the host's exception as its cause,
  or the mapping and CSV output exceptions.
- Fixed-width: `emit_with_primary_fixed_width_documents` adds
  `parse_fixed_width_bytes` / `ParseFixedWidthBytes`,
  `serialize_fixed_width_bytes` / `SerializeFixedWidthBytes`, and
  `execute_primary_fixed_width_document_bytes` /
  `ExecutePrimaryFixedWidthDocumentBytes`. Rust preserves acquisition, assembly
  and output stages through `PrimaryFixedWidthDocumentError` and native format
  errors. C# retains acquisition causes and exact fixed-width record/field/limit
  details, reusing the shared scalar readers and writers. The compatibility
  manifest profile is `primary_fixed_width_document`.
- XML: `emit_with_primary_xml_documents` requires a program with the
  single-document XML adapter and no named endpoints, and adds
  `execute_primary_xml_document_bytes` / `ExecutePrimaryXmlDocumentBytes` and
  `PRIMARY_DOCUMENT_SHAPE` / `PrimaryDocumentShape`. Members are read with the
  adapter's structured or observed root-view reader. A plan that declares
   `local_xml_file_set` is validated with its original document-set contract,
   including current-document-path expressions. Only adapter selection uses
   member options, because the entry point forms the document set itself.
   Saved-path-only XML identities select the same adapter. Rust failures are
  `PrimaryXmlDocumentError::{Acquisition, Assembly, Output}`, where a mapping
  failure is the cause of an `Output`; C# raises
   `FerrulePrimaryDocumentException`, with the host's or reader's exception as
   its cause, or `FerruleXmlOutputSetException`.
- JSON: `emit_with_primary_json_documents` adds
  `execute_primary_json_document_bytes` / `ExecutePrimaryJsonDocumentBytes`,
  with the same selection and shape constants. It uses the existing bounded
  schema-shaped strict JSON reader and writer. Rust failures are
  `PrimaryJsonDocumentError::{Acquisition, Assembly, Output}`; the input and
  output causes are `JsonBoundaryError`. C# preserves acquisition stages in
  `FerrulePrimaryDocumentException` and mapping/output failures in
  `FerruleRuntimeException`. C# input byte-limit causes retain the measured
   `Bytes` and `Maximum` in `FerrulePrimaryJsonInputException`.
- EDI: `emit_with_primary_edi_documents` adds
  `execute_primary_edi_document_bytes` / `ExecutePrimaryEdiDocumentBytes`.
  The saved source and target retain independent same-family options. Rust
  exposes `PrimaryEdiDocumentError::{Boundary, Acquisition, Assembly}` and
  `EdiBoundaryError`; C# preserves acquisition causes and raises
  `FerruleEdiException` for codec failures. The compatibility manifest profile
  is `primary_edi_document`. Each embedded schema and options descriptor is
  bounded to 1 MiB and validated before emission and before host calls.

## Local filesystem host

`document_source::LocalPrimaryFiles` (C# `FerruleLocalPrimaryFiles`) is the explicit
local filesystem host. Nothing else performs filesystem access, and no host
reaches a network: a host that wants another transport must be supplied
explicitly.

- Filenames are relative paths beneath one configured root. `/` and `\` both
  separate components; empty and `.` components are ignored, and requests
  use `/` between the remaining components.
- URI schemes, rooted, drive and UNC paths, parent components and any other
  colon are refused before the filesystem is touched.
- Every component beneath the root must be a directory or regular file, not a
  symbolic link. A wildcard selects regular files only, skips directories and
  refuses a matching link. These checks are not atomic against concurrent
  changes to the tree.
- Matching is case-sensitive over Unicode scalars; names that are not valid
  Unicode never match.
- Each document is bounded by a byte limit, 8 MiB by default.

Typed refusals are `Root`, `Uri`, `Rooted`, `Parent`, `InvalidName`,
`WildcardDirectory`, `NoMatches`, `Missing`, `Symlink`, `NotRegular`,
`TooLarge` and `Io`. A missing direct file is a load failure; a missing
wildcard directory or a refused empty match is a selection failure.

The portable cohorts in `conformance/primary-filename-documents` (CSV) and
`conformance/primary-xml-documents` (XML), and
`conformance/primary-json-documents` (strict JSON) compare complete documents, typed
failures and the ordered host calls of the interpreter, generated Rust and
generated C#.

`conformance/primary-fixed-width-documents` covers the fixed-width policies and
combined row semantics through the same interpreter/generated-host harness.
`conformance/primary-edi-documents` covers the writable EDI families through
that harness, including cross-document grouping and path identity.

## Command-line, editor and generation routes

A saved document project is the `document_project` file artifact; it is never
read as an ordinary project, and an ordinary project is never read as a
document project.

- `ferrule run-document --project DOCUMENT --input-root DIR` runs it through
  the local host. `--max-document-bytes` and `--max-documents` bound the
  selection, `--param NAME=VALUE` supplies parameter filenames and mapping
  values, and `--output` defaults to the saved target path. Documents are read
   and the target written in the route's format. The output is published by
  staged replacement only after the whole run succeeds, and never replaces the
  project or a document the run read. `cli::execute_primary_document` is the
  same run without publication, and `cli::execute_primary_document_with_host`
   runs it through any host.
   A saved output suffix never changes the admitted format. CSV serialization
   writes into a bounded output buffer and refuses excess output before
   publication.
- `ferrule generate --primary-documents` (also `--primary-csv-documents`)
  generates the document library for the plan's route; `--raw-delimited-crlf`
  selects raw output for a CSV route. The generated identity and compatibility
  manifest options apply, and the manifest records the `primary_csv_document`,
   `primary_raw_delimited_document`, `primary_xml_document` or
   `primary_json_document` profile. The
  backends' `Generator::emit_with_primary_csv_documents` and
   `emit_with_primary_xml_documents` and `emit_with_primary_json_documents`
   methods return the free functions'
  artifacts under default options.
- The editor opens a document project with its filename plan, saves the plan
  back unchanged and shows an input root (initially the project's folder) and
  limits in place of the input file. Run publishes through the command-line
  run; Preview performs the same run in memory. Debug Run, Debug Preview and
  library generation are not offered for document projects.
