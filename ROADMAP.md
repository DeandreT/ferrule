# Ferrule `.mfd` Compatibility and Product-Parity Roadmap

Updated: 2026-10-01

## Goal

Ferrule targets full behavioral compatibility and product-workflow parity with
the Enterprise 2026 Release 2 reference application. A compatibility claim
covers the applicable combination of mapping feature, data format, execution
language, connector, platform, and product workflow. It requires equivalent
values, ordering, artifacts, failures, and external effects; syntactic import
alone is not enough.

Ferrule remains an independent, portable Rust data-mapping platform. Its open
project format, native Rust backend, browser support, bounded execution model,
and Ferrule-specific extensions are first-class capabilities. Compatibility is
implemented as a tested profile over the general Ferrule model rather than by
restricting that model to the reference application's feature or backend matrix.

The implementation tracks six independent promises:

1. `.mfd`/project import and executable behavior.
2. Reference-application acceptance and execution of Ferrule exports.
3. Complete visual authoring and maintenance without hand-editing project JSON.
4. Interpreter and generated-backend behavior for each applicable feature.
5. Debugging, project, library, configuration, packaging, and automation workflows.
6. Ferrule-native behavior and backward compatibility outside the native `.mfd` profile.

Optional AI services and separately licensed server products are versioned
profiles with their own evidence. Pixel-identical UI and byte-identical `.mfd`
serialization are unnecessary when the editable document and workflow semantics
are preserved.

## Current Baseline

- Formats, both directions: XML, JSON, CSV/fixed-width/FlexText, SQLite, XLSX,
  XBRL instances, X12, EDIFACT, HL7 v2, TRADACOMS, embedded IDoc layouts,
  and proto2/proto3 Protocol Buffers. SWIFT MT and visual PDF extraction are
  source-only.
- Mapping semantics: nested iteration and broadcast, filters, grouping,
  stable distinct-value iteration, literal/length/regex tokenizer and integer-range sequences,
  bounded existential reduction, 1-based scalar selection, and
  count/sum/average/minimum/maximum/string-join reduction over raw, filtered,
  or per-item computed generated sequences,
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
  undo/redo, persisted primary/function/named-target canvases, and deterministic
  JSON Lines execution traces; plus a WASM XML/JSON/CSV/XBRL playground.
  The native new-mapping form creates primary FlexText sources and targets
  from independently embedded `.mft` layouts; data paths remain optional and
  original configurations are unnecessary after saving.
  Named FlexText inputs and outputs use the same embedded layouts. CSV setup
  selects custom or disabled quoting and retries previews with the chosen
  dialect. Primary and named flat inputs and outputs can configure
  positional fixed-width layouts with independent Unicode widths and fill settings.
  Canvas deletion protects graph nodes used by other outputs, failure rules,
  dynamic input/output paths, and scope or generated-item ownership; ordinary
  active bindings and graph consumers still disconnect when deletion is allowed.
  Primary and named Inspector scope controls obey the same preview, run, and
  dialog edit locks as the canvases; locked interaction preserves project,
  history, and output while the scope panel remains scrollable.
  Ordered secondary sort keys expose independent expressions and directions,
  insertion, removal, and priority changes. Filtering can run before or after
  sorting; disabling sorting clears every key, and undo restores them.
  Sequence windows can move earlier or later while retaining their bound
  expressions. Whole-group copies retain complete nested content: Inspector
  grouping/binding/child edits and canvas connections into copied ancestors
  reject before changing the mapping; supported filter/sort/window controls remain editable.
  Removing a generated scope retires its private item nodes from every open
  mapping canvas. Remaining graph consumers or owners outside the removed
  subtree block removal with a reference explanation; arguments and unrelated
  graph nodes survive, and undo restores exact item identities and wires.
  Generated item nodes show their indexed scope, reducer, or failure-rule owners
  and remain read-only, including malformed saved paths that match source fields.
  Source wires and Auto-connect reuse only ordinary source fields; Auto-connect
  also reserves absent node IDs retained by generated owners.
  XML child scopes on ordinary singular target groups can select the first
  surviving element or every surviving element; unavailable formats and complex
  schemas retain their saved mode without passive changes.
- `.mfd` survey: all 187 local designs import. The isolated resource profile
  records 169 warning-free imports and 174 dependency-complete, engine-valid
  designs. Four connected chains warn in single-project mode and validate as
  typed pipelines. Twelve designs retain unresolved EDI-catalog dependencies;
  one PDF ObjectFind design retains a persisted repair dependency. It stays
  editable and rejects physical execution and faithful export. The best-effort
  profile exports/reimports 186 designs, 185 warning-free; one retains missing
  target and embedded string-parser JSON Schema provenance. All 174 supported
  round trips remain engine-valid. Native-shaped export remains a static local
  parser check: direct CLI preflight accepts 166/187, including 159 with
  warning-free original imports, and all 166 strict exports reimport cleanly.
  The isolated preview profile executes all 167 safe-input designs, publishes
  163 captured outputs, and records one expected SQLite target-constraint
  failure. Seven network or captured-service inputs are unavailable. The
  round-trip profile records 166 semantic matches, zero drifts, and one explicit
  unsupported SQL preview-contract skip. An earlier manifest recorded 79 exact
  deterministic reference matches; this machine has no pinned native oracle,
  and those matches have not been rechecked after preview corrections.
  The single-project counts do not establish faithful behavior for warned
  connected chains; the typed pipeline path is measured separately.
- Named optional host inputs retain connected defaults and overrides through project
  serialization and native `.mfd` export/reimport. Defaults run only when the
  host omits the name; an explicit null remains supplied. The interpreter and
  generated Rust/C# typed and JSON hosts agree on defaults, overrides, lazy
  evaluation, and typed failures, including nested user functions. GUI file
  runs and previews use editable session-only project values; pipelines keep
  their own values. The palette creates required or optional host inputs with
  editable names/types and a connectable default. Successful project changes
  reset project values, while failed changes preserve them. The
  local phone-list design preserves identical XML for its four-person default
  and three-person `F` override. Exact same-type optional query inputs retain
  host overrides, including native title controls and joined integer-threshold
  SELECT parameters. Required host-only inputs retain exact same-type query
  values through native SELECT and title WHERE controls, including lazy empty
  sources and typed missing/type failures. Required and connected-default inputs
  retain raw lexical preview metadata through project serialization and native
  export. Explicit design-preview execution uses it before a connected default
  when no host value is supplied; normal and generated runs ignore it. The GUI
  supports editing the saved value, previewing it, and inspecting it in the
  debugger. Optional inputs without connected defaults are diagnosed and skipped
  instead of silently becoming required inputs; their omitted-value behavior
  remains unverified and the executable import profile rejects them. Cross-type
  dynamic SQL coercion remains unsupported. Missing imported filter predicates skip the dependent
  iteration with a diagnostic instead of producing unfiltered rows.
