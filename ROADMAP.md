# Ferrule MapForce Workflow-Parity Roadmap

Updated: 2026-07-10

## Goal

Ferrule targets **workflow parity** for common data-mapping work: design a
mapping without editing JSON, run it locally or in automation, inspect how
values were produced, and migrate supported `.mfd` designs with equivalent
results.

Literal MapForce Enterprise parity is not a useful near-term target. That
would include a commercial connector catalog, several language generators,
Windows IDE integrations, AI services, and a scheduling/deployment suite.
Ferrule instead prioritizes a portable Rust runtime, open project format,
clean-room interoperability, and extensible adapters.

## Current Baseline

- Formats, both directions: XML, JSON, CSV, SQLite, X12, and EDIFACT.
- Mapping semantics: nested iteration and broadcast, filters, grouping,
  stable sorting, item limits, conditionals, value maps, lookups, positions,
  seven aggregates, computed aggregate expressions, and 30 scalar built-ins.
- Interfaces: CLI runner/validator/importers, native graph editor, and a WASM
  XML playground.
- `.mfd` survey: 73/120 local MapForce 2026 samples import; 25 import without
  warnings. The survey is diagnostic, not a compatibility claim.
- Known architectural constraints: one primary input and one target instance
  per run, scalar graph outputs, fixed-path extra sources, no trace API, and
  no project-level reusable functions.

## Capability Matrix

| Area | Ferrule now | Workflow-parity target |
| --- | --- | --- |
| XML | XSD subset, includes/imports, attributes, simple content | Namespace identity, `xsi:nil`/`xsi:type`, wildcards, mixed content |
| JSON | JSON Schema subset and local refs | Union/composition schemas, mixed arrays, dynamic properties |
| Flat files | Delimited CSV | Fixed length, reusable structured-text/FlexText-style layouts |
| Database | One SQLite table, full-replace output | Multi-table/query model, keys/relations, insert/update/delete, PostgreSQL |
| EDI | Schema-guided X12 and EDIFACT runtime | `.mfd` EDI/config import, validation reports, pluggable HL7/IDoc/etc. packs |
| Other formats | None | XLSX, Protobuf, then demand-driven XBRL/PDF support |
| Dataflow | One primary source plus named lookup sources; one target | Named N-to-M endpoints, runtime paths/parameters, ordered stage DAG |
| Functions | Scalar subset plus aggregates and scope sequence controls | First-class sequences, conversion/date/math coverage, reusable graph UDFs |
| Execution | Native interpreter, CLI, GUI, browser demo | Packaged runtime, stable library/HTTP APIs, deterministic traces |
| Authoring | Existing-project graph/scope editor | Blank-project authoring, undo/layout, schema wizards, auto-connect, preview |
| Debugging | Static validation and runtime errors | Connector history, context/row inspection, stepping, breakpoints |
| `.mfd` | Best-effort common XML/JSON/CSV/SQLite subset | Executable common profile plus actionable repair workflow |
| Code generation | None | Optional XSLT 3 for XML-only mappings; portable Rust artifact first |

## Workstreams

### A. Mapping Semantics and Interoperability

#### A1. Executable `.mfd` Common Profile

Build breadth only where imported mappings can execute equivalently.

- Normalize legacy and namespace-indexed XML entry encodings.
- Add first-class generated sequences: `distinct-values`, `tokenize`,
  `tokenize-by-length`, `generate-sequence`, and sequence slicing.
- Expand high-value conversion, date/time, math, and node functions.
- Import static generic XML type selections and inline graph-backed UDFs.
- Complete namespace, `xsi:nil`, and JSON union semantics.
- Keep every fixture self-authored; use vendor samples only as black-box
  behavioral references.

Exit criteria:

- A curated XML/JSON/CSV common-profile suite executes with equivalent values.
- At least 50 of the currently importable 73 survey mappings are warning-free.
- Unsupported constructs retain one actionable warning and partial import.

#### A2. N-to-M Endpoint and Stage Model

Replace the single-target assumption before adding multi-file special cases.

- Named source and target endpoints with runtime-overridable locations.
- Ordered target writes and deterministic failure semantics.
- Intermediate target-as-source stages represented as a DAG.
- Dynamic filenames and collection/file expansion.
- Backward-compatible loading of current `Project` JSON.

Exit criteria:

- A mixed-format two-source/two-target mapping runs deterministically.
- A three-stage mapping chains an intermediate result without temporary
  project files.
- Split-file and group-into-blocks designs have a faithful runtime model.

#### A3. Extensible Function System

- Typed function metadata: category, signature, documentation, and purity.
- Graph-backed reusable UDFs with scalar and sequence parameters.
- Process/plugin adapters for custom code without embedding arbitrary native
  libraries in the engine.
- Expand built-ins by measured `.mfd` and user demand, not raw catalog count.

### B. Authoring and Inspection

#### B1. Editor Integrity

This precedes larger GUI features.

- One mutation/session layer for `Project` plus canvas state.
- Dirty tracking and unsaved-change guards.
- Undo/redo and persisted node layout.
- Eliminate hidden placeholder nodes and graph/canvas divergence.
- Refresh scope/binding wires immediately after side-panel edits.

