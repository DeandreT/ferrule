# ferrule

An open-source, Rust-native, any-to-any data mapping tool. Describe a source schema and a
target schema, wire them together with a mapping graph (functions, conditionals, lookup
tables, filters, cross-source joins), then run the mapping headlessly from the CLI or
interactively from the visual editor.

See [ROADMAP.md](ROADMAP.md) for the capability matrix and phased plan toward
MapForce workflow parity.

## Supported formats

Core formats work as both mapping sources and targets; one-way modes are noted:

- **CSV** — delimited flat files (configurable delimiter), typed columns
- **XLSX** — typed flat worksheet tables, composite/grid source layouts, and
  hierarchical targets with repeated runtime-named worksheets and ordered row ranges
- **XML** — hierarchical documents, with an XSD importer to bootstrap schemas
- **JSON** — hierarchical documents, with a JSON Schema importer
- **SQLite** — table introspection, reads, and idempotent full-replace writes
- **EDI** — ANSI X12 and UN/EDIFACT: separator discovery from the envelope, composite
  elements, repetition separators, qualifier-driven loops (HIPAA-style `HL`/`NM1`
  hierarchies), and a lenient mode that skips segments your schema doesn't mention
- **Protocol Buffers** (target) — bounded proto2 schema import and dynamic binary
  encoding for nested messages, enums, required/optional/repeated fields, and packed scalars
- **FlexText layouts** — bounded recursive split/store/switch pipelines with fixed-width
  and delimited records; embedded layouts work as both sources and targets, and MapForce
  `.mft` configurations compile into portable project data during import
- **PDF extraction** (source) — bounded positioned-text and painted-edge extraction from
  embedded visual layouts, including fixed captures, exact page groups, ordered region
  merges, anchors, and ruled or multiline unruled table rows

## How a mapping works

A project file (plain JSON) holds four things:

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
  generate scalar sequences with `tokenize`, `tokenize-by-length`, and
  inclusive integer ranges (capped at 1,000,000 materialized items per scope), or
  iterate a typed multi-source equijoin
- optional **extra sources** — named secondary inputs (reference data) that any scope or
  lookup can address by name

## Migrating from MapForce

ferrule can convert MapForce `.mfd` designs (best-effort): XML components
(resolvable XSDs, bounded DTDs, local includes/imports, attributes, simple-content values,
top-level element refs, named types, and extensions), requestless static HTTP GET calls
with typed XML responses, JSON components (JSON Schema with
local `$ref` support, or the design's entry tree as a fallback), CSV text components
(inline delimiter/header settings), external FlexText `.mft` layouts, flat and
hierarchical XLSX targets, visual PDF sources using supported external `.pxt` layouts,
including exact page selection and named multi-page table regions, proto2 binary targets,
single-table SQLite database components (schemas introspected from the referenced
database when it's reachable), the common core functions, the aggregate family
(count/sum/avg/min/max/string-join/item-at), constants, if-else, value-map, and
filter-, group-by-, distinct-values-, tokenizer-, and generated-range-driven iteration
import directly. Core kind-32 inner equijoins import with duplicate-preserving tuple
order, composite keys, projected fields, position, filter/sort/item-limit controls,
and count or computed scalar aggregates over the joined tuples;
`string` and decimal-safe `format-number` conversion are supported;
everything else is skipped
with an actionable warning so you can finish the mapping in the editor. Export
writes the exportable subset back out as `.mfd` plus generated XSD / JSON Schema files,
picking each side's component kind from the project's instance paths. Canonical
structured-join export covers root-context collections inside the primary source.
Designs built on namespaces, `xsi:type` polymorphism, correlated/keyless joins,
multi-source or nested join export, joined aggregate export, multi-table database wiring, or other
endpoints (unsupported Excel/PDF layout variants, PDF targets, XBRL, FlexText
string-parse, or EDI-config components)
are not converted yet.

```sh
cargo run -p cli -- import-mfd --mfd design.mfd --out project.json
cargo run -p cli -- export-mfd --project project.json --out design.mfd
```

MapForce is a trademark of its owner; ferrule is an independent project.

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
- `crates/format-xml` — XSD-lite/DTD-lite schema import and XML instance read/write
- `crates/format-json` — JSON Schema import and JSON instance read/write
- `crates/format-csv` — delimited flat file schema and read/write
- `crates/format-xlsx` — flat Excel worksheet row read/write
- `crates/format-db` — database schema introspection and read/write (SQLite)
- `crates/format-edi` — EDI (ANSI X12 and UN/EDIFACT) schema-guided read/write
- `crates/cli` — headless runner (`ferrule` binary): run a project file against inputs
- `crates/gui` — visual mapping editor (`ferrule-gui` binary): schema trees, node-graph canvas,
  persisted layout sidecars, dirty-state guards, and project undo/redo

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
