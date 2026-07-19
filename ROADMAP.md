# Ferrule MapForce Workflow-Parity Roadmap

Updated: 2026-07-19

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

- Formats, both directions: XML, JSON, CSV/fixed-width/FlexText, SQLite, XLSX,
  XBRL instances, X12, EDIFACT, HL7 v2, TRADACOMS, embedded IDoc/SWIFT MT layouts,
  and proto2/proto3 Protocol Buffers; visual PDF extraction is source-only.
- Mapping semantics: nested iteration and broadcast, filters, grouping,
  stable distinct-value iteration, literal/length/regex tokenizer and integer-range sequences,
  bounded existential reduction and 1-based scalar selection over generated sequences,
  stable sorting, ordered skip/first/from/range/last sequence windows,
  conditionals, value maps, lookups,
  duplicate-preserving multi-source inner equijoins, positions, seven
  aggregates, computed aggregate expressions, root and nested dynamic JSON target
  properties, per-item dynamic typed document sources, multiple mapped outputs,
  structural group projection, mapped and computed XML occurrence sequences,
  contiguous boundary-driven grouping, bounded recursive filter/path/adjacency
  constructions, and an expanding scalar function library.
- Interfaces: CLI runner/validator/importers with JSON Lines diagnostics,
  stored endpoint defaults, native graph editor with dirty-state guards,
  undo/redo, and persisted canvas layout; plus a WASM XML/JSON/CSV/XBRL
  playground.
- `.mfd` survey: all 120 local MapForce 2026 samples import warning-free and
  engine-valid, then export, re-import, and validate warning-free. The safe execution
  survey runs 113 samples without network access or writes to the read-only sample tree;
  all 113 execute and all 113 export, re-import, validate, and execute with zero
  semantic output drifts. Across the current isolated behavioral manifests, all 79
  available deterministic references match exactly.
  These measurements describe the local sample profile, not commercial-product parity.
- Known architectural constraints: one primary driver input per run, scalar graph
  outputs, no general endpoint/stage DAG, no trace API, and no project-level
  reusable functions.

## Capability Matrix