- Generated Rust and C# hosts have compiled and executed forty supported warning-free
  local-corpus mappings: JSON-to-JSON, XML-to-JSON, FlexText-to-XML, grouped
  CSV-to-XML, grouped XML-to-XML with annual reductions, and XML-to-XML with
  three-key sorting, top-ten temperature selection, and filtered compact
  positions, plus an ordered string-join over nested contacts and lazy
  temperature classification, XML-to-CSV lookup-fed token existence, and a
  composite join across primary and named XML inputs, and Protobuf-to-CSV
  mapping through a value map and numeric user-defined function, and transposed
  XLSX-to-CSV mapping through position-indexed item-at aggregates; a recursive
  XML hierarchy collecting 90 nested file paths; four XML expense items mapped
  through integer and boolean value maps; a summary join aggregate with two
  keyed lookups per item; sixteen key/value properties from runtime-named
  generic XML elements; two dynamically named XML outputs from a local
  two-document file set; two local XML files merged into one XML document; and
  one expense report mapped to ordered primary and named XML targets.
  Cases one through fifteen, seventeen, and eighteen
  compare schema-shaped JSON results across both backends. Case sixteen uses
  typed execution APIs and compares XML bytes because its mapped output has
  multiple occurrences of a nominally singular XSD group. Case nineteen uses
  typed APIs for the file set and compares ordered paths, XML, and typed JSON
  for each output document. Case twenty uses the same typed file-set input
  path and compares one merged XML and typed JSON output. Case twenty-one
  compares both targets through typed output-set APIs and their original XML
  schemas. Its JSON transport projects away unused recursive mixed-description
  branches because the full JSON boundary cannot represent them; C# receives
  the root namespace explicitly. Case twenty-two maps an embedded-layout PDF
  source to JSON article and store records. Case twenty-three compares exact
  XML nil-aware output in both generated typed hosts and fixes owned repeated
  whole-group copies in Rust; the JSON host boundary cannot carry XML nil.
  Case twenty-four maps an embedded-schema EDIFACT order to a headerless CSV
  buyer row with coded date-time conversion, comparing typed output and exact
  CSV bytes in both generated backends. Case twenty-five recursively filters a
  local XML directory tree to 33 matching files, comparing generated Rust and
  C# typed and JSON entry points against the interpreter under the same
  schema-shaped JSON input. The native reader's retained XML child-order stream
  is checked separately: recursive filtering now removes rejected occurrences
  and updates nested child values before XML serialization in the interpreter
  and generated runtimes. Case twenty-six reads a two-file XML manifest and
  loads each referenced XML document through a bounded per-driver host loader;
  generated Rust and C# typed and JSON entry points agree with the interpreter
  on loader order, merged office records, and serialized XML. Case twenty-seven
  checks a pre-target expense-limit failure in both generated hosts' typed and
  JSON APIs, then lowers the outlying expense in memory and compares the
  successful typed JSON and XML output against the interpreter. Case twenty-eight
  applies an embedded FlexText layout to four XML name strings and compares
  generated typed JSON and exact headerless CSV with the interpreter. Case
  twenty-nine drives an XML target with one generated sequence item and computes
  three expression-valued temperature aggregates inside it; generated Rust and
  C# typed JSON and exact XML match the interpreter. Case thirty selects a
  computed JSON property across twelve item rows, filters absent and false
  values, then sorts five output rows into exact headered CSV; generated Rust
  and C# JSON values and CSV bytes match the interpreter. Case thirty-one maps
  three JSON purchase orders with twelve items and EU/US address alternatives
  into namespace-qualified XML. Generated Rust and C# typed JSON and exact XML
  bytes match the interpreter; C# XML now preserves per-node namespace
  transitions and qualified attributes. Case thirty-two maps 21 local XML
  people into a proto2 target; generated Rust and C# typed and JSON APIs agree,
  and their schema-shaped outputs re-encode to the interpreter's exact
  Protobuf bytes with matching decoded values.
  Case thirty-three reads an embedded-layout IDoc order and maps it to XML.
  Generated Rust/C# typed XML and schema-shaped JSON string/byte outputs agree
  with the interpreter on the order header and two items. Its lexical `1.000`
  and `2.000` item amounts remain strings in typed mapping results; XML and
  JSON output boundaries convert them to exact integers without floating-point
  rounding. Case thirty-four reads a configured X12 order and maps one customer
  to headerless CSV. Both generated hosts agree with the interpreter's row
  values and exact `Michelle Butler,Mrs,20200430` output line after the native
  X12 source survives schema-shaped JSON transport. Case thirty-five reads a
  local XBRL income table into four ordered operating-expense rows for a new
  XLSX sheet. Generated Rust and C# typed and JSON hosts agree with the
  interpreter, and the shared workbook writer produces matching decoded
  headers and cells, including the first period's 3,454,000,000 total. Case
  thirty-six reads nested SQLite users, groups, and applications into four
  headerless CSV rows. Generated Rust and C# typed and JSON hosts agree with
  the interpreter on row order, missing-description substitution, and exact
  CSV bytes. Case thirty-seven reads four SQLite income periods into an XBRL
  statement. Both generated hosts agree with the interpreter's typed and JSON
  results and reproduce the same instance bytes, with four contexts, two
  units, and 100 ordered facts. Case thirty-eight maps XML organizations to a
  hierarchical XLSX workbook with two runtime-named sheets. Generated Rust
  and C# typed and JSON results match the interpreter; decoding the workbooks
  confirms sheet order, dated office rows, 15 and 6 employees, and department
  bands. Case thirty-nine reads a local FlexText regex switch and publishes
  three fixed-width output sets. Generated Rust and C# typed and JSON results
  agree with the interpreter on 6, 5, and 6 ordered rows and exact padded
  LF-delimited bytes for every output. Case forty reads 15 staff rows from an
  XLSX sheet into a SQLite People table. Generated Rust and C# typed and JSON
  results agree with the interpreter, including exact nested JSON text that
  retains `25.0` as a floating value; decoded SQLite rows receive IDs 1–15.
  The book-catalog case reads a two-page PDF with 52 books. The native
  PDF reader supplies schema-shaped JSON to both generated hosts; typed XML
  and JSON string/byte results match the interpreter. The generated hosts do
  not parse the PDF themselves.
  This small execution sample does not establish that all emitted survey
  designs execute equivalently.
- Generated regex operations retain complete Unicode scalars, source-order
  captures, nested character classes, intersection/difference/symmetric
  difference, and all 14 ASCII POSIX class terms. Unicode word/nonword assertions
  share scalar word membership and retain ordered alternatives, greedy/lazy
  captures, and global spans within explicit execution limits. General-category
  aliases and queries preserve scalar membership, inversion, and loose names;
  supplementary private-use aliases align and unavailable surrogate properties
  reject. Consecutive repetitions retain nested greedy/lazy semantics and exact
  capture values through the bounded scalar matcher, including nullable inner
  repeats and discarded zero-count bodies. A compiled 251-case mapping
  compares matching, replacement, and tokenization through Rust/C# JSON string
  and UTF-8 byte hosts against the interpreter. Additional boundary spellings,
  broader property vocabularies, host-only syntax, and compilation budgets
  remain separate backend differences.