Exit criteria:

- Every edit survives save/open identically.
- Disconnect, undo, and redo are lossless.
- No graph node exists invisibly after a GUI action.

#### B2. Self-Hosting Mapping Authoring

- Source/target wizards for XSD, JSON Schema, CSV, and database tables.
- Extra-source CRUD and format options.
- Scope add/remove plus target-driven scope skeleton generation.
- Searchable categorized function palette with correct initial pins.
- Auto-connect matching children and explicit subtree expansion.

Exit criteria:

- XML-to-JSON and CSV-plus-SQLite lookup mappings can be created from a blank
  project without hand-editing JSON, paths, or node IDs.

#### B3. Preview, Validation, and Debugging

- In-memory preview that does not require saving or an output filename.
- Navigable diagnostics showing all issues, including arity/type checks,
  disconnected graph paths, and required target fields.
- Engine `TraceSink` events for node values, scope candidates, filters,
  groups, sorts, limits, bindings, and partial target writes.
- Value history, source context, row navigation, then breakpoints and stepping.

Exit criteria:

- Every validation issue focuses its owning graph/scope/schema item.
- A nested filtered/grouped fixture produces a deterministic trace.
- A breakpoint can pause before a target write and expose partial output.

#### B4. Shared Native and Browser Editor

- Extract editor/session logic shared by native GUI and WASM.
- Browser project upload/download, local persistence, full graph edits,
  validation, and preview for XML/JSON/CSV text inputs.
- Keep native and browser behavior under the same authoring tests.

### C. Runtime and Connector Breadth

Prioritize connectors that align existing strengths before product-catalog
breadth.

1. Import MapForce EDI components/configurations into the existing X12 and
   EDIFACT runtime; make additional standards external schema packs.
2. Multi-table/query database IR and PostgreSQL adapter.
3. Fixed-length/structured text and XLSX.
4. Protobuf and generic HTTP/OpenAPI/GraphQL endpoints.
5. XBRL and PDF only with demonstrated use cases and maintainable libraries.

Runtime support proceeds in parallel:

- Mapping package containing project, schemas, and relative resources.
- Stable Rust library API, JSON diagnostics/traces, stdin/stdout, parameters,
  and deterministic CLI exit codes.
- HTTP service adapter. External schedulers remain preferred over building a
  FlowForce equivalent.

## Release Gates

### 0.2 - Trustworthy Migration and Editing

- A1 common-profile work underway with regression fixtures.
- B1 editor integrity complete.
- `.mfd` warning report is structured and navigable.

### 0.3 - Self-Hosting Mapper

- B2 blank-project authoring complete.
- B3 in-memory preview and navigable validation complete.
- CLI can use stored endpoint defaults and machine-readable diagnostics.

### 0.4 - Multi-Endpoint Runtime

- A2 endpoint/stage DAG complete.
- Packaged runtime plus trace API.
- Multi-output and chained mappings execute in GUI and CLI.

### 0.5 - Professional Data Integration

- Existing EDI runtime integrated with `.mfd` EDI components.
- Multi-table SQLite/PostgreSQL, FLF, and XLSX production paths.
- Reusable UDFs and shared native/browser editor core.

### 1.0 - Common Workflow Parity

Five release journeys require no hand-edited project JSON:

1. Create XML-to-JSON, auto-connect matching fields, preview, and run.
2. Enrich CSV from a database source and write two target formats.
3. Inspect a nested filtered/grouped/sorted aggregate through a trace.
4. Import `.mfd`, navigate warnings, repair, run, and export the common subset.
5. Reuse one UDF in two mappings and run a packaged project headlessly.

## Explicit Non-Goals

- Byte-identical `.mfd` output or execution of proprietary `.mfx` binaries.
- Recreating the MapForce Windows UI, AI Server, FlowForce scheduler, or IDE
  integrations.
- Near-term C++/C#/Java generator parity.
- Bundling every database driver, EDI release, XBRL taxonomy, Shopify
  specialization, or PDF/OCR engine.
- Copying vendor sample content into this repository.

## Scorecard

Update these numbers with each parity increment:

- Workspace tests: 124.
- `.mfd` survey: 73/120 import, 25 warning-free.
- Target-path mismatch warnings: 14, down from 36.
- Non-repeating structural-group warnings: 11, down from 28.

## Primary References

- [MapForce 2026r2 changes](https://www.altova.com/mapforce/whatsnew)
- [MapForce product and format scope](https://www.altova.com/mapforce)
- [Edition comparison](https://www.altova.com/mapforce/editions)
- [Database mapping](https://www.altova.com/mapforce/database-mapping)
- [Function library](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_lib.html)
- [Multiple targets and chaining](https://www.altova.com/manual/mapforce/mapforceprofessional/mf_rules_multtargets.html)
- [Debugger](https://www.altova.com/manual/Mapforce/mapforceprofessional/mff_debug.html)
- [User-defined functions](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_udf.html)