| Area | Ferrule now | Workflow-parity target |
| --- | --- | --- |
| XML | XSD subset, includes/imports, attributes, simple and ordered mixed content, `xsi:nil`, generic elements, and bounded transitive derived-type input/output | Namespace identity and remaining derived-type input shapes |
| JSON | JSON Schema subset, local refs, compatible closed-object `oneOf`/`anyOf` with required string, boolean, signed-integer, or finite-number `const` discriminators, typed dynamic properties | Scalar/array and nested union composition, optional or null discriminators, mixed arrays, unconstrained dynamic values |
| Flat files | Delimited CSV, fixed length, reusable FlexText layouts, and bounded string-fed parsing | Additional FlexText commands and parser variants |
| Database | Relational SQLite reads and full-replace writes, imported WHERE/ORDER controls, static/correlated queries, and deterministic generated keys | General query model, insert/update/delete, PostgreSQL |
| EDI | Bounded X12/EDIFACT/HL7/TRADACOMS runtime plus embedded IDoc/SWIFT layouts and executable `.mfd` configurations | Validation reports, additional configuration commands, and pluggable release packs |
| Other formats | XLSX including hierarchical and update-existing targets, native XBRL instances, proto2/proto3 input/output, static HTTP XML sources, and visual PDF sources with page selection, vertical collages, marker groups, and table layouts | XBRL taxonomy/formula/linkbase execution plus remaining PDF extraction variants and PDF targets |
| Dataflow | One primary driver plus named static/dynamic and wildcard document sources, multiple mapped targets, and dynamic per-document output paths | Fully general named N-to-M endpoints, runtime parameters, ordered stage DAG |
| Functions | Scalar subset plus aggregates, generated-sequence reducers, and ordered scope sequence windows | General sequence composition, conversion/date/math coverage, reusable graph UDFs |
| Execution | Native interpreter, explicit host-value context, CLI, GUI, browser demo | Packaged runtime, documented library/HTTP APIs, deterministic traces |
| Authoring | Existing-project graph/scope editor plus XSD/JSON blank-project setup, scope management, extra-source CRUD, undo, and layout | Complete schema/format wizards, extra-target editing, auto-connect, and preview |
| Debugging | Static validation and runtime errors | Connector history, context/row inspection, stepping, breakpoints |
| `.mfd` | 120/120 warning-free import/export/re-import validation, 113/113 safe original and round-trip executions without semantic drift, and 79/79 available deterministic references exact | Broader behavioral-reference coverage and lossless execution for the remaining supported edge profiles |
| Code generation | [Portable Rust and package-free C# libraries](docs/code-generation.md) with shared lowering, typed failures, host runtime values, ordered value maps, scalar/group targets, exact whole-group copies, source/generated iteration, controls, aggregates, recursive-collect generated sequences, and generated-sequence reducers | Broaden toward interpreter parity, publish the Rust runtime, and consider optional XML-specific XSLT |

## Workstreams

### A. Mapping Semantics and Interoperability

#### A1. Executable `.mfd` Common Profile

Build breadth only where imported mappings can execute equivalently.

Progress: legacy indexed XML names, stable `distinct-values` pipelines, first-class
`tokenize`/`tokenize-by-length` sequences, and inclusive `generate-sequence` ranges
are implemented. Generated and source-backed scopes execute and export ordered
skip/first/from/range/last windows after sort/filter/group controls with stage-correct
positions, while generated sequences can feed scalar `item-at`.
Compatible JSON object alternatives preserve required typed scalar `const`
discriminators and can drive exact derived XML type output. Transitive concrete XSD descendants are
discovered through abstract include/import intermediates. Database WHERE/ORDER controls lower into runtime scopes,
static and foreign-key-correlated queries recover executable SQLite sources,
embedded correlated catalog queries recover executable relational sources,
standalone max-one queries preserve empty/single document-root cardinality,
structured lookup UDFs lower to named secondary-source lookups or zero-to-many
constructed records, while filtered sequence-parameter UDFs can construct one
flat aggregate record. Filtered tokenizer sequences lower to executable existential
reducers, and selected sibling values lower to round-trippable lookups. Static XML
catalogs inside scalar lookup UDFs become named lookup sources, while designs with
only core output parameters synthesize an executable typed target. Mapping-path and
stable per-run clock values use explicit host context, root and nested computed JSON
targets, plain structural group copies, and filtered
or generated XML occurrence sequences lower exactly. `group-starting-with`
partitions filtered rows into contiguous groups and round-trips through `.mfd`.
Repeating copy-all groups retain their scalar descendants, `xsi:nil` remains
distinct from absent values, and inclusive ranges accept exact integral decimal
inputs.
High-value date/time/duration/missing-value functions execute natively. Non-representable
operator order produces an actionable warning instead of silently claiming exact
conversion. Core kind-32 joins lower to typed left-deep plans with composite equality
keys, duplicate-preserving execution, projected fields, flattened positions, and
filter/sort/window controls. Naked joined tuples can be counted or reduced through
a computed scalar expression, including aggregate-only joins with an independent root
plan. Nested non-repeating target projections reuse the owning tuple, while rejected
join shapes suppress redundant downstream warnings. Canonical export round-trips
root-context joins whose collections all belong to the primary source, including raw
tuple counts and computed joined aggregate values with parent-context scalar arguments.
Named static XML, JSON, flat-file, and database sources now retain separate component ownership
during export; per-item dynamic XML sources and captured HTTP POST response
boundaries also round-trip with their typed contracts.
The versioned compatibility survey records import, engine validation, export,
re-import, and post-export validation separately. All 120 local designs now pass
those structural stages without warnings. A separate isolated execution survey runs
113 safe originals successfully; all 113 exportable/re-importable executions match
semantically. The current isolated manifests provide 79 deterministic references, all
of which match exactly. Reference manifests remain a separate behavioral measure and
are not inferred from structural success.

- Preserve complete warning-free, engine-valid import coverage while expanding
  the supported component surface.
- Preserve isolated, safely redirected execution and semantic reference comparison
  without writing into the read-only vendor sample tree.
- Expand behavioral reference coverage across format-specific and mixed-content edge
  cases, especially workflows whose vendor outputs require unavailable services.
- Add general sequence composition and reusable graph-backed UDFs instead of further
  one-off lowering paths.
- Complete namespace identity, remaining derived-type input shapes, and the remaining
  scalar/array/nested or optional/null-discriminated JSON union semantics.
- Keep every fixture self-authored; use vendor samples only as black-box
  behavioral references.

Exit criteria:

- A curated XML/JSON/CSV common-profile suite executes with equivalent values.
- All warning-free imported survey projects pass engine validation.
- The survey records redirected execution and reference comparison separately
  from syntactic import success.
- Supported export profiles re-import without warnings and remain engine-valid.
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

Progress: serialized-project dirty tracking, destructive-action guards,
bounded/coalesced project undo/redo, versioned layout sidecars, and visible,
lossless input placeholders are implemented. Layout fingerprints prevent stale
sidecars from reclassifying project nodes.

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

1. Complete namespace identity, remaining `xsi:type` shapes, and remaining JSON union semantics.
2. Add a general query/database mutation IR and PostgreSQL adapter.
3. Expand remaining XLSX, FlexText, EDI, and PDF layout variants by measured demand.
4. Add generic HTTP/OpenAPI/GraphQL endpoints and dynamic protobuf document sources.
5. Add XBRL taxonomy/formula/linkbase execution only with maintainable libraries.

Runtime support proceeds in parallel:

- Mapping package containing project, schemas, and relative resources.
- Evolving Rust library API, JSON diagnostics/traces, stdin/stdout, parameters,
  and deterministic CLI exit codes. Stabilization follows the mapping and endpoint
  model rather than constraining pre-1.0 refactors.
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
- Near-term C++/Java or commercial generator-catalog parity.
- Bundling every database driver, EDI release, XBRL taxonomy, Shopify
  specialization, or PDF/OCR engine.
- Copying vendor sample content into this repository.

## Scorecard

Update these numbers with each parity increment:

- Workspace tests and strict all-target clippy pass on the pinned nightly.
- `.mfd` import: 120/120 imported, 120 warning-free, zero rejected.
- `.mfd` validation: all 120 imported projects are engine-valid.
- `.mfd` export/re-import: all 120 designs export, re-import, and validate without
  warnings in the structural survey.
- `.mfd` execution: all 113 network-independent, non-mutating originals execute.
- `.mfd` execution round trips: all 113 safe projects export, re-import, validate,
  execute, and produce semantically identical outputs.
- Behavioral references: 79/79 available deterministic outputs across the current
  isolated manifests match exactly; these are not inferred from structural success.
- Set `FERRULE_SURVEY_JSON=/path/report.json` for the versioned per-sample
  compatibility report and `FERRULE_SURVEY_DETAILS=1` for text diagnostics.
- CLI diagnostics: versioned JSON Lines cover validation, import/export
  warnings, runtime failures, and invalid command usage.
- CLI run paths: explicit flags override project-relative `source_path` and
  primary `target_path` defaults while stored extra targets retain their own paths.

## Primary References

- [MapForce 2026r2 changes](https://www.altova.com/mapforce/whatsnew)
- [MapForce product and format scope](https://www.altova.com/mapforce)
- [Edition comparison](https://www.altova.com/mapforce/editions)
- [Database mapping](https://www.altova.com/mapforce/database-mapping)
- [Function library](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_lib.html)
- [Multiple targets and chaining](https://www.altova.com/manual/mapforce/mapforceprofessional/mf_rules_multtargets.html)
- [Debugger](https://www.altova.com/manual/Mapforce/mapforceprofessional/mff_debug.html)
- [User-defined functions](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_udf.html)