- File and export fidelity: project, pipeline, generated-schema, and PDF-layout
  codecs preserve finite floating-point tags and bits. Generated JSON Schema
  and certified IDoc configuration text are reimported before publication;
  metadata that cannot survive the emitted representation rejects explicitly.
  XML text, labels, and XSD fixed/default values preserve carriage returns,
  tabs, and line breaks. Native-shaped direct PDF captures, one named page
  group, and a guarded vertical-boundary/painted-edge row layout are checked
  against the bounded local parser. The annual temperature PDF retains identical
  extraction and 148 CSV rows through strict export/reimport. A two-page
  independent merge also retains extraction and XML for 52 books through two
  strict cycles. Guarded first-page anchored invoice tables and first-page
  headers with all-page anchored rows preserve physical extraction and output
  through two strict cycles. Unsupported splitter search/skip controls and
  narrowed unnamed group regions reject explicitly. ObjectFind layouts retain
  a persisted repair marker and block physical execution and faithful export;
  the earlier stock-PDF generated execution case is withdrawn. Other complex
  templates retain a lossless Ferrule payload and report `pdf_layout` instead
  of claiming native compatibility. CSV preserves optional present-empty text
  through file, payload, browser, and strict native round trips, with typed-empty
  uncertainty diagnosed explicitly. The observed UTF-8 BOM setting survives
  project save/reopen and strict native round trips, and file, payload, and
  browser writers emit matching bytes for independent primary and named targets.
  JSON Schema import shares reference, expanded-depth, and cumulative work
  limits across ordinary trees and private predicates.
- Known architectural constraints: each mapping stage has one primary driver,
  scalar graph outputs, no general `.mfd` stage-graph import, incomplete
  connector history and no rich expression/context breakpoint predicates, and
  reusable functions limited to the currently typed scalar, record, sequence,
  recursive, hierarchy, and adjacency profiles.

## Capability Matrix

