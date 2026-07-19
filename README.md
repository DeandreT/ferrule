# ferrule

An open-source any-to-any data mapping tool. Describe a source schema and a
target schema, wire them together with a mapping graph (functions, conditionals, lookup
tables, filters, cross-source joins), then run the mapping headlessly from the CLI or
interactively from the visual editor.

See [ROADMAP.md](ROADMAP.md) for the capability matrix and phased plan toward
MapForce workflow parity.

## Supported formats

Core formats work as both mapping sources and targets; one-way modes are noted:

- **CSV** — delimited flat files (configurable delimiter), typed columns
- **XLSX** — typed flat worksheet tables, composite/grid source layouts, hierarchical
  targets with repeated runtime-named worksheets and ordered row ranges, and
  update-in-place writes that preserve unrelated workbook content
- **XML** — hierarchical documents, with an XSD importer to bootstrap schemas
- **JSON** — hierarchical documents, with a JSON Schema importer supporting local
  references, compatible closed-object `oneOf`/`anyOf` including required typed scalar
  `const` discriminators, and typed dynamic properties
- **SQLite** — table introspection plus idempotent flat or relational multi-table reads
  and full-replace writes
- **EDI** — bounded ANSI X12, UN/EDIFACT, HL7 v2, and TRADACOMS I/O, plus embedded
  IDoc and SWIFT MT layouts; X12/EDIFACT support includes custom separators,
  composites, repetitions, qualifier-driven loops, and lenient schema-guided parsing
- **Protocol Buffers** — bounded proto2/proto3 schema import and binary decoding/encoding
  for nested messages, enums, required/optional/repeated fields, implicit proto3 defaults,
  and packed scalars
- **FlexText layouts** — bounded recursive split/store/switch pipelines with fixed-width
  and delimited records; embedded layouts work as both sources and targets, and MapForce
  `.mft` configurations compile into portable project data during import
- **PDF extraction** (source) — bounded positioned-text and painted-edge extraction from
  embedded visual layouts, including fixed captures, exact and open-ended page groups,
  independent or vertical-collage region merges, anchors, marker-delimited nested groups,
  and ruled, text-derived, or multiline unruled table rows
- **XBRL** — typed instance input/output with contexts, dimensions, units, and
  namespace-qualified facts; taxonomy formula and linkbase execution are outside the
  current runtime

## How a mapping works

A project file (plain JSON) holds these core pieces:

- a **source schema** and **target schema** — trees of named groups and typed scalar
  leaves, where any node can be `repeating`
- a **graph** of value nodes: read a source field, hold a constant, call a built-in
  function (string/math/comparison/boolean), branch with `if`, translate through a
  `value_map` table, look up one matching secondary row, or project fields from a
  duplicate-preserving inner join
- a **scope tree** that drives iteration: connecting a scope to a repeating source path
  loops over it (a path may cross several repeating levels at once, flattening nested
  repetition), `filter` drops items, and field references fall back outward through
  enclosing scopes so parent-level values broadcast into child rows; scopes can also
  generate scalar sequences with `tokenize`, `tokenize-by-length`, bounded
  `tokenize-regexp`, and inclusive integer ranges (capped at 1,000,000 materialized
  items per scope); source and generated iteration support ordered skip/first/from/range/
  last windows, generated sequences can feed 1-based scalar `item-at`, and scopes can
  iterate a typed multi-source equijoin
- optional **extra sources** — named secondary inputs that any scope or lookup can
  address; a source path may also be computed per driver item to load a typed document
  at runtime
- optional **extra targets** — independently mapped outputs written alongside the
  primary target in one execution

## Migrating from MapForce

