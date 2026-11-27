# Running mappings

Use `ferrule validate` to check a saved project and `ferrule run` to map data.
Run the shell examples from the repository root in the same POSIX shell. If
`ferrule` isn't on your path, substitute `cargo run --locked -q -p cli --`
for `ferrule` (see [Toolchain baselines](toolchains.md)).

## Validate and run

Validate the [invoice example](../examples/codegen/project.json), then write its
output to a fresh directory:

```sh
demo=examples/codegen
out=$(mktemp -d)
ferrule validate --project "$demo/project.json"
ferrule run "$demo/project.json" --output "$out/invoices.json"
```

Validation checks the mapping without reading instance data. The run reads
`input.json` beside the project and writes [three invoices](../examples/codegen/expected-output.json)
in one JSON document. Its success report goes to stdout.

For static files, `--input` and `--output` override the project's `source_path`
and `target_path`. Relative saved paths start in the project directory; relative
command-line paths start in your working directory. Pick a separate output
file: a successful file run can replace an existing file.

## Parameters and target selection

This project declares a string `Tag` and a Boolean `Stop`. Select its named
`Audit` target to write just the JSON receipt:

```sh
sample=crates/cli/tests/code_generation/static_document_adapters/fixtures
ferrule run "$sample/projects/inverse.json" --input "$sample/reference-input.x12" \
  --target Audit --output "$out/audit.json" --param Tag=batch-42 --param Stop=false
```

The receipt contains `"tag": "batch-42"` and `"echo": "REF"`. Use
`--target primary` for the primary target. Without `--target`, the run
evaluates all targets; `--output` overrides only the primary destination.
A selected target that names documents dynamically takes an output directory.
Duplicate output destinations and file/directory overlaps fail before publication.

Repeat `--param NAME=VALUE` for each host input. Names are case-sensitive;
duplicates are rejected before input is read. The CLI splits at the first `=`
and passes text, which the mapping converts to the declared scalar type when
used. A normal run ignores saved preview values: an omitted value uses its
connected default or fails when required. A parameter set allows 1,024 names
(up to 256 UTF-8 bytes each) and up to 8 MiB per string value; the operating
system can impose a smaller command-line limit.

## Stdin, stdout and diagnostics

Use `-` for the primary input and for the output to produce the same invoices
through standard streams:

```sh
ferrule --diagnostics json run "$demo/project.json" - - \
  < "$demo/input.json" > "$out/piped.json" 2> "$out/diagnostics.jsonl"
cmp "$out/invoices.json" "$out/piped.json"
```

Stdout contains only the artifact bytes. Diagnostics go to stderr (human by
default, one JSON object per line with `--diagnostics json`); a failed run
returns a nonzero exit code. The same flag works with `validate`. Add
`--trace-json "$out/run.trace.jsonl"` to write an [execution trace](tracing.md)
to a separate file. Use fresh redirection paths: the shell opens `>` files
before Ferrule runs.

Use `--output -` to send a file input's result to stdout; `--input -`
requires `--output -`. Stdin needs a configured `source_path` and stdout needs
the selected target's saved path so Ferrule can identify the formats. Required
static named inputs still come from their configured local files. For per-item
dynamic sources, supply files or use the Rust payload host below. SQLite and
update-existing XLSX require filesystem access.
If the run produces zero or several artifacts, it fails before writing any
artifact bytes to stdout. The interpreter still holds parsed data and output
in memory; see [Memory use and boundary limits](memory-and-limits.md).

## Rust payload host

To map bytes your application owns, depend on Ferrule's `cli` crate. For a
`my-host` crate next to the `ferrule` checkout, add
`cli = { path = "../ferrule/crates/cli" }` under `[dependencies]`. Put this
in `src/main.rs` and run `cargo run --manifest-path ../my-host/Cargo.toml`
from the Ferrule root. The XML receipt contains the supplied timestamp in
both `Label` and `Echo`:

```rust
use std::io::{self, Write};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = Path::new(
        "crates/codegen/src/tests/fixtures/static_primary_mixed_named_xml_documents.json",
    );
    let body = b"<Input><StaticBad>false</StaticBad><StaticContext>true</StaticContext></Input>";
    let input = cli::PayloadDocument::new(Path::new("request.xml"), body)?;
    let options = cli::PayloadRunOptions::new(input)
        .with_target(cli::TargetSelection::Named("z-static"))
        .with_output_path(Path::new("receipt.xml"))
        .with_current_datetime("2026-10-13T07:08:09.123456789-07:00")?;
    let result = cli::run_project_payloads(project, &options)?;
    let [receipt] = result.artifacts.as_slice() else {
        return Err("expected one receipt".into());
    };
    io::stdout().lock().write_all(&receipt.bytes)?;
    Ok(())
}
```

`run_project_payloads` reads the saved project; `run_project_value_payloads`
accepts an already loaded `mapping::Project`. Both return a complete set of
serialized artifacts or an error. In the default profile, logical paths and
format options select the codecs. `receipt.xml` identifies the returned bytes.
Static artifact paths can be absolute or contain `..`; choose a destination
in your application if you save the bytes to disk.

Use `with_extra_sources` for host-supplied `NamedPayloadInput` documents: one
per required static source, or one per distinct logical path for a dynamic
source. `with_runtime_parameters` accepts an `engine::RuntimeParameters` set;
`with_trace_sink` observes execution. To use saved design-time values, select
`with_execution_purpose(engine::ExecutionPurpose::Preview)`; supplied values
still win. See [payload limits](memory-and-limits.md#limits-are-route-specific)
and the opt-in [bounded document profile](design/bounded-document-payloads.md).

`with_current_datetime` fixes one clock for evaluation and all selected
serializers. It accepts ASCII `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)`
up to 35 bytes: a valid Gregorian date in years 0001–9999, hours 00–23,
minutes and seconds 00–59, an optional 1–9 digit fraction, and an explicit
offset between −14:00 and +14:00 inclusive. The builder rejects invalid values
and preserves accepted spelling. Omitting it samples the clock once per run.
Replaying the time does not replay random values or GUIDs.

## Related workflows

- To get a source or target schema as JSON, run
  `ferrule import-xsd --xsd crates/cli/tests/fixtures/orders/Orders.xsd`.
  `import-json-schema --schema FILE` and `import-db --db FILE --table TABLE`
  do the same for a JSON Schema or a SQLite table.
- For an existing `.mfd` design, see [import options](mfd-interop.md#import)
  and the [export compatibility check](mfd-interop.md#export).
- For connected projects, see [running a pipeline](mapping-pipelines.md#file-host-execution)
  and [previewing all stages in memory](mapping-pipelines.md#in-memory-preview).
- To embed a compiled mapping, see the [generated JSON host API](code-generation.md#json-host-boundary)
  and [runnable Rust and C# hosts](../examples/codegen/README.md).