| Area | Ferrule now | Full-compatibility target |
| --- | --- | --- |
| XML | XSD subset, local include/import graphs, named model/attribute groups, typed element/simple-content/attribute defaults, expanded-name identity for elements and attributes including compatible same-local strict-wildcard alternatives, simple and ordered mixed content, `xsi:nil`, namespace-constrained skip element wildcards, lax element and attribute wildcards with typed known declarations plus nonduplicating generic fallback, closed strict wildcards resolved to exact singular or repeating typed choices, strict known-attribute projection, direct or named-group attribute wildcards, bounded cross-namespace substitution groups, and compatible transitive element-only or mixed `complexContent` plus scalar-text/attribute-only `simpleContent` extension/restriction alternatives | Remaining derived-type input shapes, XSD 1.1 wildcard exclusions, unordered wildcard compositors, and unresolved strict wildcard declaration sets |
| JSON | JSON Schema subset, confined external and local refs, compatible structural `allOf` intersections across objects, scalar domains, and matching arrays, bounded exact scalar `const`/`enum` value sets including finite `anyOf`/`oneOf` composition, exact numeric ranges including contiguous same-type `anyOf` unions, exact decimal `multipleOf` constraints, exact array-count, `contains` match-count, object-property-count, and Unicode string-length intervals, exact structural `uniqueItems`, bounded exact homogeneous Draft 2020-12/undeclared `prefixItems` and Draft 4/6/7/2019-09/undeclared tuple-form `items` normalization, bounded portable string `pattern` assertions, exact closed homogeneous `patternProperties`, exact object-property presence, property dependencies and whole-object dependent-schema predicates, exact single-property-presence conditional normalization for Draft 7/2019-09/2020-12/undeclared schemas including guarded nullable objects and representable `else: false`, property-name constraints including exact finite and portable-pattern `not` complements, and open/closed object semantics, exact nullable scalar/object/array wrappers including flat multi-branch nullable compositions, heterogeneous scalar type arrays, exact scalar `anyOf`, pairwise-disjoint scalar `oneOf`, identical or scalar-domain-subsumed array `anyOf` branches, compatible object `oneOf`/`anyOf` with required or optional string, boolean, signed-integer, finite-number, or JSON-null discriminators, same-mode and provably disjoint cross-mode nested object unions with compatible wrapper constraints, typed and unconstrained dynamic properties, and bounded ordered `format` annotation preservation without vocabulary assertion | Incompatible or correlated validation composition, correlated property-name unions and complements involving length, format, or mixed assertions, distinct per-selector `patternProperties` schemas, pattern-property objects under active `allOf`, alternatives, or structural `$ref` siblings, open or typed pattern-property fallbacks, general overlap intersection, pattern-property value shapes outside the ordinary exact JSON profile, `unevaluatedProperties`, value-sensitive, multi-trigger, general-`if`, `else: false` over object alternatives or a closed undeclared trigger, or other nontrivial-`else` conditional schemas, general heterogeneous positional array schemas, heterogeneous or correlated numeric-range scalar unions, heterogeneous array composition, overlapping cross-mode or incompatible typed-wrapper union composition, structured discriminator values, mixed arrays, and remaining validation-keyword enforcement |
| Flat files | Delimited CSV with configurable single-byte or disabled quoting, fixed length, reusable FlexText layouts, and bounded string-fed parsing | Additional FlexText commands and parser variants |
| Database | Relational SQLite reads and full-replace writes, imported WHERE/ORDER controls, static/correlated queries, and deterministic generated keys | General query model, insert/update/delete, PostgreSQL |
| EDI | Bounded X12/EDIFACT/HL7/TRADACOMS runtime plus embedded IDoc/SWIFT layouts and executable `.mfd` configurations | Complete applicable validation/autocompletion behavior, configuration commands, dialects, and versioned release packs |
| Other formats | XLSX including hierarchical and update-existing targets, native XBRL instances, proto2/proto3 input/output, static HTTP XML sources, and visual PDF sources with page selection, vertical collages, marker groups, and table layouts | XBRL taxonomy/package/view semantics, complete applicable Protobuf/XLSX profiles, and remaining PDF extraction, template-editor, and OCR workflows; PDF remains source-only like the reference product |
| Dataflow | One primary driver per stage plus named static/dynamic and wildcard document sources, bounded typed host runtime parameters, multiple mapped targets, dynamic per-document output paths, a validated ordered stage DAG with a file host and optional per-stage mapping paths, bounded serial XML pass-through chain import and guarded export for up to 64 pass-through targets with XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, bounded XBRL, or new-workbook XLSX final primary output, connected final-stage XML target fan-out or one named CSV or JSON target beside an XML primary, and original XML hosts feeding named inputs across stages through distinct ports or shared-port fan-out, plus one guarded earlier-intermediate branch that feeds the final stage as a named input, and GUI editing/inspection/running of saved pipelines with stored input-path hints | Fully general named N-to-M endpoints, general `.mfd` stage-graph import/export including other connected later-stage named sources, service hosts, and embedded per-stage graph editing |
| Functions | Scalar subset plus aggregates, generated-sequence reducers, ordered scope sequence windows, and typed reusable graph UDFs | General first-class sequence composition and higher-order reusable mappings |
| Execution | Native interpreter, unified bounded host run options, bounded raw-payload library execution, ordered file and payload artifact reports, deterministic versioned CLI JSONL traces, CLI, GUI, browser demo | Packaged runtime, documented HTTP API |
| Authoring | Existing-project graph/scope editor plus XSD/JSON/CSV/SQLite/Protocol Buffers blank-project setup, explicit native/Ferrule MFD export profiles, SQLite table introspection for named lookup sources, scope management, extra-source CRUD, named-target CRUD and canvases, deterministic compatible-field auto-connect, bounded in-memory preview, undo, and layout | Complete schema/format wizards |
| Debugging | Static validation, runtime errors, deterministic node/scope/control/target-field traces, a bounded searchable GUI run report, post-run graph-node input/output history for direct calls, conditionals, value maps, lookups, dynamic keys, collection searches, XML mixed-content replacements, generated-sequence generator arguments, existence predicates, item-at indexes, and aggregate and generated-sequence reduction expressions/arguments, bounded source-row previews with nested row/join context, event-by-event replay of completed traces with direct links from retained trace/history/source-row entries, stage-attributed pipeline traces with stage-specific Node History/Source Rows/Replay, opt-in target-write, post-evaluation graph-node, and delivered graph-input debug hooks, and worker-backed live GUI Preview/file-Run/pipeline stepping with static target-field breakpoints, bounded typed scalar-value, innermost active-position, exact active-frame source-field, target-write value-node, exact expression-node/value, recorded consumer-pin conditions, first failing graph or reusable-function node pauses, and shallow active source-frame snapshots | Remaining connector classes, full source-row inspection, richer expression/context breakpoint predicates |
| `.mfd` | 187 imports (169 warning-free in the isolated resource profile), 174 dependency-complete engine-valid projects, 186 best-effort exports/reimports (185 warning-free), 174 supported engine-valid round trips, persisted EDI-catalog and PDF repair dependencies, explicitly trusted/confined catalog resolution, 167/167 safe preview executions with 163 outputs, and 166 semantic round-trip matches with zero drift and one unsupported-contract skip; previous native references remain unverified here | Reference-application open/validate/execute/re-save verification, complete behavioral-reference coverage, and broader explicit extension-dependent export reporting |
| Code generation | [Portable Rust and package-free C# libraries](docs/code-generation.md) with shared lowering, bounded schema-shaped JSON host APIs including heterogeneous scalar-union boundaries and targets, catalog-backed scalar functions including schema-guided JSON-string field projection and typed object serialization, embedded delimited and fixed-width FlexText field projection, typed failures and ordered failure rules, host runtime values and bounded typed parameters, ordered value maps, static and per-driver dynamic named inputs, dynamic source fields, cross-source lookups, expression-driven collection search, structured XML serialization and ordered mixed-content replacement, root-context static inner joins, bounded per-item correlated join scopes and joined-tuple reductions, multiple mapped outputs, dynamic document sets and JSON object construction, scalar/group targets, exact whole-group copies, recursive-filter, path-hierarchy, and adjacency-tree construction, source/generated iteration and ordered scope concatenation, keyed/marker/block grouping, post-group member filters, controls, aggregates, recursive-collect generated sequences, and generated-sequence reducers; all 174 dependency-complete survey designs emit in both languages | Compile-and-execute parity for applicable mappings, published/versioned endpoint hosts, and Java, C++, XSLT 1/2/3, and XQuery generators according to the reference product's format/feature matrix |

## Workstreams

### 0. Versioned Conformance and Compatibility Profiles

Progress: a versioned ledger records independent status cells and supports
strict profile gates; its inventory is explicitly incomplete. Export preflight
classifies known Ferrule extension dependencies and blocks a selected native
profile before publication. Strict executable import now rejects warnings,
unresolved runtime dependencies, and invalid mappings while the default import
remains repair-oriented. Selected private survey counts are enforceable in a
qualification environment; native reference-application execution evidence remains absent.

- Maintain a checked-in, machine-readable Enterprise 2026r2 `.mfd`
  capability ledger.
  Track import, interpreter execution, Ferrule self-roundtrip, native `.mfd`
  export, GUI authoring, debugging, and every generated backend independently.
- Keep best-effort import for repair workflows, and add a strict compatibility
  check that rejects connected behavior which would otherwise be lost.
- Separate native-compatible `.mfd` exports from exports that require Ferrule
  extensions. Native claims require reference-application validation and
  execution evidence.
- Turn selected private-corpus survey baselines into assertion-based qualification
  gates. Missing expected resources, reduced coverage, new warnings, or semantic
  drift must fail the qualification run instead of producing a false green.
- Record the exact reference-application build, edition, platform, backend,
  driver, schema dialect, and catalog versions for each result.

Exit criteria:

- Every official baseline capability has a stable identifier and an explicit
  applicability/status entry. Unknown or untested is never counted as supported.
- Imported-design execution, native-export execution, and a native
  re-save back into Ferrule agree for every native-export claim.
- Ferrule-native extensions remain executable and round-trip losslessly even when
  they have no native reference-application representation.

### A. Mapping Semantics and Interoperability

#### A1. Executable `.mfd` Common Profile

Build breadth only where imported mappings can execute equivalently.

Progress: legacy indexed XML names, stable `distinct-values` pipelines, first-class
`tokenize`/`tokenize-by-length` sequences, and inclusive `generate-sequence` ranges
are implemented. Generated and source-backed scopes execute and export ordered
skip/first/from/range/last windows after sort/filter/group controls with stage-correct
positions, while generated sequences can feed scalar `item-at`.
Compatible JSON object alternatives preserve required or optional typed scalar
and JSON-null `const` or `enum` discriminators. Bounded scalar multi-value
domains remain exact through direct schemas, references, `allOf`, and finite
`anyOf`/`oneOf` composition. Heterogeneous scalar
type arrays and exact scalar `anyOf` unions preserve each runtime value tag,
while nullable members distinguish explicit JSON null from a missing property.
Array `uniqueItems` assertions compare complete JSON values, ignore object
member order, preserve nested array order, and use exact supported numeric
identity on native and generated boundaries.
JSON objects now preserve the standard default-open contract: omitted or
`true` `additionalProperties` retains arbitrary dynamic values, schema-valued
forms retain their exact value domain, and explicit `false` rejects undeclared
properties at native and generated input boundaries. Compatible `allOf`
intersections cannot widen a closed branch, and canonical schema export retains
the resulting open, typed-open, or closed contract.
An exact closed homogeneous `patternProperties` profile is also executable.
The containing schema must explicitly type `object` or `object | null`, set
`additionalProperties: false`, and give every portable selector in a nonempty
bounded map one identical exactly representable value schema. Scalar,
structured object, and homogeneous array values use the ordinary supported
JSON value profile. Selectors are retained in declaration order and combined
as an OR. A declared fixed property matched by any selector must have the same
schema, while nonmatching fixed properties remain independent. Runtime
decoding selects fixed properties first and applies the selector set only to
remaining names; an independent `propertyNames` constraint still applies to
every name. Dependency rules whose triggers are neither fixed nor selected are
semantically unreachable and normalize away. Nullable object null bypasses the
object rules. Native, generated Rust, and generated C# input, normalized
output, and JSON Lines boundaries use the same portable matcher and shared
per-document pattern work budget. Canonical export writes the retained
`patternProperties` map with `additionalProperties: false`, and re-import
preserves it. An empty map is a no-op.
Distinct selector value schemas, open or typed fallbacks, general overlap
intersection, value shapes outside the ordinary exact JSON profile, and
pattern-property objects under active `allOf`, object alternatives, or
structural `$ref` siblings remain explicit rejections.
Concrete JSON objects also retain exact `minProperties`/`maxProperties`
intervals through references, nullable wrappers, compatible `allOf`, and
alternatives with one common effective interval. Native and generated Rust/C#
boundaries count distinct parsed input properties before object decoding and
normalized output members after absent values are omitted. Inconsistent
required/closed-object counts and correlated alternative intervals reject
rather than widen.
Object property dependencies and whole-object dependent predicates are
executable on native and generated Rust/C# boundaries. Modern
`dependentRequired` and legacy property-array `dependencies` normalize to one
bounded trigger-to-required-property relation. A required-only
`dependentSchemas` or schema-valued legacy `dependencies` entry lowers to that
same relation; other exactly representable entries retain their complete
whole-object predicate. When a trigger is present, explicit JSON null included,
the predicate sees the complete containing object. Input validates parsed
members, while output validates the normalized object after absent Ferrule
values are omitted. Compatible `allOf` branches append conjunctive predicates;
canonical export preserves their retained declaration order, including
interleaved repeated triggers. Nullable object null bypasses them; object
alternatives retain only one identical effective constraint set. Retained
predicates may use the executable JSON subset recursively, including
independently bounded nested dependent schemas. Draft 7, 2019-09, 2020-12, and
undeclared explicitly typed object schemas also normalize an `if` that tests
exactly one required property's presence and a supported `then` predicate into
this same dependent-schema model. A nullable outer object is supported with an absent or
`true` `else` only when the `if` explicitly proves `type: object`. Exact
`else: false` requires a trigger that can be represented as an ordinary
required field; it removes the nullable bypass when the false branch rejects
null and retains any supported `then` dependency. It does not combine with
existing object alternatives, and a closed object must already declare the
trigger. Canonical export writes `required` and, when `then` retains a
dependency, `dependentRequired` or `dependentSchemas` as appropriate.
Value-sensitive, multi-trigger, general-`if`, and other nontrivial `else`
schemas, unevaluated keywords, and heterogeneous positional array schemas
remain explicit rejections rather than approximations.

The bounded exact homogeneous `prefixItems` subset normalizes Draft 2020-12
and schemas without `$schema` into the existing repeated-item form. Every
entry in the bounded finite prefix must normalize to one identical
non-repeating item shape. The tail must use that shape, be closed so the prefix
length becomes `maxItems`, or be provably unreachable under an existing
maximum; an unconstrained tail is exact only when the common item shape accepts
arbitrary JSON. Canonical export writes equivalent ordinary `items` and `maxItems`
constraints. This lowers entirely to the current schema IR and therefore
requires no engine, Rust runtime, C# runtime, or backend-emitter extension.
Draft 4 through Draft 2019-09 treat `prefixItems` as unknown, while
heterogeneous prefixes, contradictory bounds, concrete nested-array entries,
and active `unevaluatedItems` continue to reject.

The complementary homogeneous legacy tuple profile accepts array-valued
`items` in Draft 4, 6, 7, and 2019-09 resources and in schemas without
`$schema`. Each non-empty bounded tuple contains at most 4,096 positional
members, all of which must normalize to the same non-repeating item shape. Its
`additionalItems` tail must use that same shape, be `false` so the tuple length
becomes `maxItems`, or be provably unreachable
under an existing maximum; an absent or `true` tail is exact only for an
arbitrary-JSON item shape or when the maximum makes the tail unreachable.
Explicit and derived item-count intervals are intersected, and contradictions
reject. Canonical export lowers the result to ordinary schema-valued `items`
plus `maxItems` when closed, so the existing mapping IR and native/generated
Rust and C# boundaries execute it unchanged. Draft 2020-12 instead requires
schema-valued `items`; array-valued `items` rejects there rather than being
mistaken for `prefixItems`. Heterogeneous positional members or reachable
heterogeneous tails remain unsupported.

Object `propertyNames` assertions validate every actual key, including declared,
runtime-named, and empty-string properties. Exact `false`, finite
`const`/`enum` name sets and finite `not` complements, Unicode-scalar length
intervals, bounded portable pattern conjunctions/disjunctions and their exact
complements, and retained nonasserting `format` annotations execute on raw
parsed input keys and normalized emitted keys in native and generated Rust/C#
boundaries. Finite and pattern complements compose through intersections,
compatible unions, references, and double negation. `true` and an
unconstrained schema normalize away. Correlated predicates and complements
involving length, format, or mixed assertions remain outside the subset rather
than being approximated.
Array `contains` assertions retain a bounded conjunction of schema-shaped item
predicates with exact match-count intervals. A plain assertion requires at
least one match; Draft 2019-09 and newer `minContains`/`maxContains` modifiers
set the interval explicitly. Native and generated Rust/C# boundaries count raw
input members and normalized emitted members, nullable array null bypasses the
assertions, and predicate patterns share the document matcher budget. Compatible
`allOf` branches retain every conjunct; array alternatives survive only when
their effective assertions are identical or one branch exactly contains the
other. Draft 4 resources ignore `contains`, while Draft 6/7 apply its default
minimum but ignore the newer modifiers.
Array `anyOf` branches also collapse exactly when one scalar item domain
contains every narrower branch. Nullable `oneOf` and `anyOf` compositions can
combine null with multiple compatible object, scalar-union, or subsumed-array
branches. Those alternatives can drive exact derived XML
type output. Transitive concrete XSD descendants are discovered through abstract
include/import intermediates, and bounded substitution groups retain their exact
concrete element names. Database WHERE/ORDER controls lower into runtime scopes,
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
Scalar and nested scalar UDFs can compose bounded tokenization or inclusive
integer generation with 1-based `item-at`, a filtered existential test, or
count/sum/average/minimum/maximum/string-join over raw, filtered, or per-item
computed values; those definitions inline to the same native reducers used by
ordinary mappings and generated Rust/C# libraries. Generated-sequence predicates
and value expressions can read both the generated item and its 1-based position.
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
The versioned compatibility survey records import, validation, export, re-import,
and post-export validation separately. All 187 designs import; the isolated
resource profile has 169 warning-free imports and 174 dependency-complete,
engine-valid projects. Four connected chains warn in single-project mode and
validate as typed pipelines. Twelve designs preserve unresolved EDI-catalog
requirements, while one PDF ObjectFind layout is a persisted repair draft.
Faithful export blocks that PDF boundary. The remaining 186 designs export and
reimport, 185 without warnings; one retains unavailable JSON Schema provenance.
All 174 supported round trips remain engine-valid. Safe preview execution passes
167/167 attempts and writes 163 redirected outputs, with one expected database
write failure. Round-trip execution has 166 semantic matches, zero drifts, and
one explicit unsupported SQL preview-contract skip. Native references remain a
separate measure: the earlier 79 matches are unavailable here and have not been
rechecked after preview corrections. Structural or local execution success
cannot certify reference-application acceptance.

- Restore warning-free, engine-valid import coverage by faithfully representing
  connected stages while expanding the supported component surface.
- Preserve isolated, safely redirected execution and semantic reference comparison
  without writing into the read-only vendor sample tree.
- Expand behavioral reference coverage across format-specific and mixed-content edge
  cases, especially workflows whose vendor outputs require unavailable services.
- Add general sequence composition and reusable graph-backed UDFs instead of further
  one-off lowering paths.
- Complete remaining derived-type input shapes and the remaining general scalar/
  array, nested, overlapping, or untyped-discriminator JSON union semantics.
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

Progress: the typed [pipeline model](docs/mapping-pipelines.md) connects
complete projects through primary or named target outputs into primary or
static named inputs. It validates IDs, references, schemas, and cycles before
execution, then runs stages in stable dependency order. The file host publishes
selected outputs atomically only after the complete graph succeeds. Bounded
serial XML pass-through chains import and export as connected `.mfd` designs,
with XML, CSV, fixed-width text, FlexText, JSON, Protocol Buffers, bounded XBRL, or
new-workbook XLSX final primary targets. Native export preflight rejects
unsupported stage shapes before publishing artifacts. Intermediate XML
boundaries retain their output instance and
source preview instance on the merged pass-through component without inventing
an output path from a preview path. All four local chains export and reimport
in the strict native profile. The date/time chain
uses bounded native XML target casting with exact `xs:dateTime` XSD restoration;
shared conversions and unsupported target subtree shapes still reject before
publication. A local XML-to-hierarchical-XLSX mapping also runs after an
identity XML stage; strict export and reimport preserve decoded workbook cells.
An XML-to-fixed-width mapping retains byte-exact text and parsed rows through
the same chain round trip.
Local XML-to-FlexText and XML-to-Protocol-Buffers mappings also run after an
identity XML stage; their final artifacts remain byte-exact across strict
export and reimport. The Protobuf chain publishes the same binary output
through the CLI and writes an adjacent schema during export.
An XML-to-XBRL chain publishes the same instance bytes through the CLI and
strict export/reimport. Its guarded profile excludes presentation and numeric
fact metadata that the current XBRL boundary cannot represent.
An XML primary final stage can also publish one connected named CSV or JSON
target. Direct execution, strict export/reimport, and CLI publication retain
exact XML and CSV or JSON bytes; nonfinal, ambiguous, and unrepresented named
targets reject before artifacts.
Intermediate or named XLSX targets and update-existing workbooks reject before
publication.
Synthetic two- and four-stage chains
preserve original XML host sources feeding named inputs in several stages
across export and reimport, including shared-port fan-out from one graph
vertex. A guarded XML chain of at least three stages also retains one
nonadjacent earlier intermediate as a named input of the final stage while
its immediate predecessor supplies the primary input. Three- and four-stage
synthetic runs cover branches from the first or second intermediate. Strict
native export/reimport preserves each pass-through component and a shared output
vertex; direct and CLI execution preserve the final XML bytes where tested.
General `.mfd` stage-graph import/export, service hosts, general driver cardinality, and embedded
per-stage graph editing remain. The file host can supply a distinct active
mapping path for each stage
while retaining the pipeline path as the top-level mapping path. The native
GUI can edit, validate, atomically save, inspect, run, and debug a saved pipeline
independently of the open project; each stage's Project internals are still
edited in the ordinary Project editor. In-memory pipeline Preview and Debug
Preview return every stage's primary and named outputs through a bounded byte
host, with separate logical format paths and no output publication. Typed
stage edges retain their values without temporary files.

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

Progress: one invariant-preserving catalog now owns all builtin identities,
fixed/ranged/variadic arities, parameter and return domains, categories,
authoring exposure, documentation, purity, determinism, and dispatch. Engine
validation rejects invalid call arities before execution, and the GUI derives
its searchable categorized function palette, initial pins, pin labels, and
argument controls from the same catalog.

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

- Source/target wizards for XSD, JSON Schema, CSV, Protocol Buffers, and database tables.
- Extra-source CRUD and format options.
- Scope add/remove plus target-driven scope skeleton generation.
- Searchable categorized function palette with correct initial pins.
- Auto-connect matching children and explicit subtree expansion.

Progress: XSD/JSON/CSV/SQLite/Protocol Buffers blank-project setup, introspected SQLite named
lookup sources, extra-source and named-target CRUD, independent primary/named-
target canvases, scope editing, the catalog-backed function palette, and
conservative compatible-field auto-connect and bounded target-driven static
scope-subtree expansion are implemented. Broader database boundary wizards
remain.

Protocol Buffers setup selects independent source/target root messages and
optional binary file paths. Bounded local schema imports are embedded in the
saved project; a synthetic nested Unicode mapping saves, reopens, and produces
identical binary output after its original schemas are removed. Invalid root
changes clear the prior projection, and failed imports preserve the open
mapping. The GUI also exposes guarded native MFD export with component-specific
findings and `.mfd` filename suggestions; native application acceptance remains
unverified.

Named boundary editors retain embedded formats independently of filename
suffixes and reject incompatible Protocol Buffers schema replacement. Passive
rendering preserves CSV defaults, while deliberate format changes clear stale
settings. Synthetic save/reopen checks retain exact named binary bytes and
explicit XML/JSON/JSON Lines outputs through file and payload hosts.

Named Protocol Buffers input/output setup now requires independent supported
roots and retains its embedded schema graph through undo/redo, save/reopen,
and exact file/payload output checks after original schemas are removed.
Abandoning a pending binary schema after changing a previously introspected
SQLite path or table requires loading a new table schema.

Named delimited-text editors expose independent delimiters, custom or disabled
quotes, headers, input empty-text handling, and output BOM settings. Pending
invalid edits preserve the saved boundary. File and payload checks cover
Unicode, embedded delimiters, distinct input/output dialects, and saved format
identity for extensionless or unfamiliar filenames.

Function-body node menus select the saved function output directly, including
parameter identity outputs. Output protection moves with the selection;
undo/redo, save/reopen, lazy conditional evaluation, and file/payload execution
retain the selected expression. Shared graph removal also checks inactive
target bindings and project-owned controls before changing a node.
Function creation and call insertion respect the same editing lock as the
canvas during runs and pending project actions; viewing and cancellation
remain available.

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

Progress: node, scope-candidate, filter, sort, grouping, window, and successful
target-field events are exposed through the engine and versioned CLI JSONL.
The GUI shows a bounded, searchable run report and can execute the current
unsaved project against bounded editable input without writing output files.
Its worker-backed Debug preview pauses before each ordinary target-field write,
shows the pending value, bounded current-scope draft, and up to four shallow
active source frames, and supports Step,
Continue, Pause at next write, Cancel, and a selector for exact static
target-field breakpoints. Saved file-backed Run uses the same live controls,
with cancellation before output publication and app-close deferral during
publication. Saved pipelines support in-memory Preview and Debug Preview of
all stage outputs, stage-qualified traces, and the same worker-backed stepping
controls. File pipeline runs support cancellation before the shared publication
boundary. Preview and saved runs can narrow a breakpoint by a
complete typed scalar value within the debugger's bounded preview or by the
innermost active 1-based position after scope controls. A source-field
condition can match one immediate field in active frame 0–3 by complete typed
scalar value, even beyond the shallow eight-field frame preview.
A value-node ID condition can pause before a target write driven by that graph
node, including in a selected pipeline stage; intermediate graph evaluations
do not create a live pause.
An independent expression-node condition pauses after a selected graph node
evaluates successfully, optionally matching its complete typed scalar value.
This includes filters and pre-target rules with no target write. A paused
expression can Step to the next evaluated node or Cancel before publication;
pipeline expression conditions can select one stage.
An independent consumer-pin condition pauses after a recorded input value
reaches an exact graph node and 1-based pin, optionally matching a complete
typed scalar value and one pipeline stage. Step advances to the next recorded
pin delivery. Untaken branches, target bindings, scope-control edges, and
other uninstrumented graph inputs do not produce pin pause events.
Generated-sequence graph nodes deliver their evaluated generator arguments in
visible pin order; `item-at` also delivers its parent-context index after
generation. Null-short-circuited and failed later arguments remain undelivered.
Completed pipeline reports now retain a stage-attributed, globally bounded
trace. Node History, Source Rows, and Replay select a stage to distinguish
reused graph-node IDs without changing the trace-event schema.
Version 4 CLI JSON traces also carry function-qualified body-node outputs and
delivered inputs. Nested calls keep their own function IDs, while GUI History
and Replay distinguish function-local nodes from main-graph nodes in each
pipeline stage. Live Preview, file-backed Run, and pipeline Run can also pause
on one function-qualified body-node output, Step, or Cancel before publication;
caller positions remain visible while function-local source frames stay empty.
Function-qualified input-pin conditions can likewise pause on delivered visible
pins by consumer, one-based pin number, and typed scalar value; Step and Cancel
work across Preview, file-backed Run, and pipeline stages. Failed-evaluation
breakpoints pause once at the deepest failing main-graph or reusable-function
body node. The bounded error preview and source positions are visible in
Preview, file-backed Run, and a selected pipeline stage; Continue propagates
the original typed failure and Cancel stops before publication.
Preview selects the active primary or named target and preflights every required
secondary source. Direct graph input consumption is now recorded for calls,
conditionals, value maps, lookups, dynamic keys, collection-search predicates
and selected values, XML mixed-content replacements, generated-sequence
existence predicates, and aggregate expressions and arguments. The GUI groups
those events with node outputs and shows bounded
source-row previews before filters and sorting. Retained trace rows,
node-history occurrences, and selected source-row details
can jump directly to the same event in Replay, even after report filtering.
Source-row details and Replay also inspect bounded nested records, ordered
collection items, and portable document paths, with explicit omissions and
stage-aware nested-value search. Validation diagnostics can focus their typed
graph, function, schema, scope, endpoint, or failure-rule owner; stale snapshots
require revalidation. Remaining connector classes, full active-context
inspection, and richer expression/context breakpoint predicates remain.

#### B4. Shared Native and Browser Editor

- Extract editor/session logic shared by native GUI and WASM.
- Browser project upload/download, local persistence, full graph edits,
  validation, and preview for XML/JSON/CSV text inputs.
- Keep native and browser behavior under the same authoring tests.

### C. Runtime and Connector Breadth

Prioritize connectors that align existing strengths before product-catalog
breadth.

1. Complete remaining `xsi:type` shapes and JSON compositions; finish JSON5
   native `.mfd` component interop and generated-host coverage beyond the
   schema-shaped CLI/browser document adapters.
2. Add a general query/database mutation IR; qualify every in-scope relational
   driver and the MongoDB, CouchDB, and Cosmos DB profiles.
3. Complete applicable XLSX, FlexText, EDI, Protobuf, XBRL, and PDF extraction
   profiles, including versioned catalogs, template editors, and OCR.
4. Add live HTTP/OpenAPI, SOAP/WSDL, GraphQL/Shopify endpoints and service-host
   workflows with explicit authentication and external-effect contracts.
5. Keep unsupported standards or releases explicit in the conformance ledger
   until their native and generated execution paths are verified.

Runtime support proceeds in parallel:

- Mapping package containing project, schemas, and relative resources.
- Evolving Rust library API, JSON diagnostics/traces, stdin/stdout, parameters,
  and deterministic CLI exit codes. Stabilization follows the mapping and endpoint
  model rather than constraining pre-1.0 refactors.
- HTTP/service adapters with controlled integration fixtures and credential
  references. Workflow/server integrations are tracked as separate product
  profiles over the same execution core.

### D. Generated and Packaged Execution

- Close all applicable interpreter-versus-Rust/C# gaps and execute the corpus
  through generated artifacts instead of treating emission as completion.
- Add generated format/endpoint hosts and publish versioned runtime packages.
- Implement Java, C++, XSLT 1.0/2.0/3.0, and XQuery backends according to the
  reference product's per-format and per-feature support matrices.
- Qualify compilers, runtimes, deployment layouts, parameters, configuration,
  diagnostics, and external effects on every claimed platform.

### E. Product and Ecosystem Workflows

- Complete native authoring for every supported graph/scope/endpoint construct,
  reusable library, decision table, schema, and format template.
- Add cancellable debug sessions, connector value history, context/row
  inspection, conditional breakpoints, stepping, and partial-output inspection.
- Support multi-mapping projects, shared libraries, global/environment resources,
  relocatable packages, generated mapping documentation, and dependency repair.
- Provide versioned automation and IDE integration adapters for in-scope COM,
  Java, OLE/ActiveX, Visual Studio, and Eclipse workflows.
- Track proprietary server execution files, FlowForce deployment, and optional AI
  assistance as explicit profiles. A Ferrule package or scheduler is an extension,
  not evidence of compatibility with a proprietary artifact or service.

## Release Gates

### M0 - Measurable Contract

- The versioned capability ledger covers the full selected edition/build.
- Survey qualification fails on missing expected resources, coverage regression,
  new warnings, semantic drift, or an unreviewed skip.
- Import, interpreter, self-roundtrip, native export, GUI/debugging, and each
  generated backend have independent evidence states.

### M1 - Trustworthy Native Interchange

- Strict compatibility diagnostics prevent silent connected-behavior loss.
- Native-compatible exports open, validate, execute, and re-save in the reference application.
- Ferrule-extension exports remain lossless and are identified before publish.
- Missing local catalog and controlled-service evidence in the current corpus is
  closed or remains an explicit blocked cell rather than a compatibility pass.

### M2 - General Semantic Foundation

- A2 endpoint/stage DAG complete.
- First-class scalar, record, sequence, and document values compose through
  reusable mappings and ordered stages.
- Value, cardinality, context, laziness, state, error timing, and side-effect
  semantics match the reference behavior for the in-scope matrix.
- Existing Ferrule projects migrate without behavioral loss.

### M3 - Complete Authoring and Debugging

- Every existing engine capability can be created, edited, undone/redone,
  saved/reopened, previewed, debugged, and exported without hand-edited JSON.
- Validation and runtime diagnostics navigate to stable graph/scope/schema/
  endpoint owners.
- Large and remote runs are cancellable and never freeze the editor or publish
  partial file artifacts as successful outputs.

### M4 - Enterprise Execution Breadth

- Schema/format, database, service, standards-catalog, and external-function
  cells in the selected Enterprise matrix have native and GUI conformance tests.
- Database/service tests verify requests, effects, ordering, generated keys,
  transactions, rollback, faults, cancellation, and retry policy.
- Performance envelopes and resource limits are published and qualified.

### M5 - Full Compatibility and Product Parity

- Every applicable generated backend compiles and executes the conformance suite
  on its qualified environment and agrees with the interpreter and reference application.
- Project, library, resource, documentation, automation, IDE, deployment, and
  optional product profiles meet their recorded workflow gates.
- Every in-scope ledger cell is verified. Remaining blocked, unsupported, or
  untested cells narrow the published claim instead of being counted as parity.

## Compatibility Boundaries

- Ferrule does not copy vendor source code, generated code, proprietary catalog
  content, or sample content. Vendor samples remain local, read-only black-box
  references; all checked-in fixtures are self-authored.
- Byte-identical `.mfd` serialization and pixel-identical UI are not required;
  executable semantics, editable document structure, and workflows are.
- Ferrule-native features are never disabled to mimic a narrower backend. The
  native `.mfd` profile validates applicability while the general Ferrule model
  continues to evolve.
- Proprietary `.mfx`, licensed catalogs, drivers, services, and optional AI may
  require a user-supplied licensed dependency or a documented vendor bridge.
  Ferrule does not label its own package format as a compatible vendor artifact.
- A capability without direct evidence remains unverified even when a nearby
  feature or a Ferrule self-roundtrip succeeds.

## Scorecard

Update these numbers with each parity increment:

- Workspace tests and strict all-target clippy pass on the pinned nightly.
- `.mfd` import: 187/187; 169 are warning-free in the isolated resource profile.
  Four connected chains warn in single-project mode and validate as pipelines;
  synthetic linear three- and four-stage XML chains also import and execute.
- `.mfd` validation: all 174 dependency-complete projects are engine-valid.
  Twelve designs retain EDI-catalog requirements and one retains a PDF repair
  dependency; all thirteen are explicitly blocked for physical execution.
- `.mfd` export/re-import: 186 designs export/reimport, 185 without warnings.
  One retains unresolved target and embedded string-parser JSON Schema
  provenance. The unsupported PDF repair draft rejects faithful export.
  All 174 supported round trips remain engine-valid. Guarded anchored PDF
  templates now emit native shapes. Direct CLI native preflight accepts 166/187;
  159 also import without warnings, and all 166 strict exports reimport cleanly.
  No local design still reports a PDF layout extension, while the ObjectFind
  draft is explicitly blocked. These checks do not certify native acceptance.
- `.mfd` preview execution: 167/167 attempts complete, producing 163 redirected
  outputs and one expected SQLite target-constraint failure. Thirteen designs
  are dependency-blocked; seven network/captured-service inputs are unavailable.
- `.mfd` preview round trips: all 167 safe projects export, re-import, validate,
  and execute; 166 match semantically with zero drift. One unsupported SQL
  text-to-numeric preview contract is explicitly excluded from the match claim.
- Code generation: 174/174 dependency-complete designs lower and emit for Rust
  and C#. Forty supported opt-in local cases have compiled and executed in both
  backends, with the book-catalog case rechecked at this checkpoint. The earlier
  stock-PDF extraction case is withdrawn and retained as a rejection test.
  Other survey designs still require generated execution checks.
- Behavioral references: an earlier isolated manifest recorded 79 exact
  deterministic outputs. That reference gate is not available on this machine
  and has not been rechecked after correcting preview semantics; these results
  are not inferred from structural success.
- Set `FERRULE_SURVEY_JSON=/path/report.json` for the versioned per-sample
  compatibility report and `FERRULE_SURVEY_DETAILS=1` for text diagnostics.
- All three report-producing read-only surveys accept one explicitly selected
  package manifest and ordered EDI/JSON catalog path lists through the shared
  `FERRULE_MFD_SURVEY_*` environment contract. Schema-version-1 reports record
  non-path resource selection provenance and effective root counts.
- CLI diagnostics: versioned JSON Lines cover validation, import/export
  warnings, runtime failures, and invalid command usage; execution traces use
  a separate bounded version-3 JSON Lines contract.
- CLI run paths: explicit flags override project-relative `source_path` and
  primary `target_path` defaults while stored extra targets retain their own paths.

## Primary References

- [2026r2 release changes](https://www.altova.com/mapforce/whatsnew)
- [Reference product and format scope](https://www.altova.com/mapforce)
- [Edition comparison](https://www.altova.com/mapforce/editions)
- [Database mapping](https://www.altova.com/mapforce/database-mapping)
- [Function library](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_lib.html)
- [Multiple targets and chaining](https://www.altova.com/manual/mapforce/mapforceprofessional/mf_rules_multtargets.html)
- [Debugger](https://www.altova.com/manual/Mapforce/mapforceprofessional/mff_debug.html)
- [User-defined functions](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_udf.html)
