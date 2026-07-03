# ferrule

An open-source, Rust-native graphical any-to-any data mapping tool. Wire a source schema
(XML/XSD, JSON, CSV, database, EDI, ...) to a target schema, apply built-in or custom
functions along the way, and run the mapping or generate code from it.

Status: early scaffolding, not yet usable. See the crate layout below for the current plan.

## Workspace layout

- `crates/core` — schema-agnostic in-memory IR: schema trees and data instance trees
- `crates/mapping` — mapping graph IR (nodes/edges/functions/conditions) and project file format
- `crates/functions` — built-in function library (string/math/date/aggregate/node-set)
- `crates/engine` — interprets a mapping graph against source instance(s) to produce target instance(s)
- `crates/format-xml` — XSD-lite schema import and XML instance read/write
- `crates/format-json` — JSON Schema import and JSON instance read/write
- `crates/format-csv` — delimited/fixed-width flat file schema and read/write
- `crates/format-db` — database schema introspection and read/write
- `crates/format-edi` — EDI (ANSI X12) schema-guided read/write
- `crates/cli` — headless runner (`ferrule` binary): run a project file against inputs
- `crates/gui` — visual mapping editor (`ferrule-gui` binary): schema tree panes + node-graph canvas

## License

Licensed under the [GNU General Public License v3.0](LICENSE).
