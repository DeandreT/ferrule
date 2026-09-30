# ferrule

ferrule is an open-source, Rust-native graphical data mapper. Connect source
and target schemas with functions, filters, aggregates, lookups, and joins,
then run the mapping from the native editor, the CLI, or an embedded Rust
application.

Projects are plain JSON. The mapping engine and format adapters are separate,
so one mapping can cross XML, JSON, tabular, database, EDI, binary, and other
document formats without format-specific graph logic.

## Highlights

- Visual mapping editor with undo/redo, saved canvas layouts, validation,
  in-memory preview, and run reporting
- Headless validation and execution with human-readable or JSON Lines
  diagnostics
- Nested iteration, outward value broadcast, filters, grouping, stable sorting,
  sequence windows, aggregates, lookups, and duplicate-preserving inner joins
- Multiple named inputs and outputs, including dynamic document paths
- Best-effort `.mfd` import and export with actionable warnings
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
Saved input preview values apply only to the editor's preview and debug preview.
Supplied run values override them; normal runs use connected defaults or require
a host value, independently of the saved preview.

Projects with additional targets can evaluate and publish only one target. This
is useful when target paths intentionally overlap or a host needs one artifact
class from a larger mapping:

```sh
cargo +nightly run -p cli -- run \
  --project project.json \
  --target audit \
  --output audit.json
```

Use `--target primary` for the primary target. Without `--target`, ferrule
evaluates every target and atomically rejects destination collisions.

For a stateless, payload-compatible mapping, the CLI can stream one primary
input and exactly one output artifact without creating input or output files.
The configured `source_path` and `target_path` select the input/output formats:

```sh
ferrule run project.json - - < order.csv > invoice.csv
```

`-` is valid only for the primary input and one stdout artifact. Named sources
keep their configured local paths. Dynamic named sources, SQLite, and
update-existing XLSX remain filesystem-host operations; mappings that produce
more than one artifact are rejected before stdout receives bytes. No success
report is mixed into stdout; diagnostics remain on stderr.

Rust hosts can run the same interpreter without temporary input or output files:

```rust
let input = cli::PayloadDocument::new(
    std::path::Path::new("order.json"),
    request_body,
)?;
let outcome = cli::run_project_payloads(
    std::path::Path::new("project.json"),
    &cli::PayloadRunOptions::new(input)
        .with_target(cli::TargetSelection::Named("audit")),
)?;
for artifact in outcome.artifacts {
    publish(artifact.path, artifact.bytes)?;
}
```

Logical paths select the format and identify returned artifacts. Payload runs
accept named static or dynamic secondary inputs, typed runtime parameters, and
tracing. Inputs and outputs are bounded to 64 MiB per document, 256 MiB per
run, and 4096 output artifacts; logical paths are limited to 4096 UTF-8 bytes
and source names to 256. SQLite and update-existing XLSX operations remain
filesystem APIs because they modify persistent state.
Payload hosts can explicitly select design-time inputs with
`.with_execution_purpose(engine::ExecutionPurpose::Preview)`; the default is a
normal run.

Pipelines also support an [in-memory preview](docs/mapping-pipelines.md#in-memory-preview)
that returns every stage's outputs, with stage-qualified tracing and debug
controls. The GUI's Run Pipeline dialog keeps preview format paths separate
from output files selected for saving.

Generated Rust and C# libraries also expose bounded schema-shaped JSON methods
such as `execute_json` and `GeneratedMapping.ExecuteJson`. Source-aware variants
accept exact named JSON inputs, and output-set variants return the primary
document plus ordered named target documents. See
[code generation](docs/code-generation.md#json-host-boundary).

## Common Workflows

Bootstrap schemas from existing metadata:

```sh
cargo +nightly run -p cli -- import-xsd --xsd Orders.xsd
cargo +nightly run -p cli -- import-json-schema --schema customers.schema.json
cargo +nightly run -p cli -- import-db --db warehouse.db --table orders
```

Import or export a `.mfd` design:

```sh
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --out project.json
cargo +nightly run -p cli -- import-mfd --mfd package/maps/design.mfd --package-root package --out project.json
cargo +nightly run -p cli -- import-mfd --mfd package/maps/design.mfd --package-manifest package/ferrule-package.json --out package/projects/project.json
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --edi-catalog-root edi-configs --out project.json
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --json-schema-root schemas --out project.json
cargo +nightly run -p cli -- import-mfd --mfd chained.mfd --pipeline --out flow.json
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd --check --report-json
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd
```

`export-mfd` defaults to `--profile ferrule-extensions`, preserving Ferrule's
round-trip features. `--profile native-mfd` refuses known Ferrule-only
dependencies and lossy exports before writing any artifacts. `--check` renders
the export for compatibility inspection without writing files or directories;
`--report-json` prints a versioned, deterministic report on stdout. See
[`.mfd` interoperability](docs/mfd-interop.md#export) for the report's
limits.

For a supported connected XML design with up to 64 serial pass-through
targets, `import-mfd --pipeline` writes a typed pipeline with one stage per
target.
Run it with `run-pipeline` by supplying its host inputs and selected stage
outputs.

Emit machine-readable validation diagnostics:

```sh
cargo +nightly run -p cli -- --diagnostics json validate --project project.json
```

Capture a deterministic execution trace without changing stdout or diagnostic
output:

```sh
cargo +nightly run -p cli -- run --project project.json --trace-json run.trace.jsonl
```

Run a [mapping pipeline](docs/mapping-pipelines.md) and publish only selected
stage targets after the complete graph succeeds:

```sh
cargo +nightly run -p cli -- run-pipeline --pipeline flow.json \
  --input orders orders.json --output invoice invoice.json \
  --named-output prepare audit audit.json
```

## Documentation

- [Mapping model and workspace architecture](docs/architecture.md)
- [Mapping pipelines](docs/mapping-pipelines.md)
- [Execution trace JSON Lines contract](docs/tracing.md)
- [Supported formats](docs/formats.md)
- [`.mfd` interoperability](docs/mfd-interop.md)
- [Rust and C# code generation](docs/code-generation.md)
- [Runnable generated Rust and C# hosts](examples/codegen/)
- [Compatibility and product-parity roadmap](ROADMAP.md)

The integration fixtures under `crates/cli/tests/fixtures/` are executable
examples covering XML, JSON, CSV, SQLite, X12, EDIFACT, and cross-source
enrichment.

## License

Licensed under the [GNU General Public License v3.0](LICENSE).

ferrule is an independent project.
