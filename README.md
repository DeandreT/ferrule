# ferrule

ferrule is an open-source, Rust-native graphical data mapper. Connect source
and target schemas with functions, filters, aggregates, lookups, and joins,
then run the mapping from the native editor, the CLI, or an embedded Rust
application.

Projects are plain JSON. The mapping engine and format adapters are separate,
so one mapping can cross XML, JSON, tabular, database, EDI, binary, and other
document formats without format-specific graph logic.

## Highlights

- Visual mapping editor with undo/redo, saved canvas layouts, validation, and
  run reporting
- Headless validation and execution with human-readable or JSON Lines
  diagnostics
- Nested iteration, outward value broadcast, filters, grouping, stable sorting,
  sequence windows, aggregates, lookups, and duplicate-preserving inner joins
- Multiple named inputs and outputs, including dynamic document paths
- Best-effort MapForce `.mfd` import and export with actionable warnings
- Deterministic Rust and package-free C# mapping-library generation for the
  supported portable subset

See [Supported formats](docs/formats.md) for the complete direction and feature
matrix.

## Quick Start

ferrule currently uses the Rust nightly toolchain:

```sh
rustup toolchain install nightly
```

Run the checked-in XML-to-CSV example:

```sh
cargo +nightly run -p cli -- run \
  --project crates/cli/tests/fixtures/orders/project.json \
  --input crates/cli/tests/fixtures/orders/Orders.xml \
  --output order-lines.csv
```

Validate a project without reading its input:

```sh
cargo +nightly run -p cli -- validate \
  --project crates/cli/tests/fixtures/orders/project.json
```

Launch the native visual editor:

```sh
cargo +nightly run -p gui
```

Run `cargo +nightly run -p cli -- --help` for the complete CLI. Input and
output paths may be omitted when the project stores `source_path` and
`target_path` values.

Mappings with typed host inputs accept repeatable parameters:

```sh
cargo +nightly run -p cli -- run \
  --project project.json \
  --param correlation_id=txn-42 \
  --param control_number=7001
```

CLI parameter values begin as strings and are coerced by each declaration's
scalar type. Names are exact and duplicates are rejected before input is read.

## Common Workflows

Bootstrap schemas from existing metadata:

```sh
cargo +nightly run -p cli -- import-xsd --xsd Orders.xsd
cargo +nightly run -p cli -- import-json-schema --schema customers.schema.json
cargo +nightly run -p cli -- import-db --db warehouse.db --table orders
```

Import or export a MapForce design:

```sh
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --out project.json
cargo +nightly run -p cli -- import-mfd --mfd package/maps/design.mfd --package-root package --out project.json
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd
```

Emit machine-readable validation diagnostics:

```sh
cargo +nightly run -p cli -- --diagnostics json validate --project project.json
```

## Documentation

- [Mapping model and workspace architecture](docs/architecture.md)
- [Supported formats](docs/formats.md)
- [MapForce interoperability](docs/mapforce-interop.md)
- [Rust and C# code generation](docs/code-generation.md)
- [Runnable generated Rust and C# hosts](examples/codegen/)
- [Workflow-parity roadmap](ROADMAP.md)

The integration fixtures under `crates/cli/tests/fixtures/` are executable
examples covering XML, JSON, CSV, SQLite, X12, EDIFACT, and cross-source
enrichment.

## License

Licensed under the [GNU General Public License v3.0](LICENSE).

MapForce is a trademark of its owner. ferrule is an independent project.