ferrule can convert MapForce `.mfd` designs (best-effort): XML components
(resolvable XSDs, bounded DTDs, local includes/imports, attributes, simple-content values,
top-level element refs, named types, and extensions), requestless static HTTP GET calls
with typed XML responses, JSON components (JSON Schema with
local `$ref` support, or the design's entry tree as a fallback), CSV text components
(inline delimiter/header settings), external FlexText `.mft` layouts and supported
string-parser components, flat and
hierarchical XLSX targets, visual PDF sources using supported external `.pxt` layouts,
including page selection, named multi-page table regions, vertical collages, and
marker-delimited nested records, proto2/proto3 binary sources and targets,
SQLite database components, including relational table graphs (schemas are
introspected from the referenced database when it's reachable), the common core functions, the aggregate family
(count/sum/avg/min/max/string-join/item-at), constants, if-else, value-map, and
filter-, group-by-, distinct-values-, tokenizer-, and generated-range-driven iteration
import directly. Iteration windows preserve ordered skip/first/from/range/last controls,
and generated tokenizer/range sequences can also feed scalar `item-at`;
bounded transitive XSD derivation through abstract include/import intermediates preserves
concrete `xsi:type` alternatives. Connected target components become independent project outputs, and
connected document paths can become per-item dynamic sources. Recognized recursive UDF
shapes lower to bounded typed recursive filters, path hierarchies, or adjacency trees.
Core kind-32 inner equijoins import with duplicate-preserving tuple
order, composite keys, projected fields, position, filter/sort/window controls,
and count or computed scalar aggregates over the joined tuples;
`string` and decimal-safe `format-number` conversion are supported;
everything else is skipped
with an actionable warning so you can finish the mapping in the editor. Export
writes the exportable subset back out as `.mfd` plus generated XSD / JSON Schema files,
picking each side's component kind from the project's instance paths. Named static
sources, per-item dynamic XML sources, and typed captured HTTP response boundaries
retain their component ownership. Canonical structured-join export covers root-context
collections inside the primary source and root-context joined aggregate consumers.
Static typed protobuf sources and targets export canonically with generated `.proto`
siblings; dynamic per-item protobuf sources remain unsupported.
Designs built on general namespace identity or `xsi:type` shapes outside the bounded
local derived-type profile,
scalar-key/keyless correlated joins,
multi-source or nested join export, general SQL/database
composition, or other endpoints (unsupported Excel/PDF layout variants, PDF targets,
general XBRL taxonomy/linkbase execution, or unsupported
FlexText/EDI configuration variants)
are not converted yet.

```sh
cargo run -p cli -- import-mfd --mfd design.mfd --out project.json
cargo run -p cli -- export-mfd --project project.json --out design.mfd
```

MapForce is a trademark of its owner; ferrule is an independent project.

## Code generation

ferrule can generate buildable Rust and C# mapping libraries. The current
code-generation subset covers constants, ordinary and frame-pinned source
fields, 1-based collection positions, and constructed target scopes, including
nested/repeating target groups, exact numeric target adaptation and repeating
scalar bindings. Source-backed scopes support empty, nested, and multi-hop paths
with flattened source-order output, innermost-to-outermost field fallback,
active collection ownership, first-item reads through repetitions that are not
iterated, boolean filters, stable mixed-direction multi-key sorting, both
sort/filter orders, ordered skip/first/from/range/last windows, and repeated,
first-item, or mapped-sequence output. Sort keys and filter-before-sort predicates
observe raw positions; filters after sorting observe sorted positions; output
items and their descendants observe final positions compacted per parent. Window
bounds execute once in the parent context and retain typed item-count failures.
Scopes can also generate scalar candidates by literal tokenization,
Unicode-scalar fixed-length tokenization, or bounded inclusive integer ranges.
Generated values expose the same private empty-path item and 1-based position
semantics in both backends, while named fields continue to fall back to the
parent source context. Sequence arguments evaluate once from left to right;
Null yields no items and skips later arguments, and range materialization keeps
the engine's one-million-item limit and structured size failures.
It also supports all ordinary collection aggregates (`count`, `sum`, `avg`,
`min`, `max`, `join`, and `item_at`) over direct fields or computed per-item
expressions, with parent-context scalar arguments and typed numeric failures.
Lazy `if` expressions and scalar calls cover boolean `and`/`or`/`not`, `exists`,
the `is_empty`/`starts_with`/`contains` string predicates, arithmetic
`add`/`subtract`/`multiply`/`divide`, and the `equal`/`not_equal`/`less_than`/
`greater_than`/`less_or_equal`/`greater_or_equal` comparisons. Call arguments
evaluate once in declared order, `if` evaluates only its selected branch, and
runtime failures remain typed. Projects using other graph or scope features
receive actionable capability diagnostics before any output directory is
created.

```sh
# Standalone, package-free .NET 10 library
cargo run -p cli -- generate \
    --project project.json --language csharp --out generated-csharp

# Rust library linked to a local runtime crate during workspace development
cargo run -p cli -- generate \
    --project project.json --language rust --out generated-rust \
    --rust-runtime-path crates/codegen-runtime
```

Rust generation currently requires `--rust-runtime-path`; this will become an
optional versioned dependency after the runtime crate is published. Generation
rejects an existing output directory rather than replacing user files.

## Quick start

```sh
# Run a mapping: formats are chosen by file extension
cargo run -p cli -- run \
    --project examples/project.json \
    --input orders.xml \
    --output order_lines.csv

# --input/--output may be omitted when source_path/target_path are stored in the project
cargo run -p cli -- run --project examples/project.json

# Check graph, scope, and schema references without reading input data
cargo run -p cli -- validate --project examples/project.json

# Emit versioned JSON Lines diagnostics on stderr for automation
cargo run -p cli -- --diagnostics json validate --project examples/project.json

# Bootstrap a schema from existing metadata
cargo run -p cli -- import-xsd --xsd Orders.xsd
cargo run -p cli -- import-json-schema --schema customers.schema.json
cargo run -p cli -- import-db --db warehouse.db --table orders

# Launch the visual editor
cargo run -p gui
```

The integration tests double as worked examples — each pairs a project file with inputs
and expected outputs, covering XML-to-CSV flattening with broadcast fields, JSON,
XLSX, and SQLite round-trips, X12/EDIFACT extraction, and CSV enrichment joined
against a JSON reference file. See `crates/cli/tests/fixtures/`.

## Workspace layout

- `crates/ir` — schema-agnostic in-memory IR: schema trees and data instance trees
- `crates/mapping` — mapping graph IR (nodes/edges/functions/conditions) and project file format
- `crates/functions` — built-in function library (string/math/comparison/boolean)
- `crates/engine` — interprets a mapping graph against source instance(s) to produce target instance(s)
- `crates/codegen` — validated backend-neutral lowering and generated artifact model
- `crates/codegen-runtime` — runtime primitives used by generated Rust mappings
- `crates/codegen-rust` — deterministic Rust library source generator
- `crates/codegen-csharp` — deterministic standalone C# library source generator
- `crates/format-xml` — XSD-lite/DTD-lite schema import and XML instance read/write
- `crates/format-json` — JSON Schema import and JSON instance read/write
- `crates/format-csv` — delimited flat file schema and read/write
- `crates/format-xlsx` — flat Excel worksheet row read/write
- `crates/format-db` — database schema introspection and read/write (SQLite)
- `crates/format-edi` — EDI (ANSI X12 and UN/EDIFACT) schema-guided read/write
- `crates/format-protobuf` — bounded proto2/proto3 schema and binary instance read/write
- `crates/format-flextext` — embedded recursive structured-text layout read/write
- `crates/format-pdf` — bounded layout-driven PDF text extraction
- `crates/format-xbrl` — typed XBRL instance read/write
- `crates/cli` — headless runner (`ferrule` binary): run a project file against inputs
- `crates/gui` — visual mapping editor (`ferrule-gui` binary): schema trees, node-graph canvas,
  persisted layout sidecars, dirty-state guards, and project undo/redo

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
