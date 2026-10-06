# Mapping pipelines

## Importing a connected `.mfd` design

The CLI can import a connected design with up to 64 serial XML pass-through
targets as a runnable pipeline. Its final primary target may be XML, delimited
CSV, fixed-width text, configured FlexText, JSON, Protocol Buffers, XBRL
without presentation metadata, or a new XLSX workbook:

```sh
cargo +nightly run -p cli -- import-mfd --mfd chained.mfd --pipeline --out flow.json
cargo +nightly run -p cli -- run-pipeline --pipeline flow.json \
  --input source source.xml --output mfd-stage-2 final.xml
```

The imported host name comes from the source component and is recorded in
`flow.json`. Supply that name and the desired output path to `run-pipeline`.
The example has one pass-through target. With three pass-through targets, the
final stage ID is `mfd-stage-4`.
The last pass-through target may also feed connected named XML targets in that
final stage, or one named CSV or JSON document target beside an XML primary target. Use
`--named-output STAGE TARGET PATH` to publish one. A bounded XML chain of at
least three stages may also feed the final stage from one nonadjacent earlier
intermediate as a named source while its immediate predecessor remains the
primary source. Other intermediate branches and bypasses reject during import.
CSV, fixed-width text, FlexText, JSON, Protocol Buffers, bounded XBRL, and XLSX
are supported as the final primary target. Intermediate targets remain XML;
other named targets remain XML.
XLSX targets that update an existing workbook are outside this profile.
Synthetic local round trips cover CSV, FlexText, JSON, and Protocol Buffers final targets. Local
fixed-width, FlexText, Protocol Buffers, and hierarchical XLSX mappings run after an identity XML
stage. Fixed-width and FlexText output retain exact serialized bytes and parsed
values; Protocol Buffers retains exact binary output and decoded messages;
XLSX retains decoded worksheet cells through strict export and reimport. A
fixture-backed XBRL final target retains exact instance XML bytes and parsed
fact/context elements; presentation metadata and numeric fact bindings remain outside this
chain profile.
XML-primary chains with a named CSV or JSON target retain both exact serialized
outputs through strict export, reimport, and CLI publication. Named JSON Lines
remain outside this profile.
Protocol Buffers export includes a referenced `.proto` sibling. These checks do
not establish reference-application acceptance.
An intermediate XML pass-through component can retain its declared output
instance and the next stage's source preview instance, even when those paths
differ. Import and export preserve both paths on that one component; the local
chained-report sample and a distinct-path synthetic case pass round trips.
Import does not infer an output file from a source preview path or an absent
instance. All four local chains export and reimport in the strict native
profile; one requires bounded native XML date/time casting and exact XSD
restoration. Unsupported casts reject before publishing artifacts.
A serial chain can also connect an original static XML host source to named
inputs in later stages, including intermediate stages and repeated use of the
same source. Export reuses that original component when its name, schema, path,
options, and output ports still match. Repeated connections from one output port
share one graph vertex with multiple edges. Other connected later-stage named
sources reject before publication. The earlier-result named source requires
the same XML schema, options, and preview path used when the producer's
immediate successor reads that result; one native component carries both
connections. Synthetic four-stage runs cover branches from the first and
second intermediates through execution and strict native export/reimport.
The same trusted package manifest and ordered EDI/JSON catalog options used
by ordinary `import-mfd` apply. Unsupported stage shapes fail before the
pipeline file is written. Each imported stage records the original `.mfd`
identity as a path relative to the saved pipeline, so moving both files
together preserves runtime mapping-path expressions.

`mapping::Pipeline` connects complete Ferrule projects into a stage graph. A
stage's primary source and each static named source can read a host-owned input,
the primary target of another stage, or one of that stage's named targets.
References use stable stage IDs, so stages may appear in any declaration order.
`engine::run_pipeline_with_context` validates the whole graph, executes it in a
stable dependency order, and returns all stage outputs only after every stage
succeeds. `engine::run_pipeline` is the simpler entry point for projects that
do not need host runtime context.

The ordinary serialized form has this shape; each `project` is a Ferrule
project. [Pipeline file loading](project-files.md) also accepts the versioned
JSON form used to retain exact floating-point values inside stages:

```json
{
  "stages": [
    {
      "id": "prepare",
      "project": { "...": "ordinary Ferrule project fields" },
      "source": { "kind": "host", "name": "orders" }
    },
    {
      "id": "invoice",
      "project": { "...": "ordinary Ferrule project fields" },
      "source": { "kind": "stage_target", "stage": "prepare" },
      "extra_sources": [
        {
          "name": "tax_table",
          "from": { "kind": "host", "name": "taxes" }
        }
      ]
    }
  ]
}
```

Add `"target": "NAME"` to a `stage_target` reference to read a named target.
An optional top-level `"main_mapping_path": "maps/original.mfd"` sets the
main mapping-file identity seen by runtime path expressions. Its relative path
resolves from the pipeline file's directory. Imported chained designs preserve
their original `.mfd` identity here; older pipelines without this field use the
pipeline JSON path.
For file-host runs, an optional
`"mapping_path": "stages/prepare.ferrule.json"` on a stage gives that stage
its active mapping-file identity for runtime expressions. Relative paths
resolve from the pipeline file's directory. When omitted, the pipeline file
remains the active mapping path, preserving existing pipelines. In-memory
callers can supply per-stage runtime paths through
`engine::run_pipeline_with_stage_contexts`.
Every static named source declared by the stage project needs exactly one
pipeline binding. Dynamic named sources continue to use the supplied
`ExecutionContext` loader. The primary and named output schema of a producing
stage must currently have the same typed shape and constraints as the receiving
boundary schema, though the root names may differ; incompatible edges,
missing IDs or targets, duplicate bindings, and cycles fail validation before
execution. Reusing one host input name with incompatible schemas also fails.

## File-host execution

Save the pipeline envelope as JSON and provide every host input and published
output explicitly:

```sh
cargo +nightly run -p cli -- run-pipeline \
  --pipeline flow.json \
  --input orders ./orders.json \
  --input taxes ./taxes.json \
  --output invoice ./invoice.json \
  --named-output prepare audit ./audit.json \
  --param batch=2026-09
```

Repeat `--input NAME PATH`, `--output STAGE PATH` for primary stage targets,
and `--named-output STAGE TARGET PATH` for named targets. The host loads each
input using the schema and format options of its bound stage boundary. A reused
host name must have compatible schemas and identical format options in every
place it is bound. A root schema name difference is allowed for explicitly
configured JSON or JSON Lines host inputs; other reused file inputs must have
identical schemas so parsing cannot depend on stage declaration order. Each
stage loads its own dynamic named sources, so stages may reuse a local source
name with different schemas and format options. Missing, extra, or
duplicate host names fail before any file is read. Input paths follow the
process working directory; computed dynamic source paths resolve from the
pipeline file's directory.

Every stage runs in dependency order with intermediate results held in memory.
Only targets named by `--output` or `--named-output` are written. Dynamic
document targets interpret their selected path as a base directory. All
selected targets are rendered and published as one batch after every stage
succeeds. Duplicate or overlapping output paths, and paths that would replace
the pipeline file or a loaded input file, including a member of an XML file
set, fail before publication. A later stage failure leaves earlier stage
outputs unpublished. The CLI publishes
primary selections before named selections, each in its option order; the
library API preserves its output request order.

`cli::run_pipeline_file` exposes this file host to Rust callers, with
`PipelineHostFile` and `PipelineOutputFile` selectors. The
`run_pipeline_file_with_options` variant accepts bounded typed runtime
parameters, an optional stage-qualified pre-insertion debug hook, and an
optional gate immediately before the batch is staged. A hook can cancel
evaluation at an ordinary target-field write in any stage. A gate can cancel
after every stage succeeds while leaving existing outputs untouched. Each
stage can use its own active mapping path while the optional
top-level main mapping path supplies the shared main identity; without it, the
pipeline file remains the main mapping path. Referenced local mapping files are
protected from output overwrite. All stages share one captured date-time
value. Ordinary `Project` JSON and single-project execution remain valid.

## In-memory preview

`cli::preview_pipeline_value_payloads` accepts host-owned bytes through
`PipelineHostPayload` and executes the complete graph once. Stage edges carry
typed instances directly, so an intermediate result does not need a temporary
file or a serialization round trip. The outcome contains every stage's primary
and named artifacts in execution order, with each artifact identified by its
stage, optional target name, and logical path. No output files are written.

```rust
let hosts = [cli::PipelineHostPayload::new(
    "orders",
    cli::PayloadDocument::new(std::path::Path::new("orders.json"), input_bytes)?,
)?];
let identities = [cli::PipelinePreviewOutputIdentity {
    stage: "invoice".into(),
    target: None,
    path: "preview/invoice.json".into(),
}];
let outcome = cli::preview_pipeline_value_payloads(
    &pipeline,
    std::path::Path::new("flow.json"),
    &cli::PipelinePreviewOptions::new(&hosts).with_output_identities(&identities),
)?;
```

Each target needs a logical format path, supplied explicitly or by its stored
target path. These paths identify returned bytes; they never select files for
publication. Stored relative instance paths resolve from the pipeline file's
directory, independently of the stage's runtime mapping identity. Distinct
stages may use the same logical path. Missing, extra,
duplicate, or incompatible host inputs and known unsupported decode/render
formats fail before stage evaluation. Typed stage edges ignore unused source
file hints. A stage or rendering failure returns no partial
outcome. Inputs and serialized outputs are limited to 64 MiB per document and
256 MiB each across the preview, with at most 4096 output artifacts.

Preview uses saved design-time host values when no host override is supplied.
Explicit overrides, including null, take precedence. All stages share one
captured date-time and main mapping identity, while retaining their own active
mapping identities. Stage-qualified trace and debug callbacks and a cancellation
flag are available through `PipelinePreviewOptions`. SQLite, update-existing
XLSX, local XML file sets, and dynamic named source loading require other hosts
and are unavailable in this preview API. Captured external-service payloads
can be supplied as bytes without a network request.

Local tests cover typed XML named-target to JSON transfers, stage-qualified
runtime values and observers, host overrides including null, late failures,
cancellation, and actual serialized output limits. A self-authored three-stage
import retains all output bytes across two strict native export/reimport cycles
without reopening a removed input file or modifying an existing output.

## Editing in the GUI

Use **File → Edit Pipeline** to open a saved pipeline as a separate document,
or **File → New Pipeline** to choose a new file path. The pipeline editor does
not replace the open mapping project or its canvas history. Add a stage from an
existing Ferrule project file; the editor embeds a copy, rebases its static
instance paths to the pipeline location, and starts its primary and static
named inputs as host bindings. Select a stage to rename its ID, change an input
to a host name or another stage's primary or named target, or remove a stage
that no other stage uses. Renaming rewires every downstream reference in one
edit. A loaded pipeline with missing static named-source bindings can add host
bindings for them from the selected stage. The editor shows whole-pipeline
validation issues and saves only a valid pipeline. It detects external file
changes before an atomic save and asks before discarding unsaved pipeline edits.
The **Run Pipeline** dialog prepopulates a host input path when stored paths
from its bound stages resolve to one file relative to the pipeline location.
Conflicting path hints leave that input blank. Output selection remains
explicit in the dialog.

Use **File → Import MFD as Pipeline** for a supported connected design. This
explicit action leaves ordinary **Import MFD** behavior unchanged for single
mappings. It reads and validates the connected stages before asking for a new
pipeline file location. The new document is unsaved until **Save pipeline**;
choosing a location does not create a file. The first stage is selected, and
the open mapping project, canvas layout, and undo history remain available.
Stored instance paths and the original mapping identity are rebased to the
chosen pipeline location. Cancelling either chooser or a failed import keeps
the previous pipeline document. Replacing a pipeline with unsaved edits uses
the existing **Keep editing** / **Discard changes** guard before opening a
chooser. Single-stage designs and unsupported branching retain the native
pipeline import diagnostic; they do not fall back to an ordinary import.
The toolbar's **Cancel** action clears the current import or export request
and releases the pipeline controls. An application close request also cancels
these MFD choosers before the usual unsaved pipeline and mapping guards run;
late dialog results cannot open a new document or publish an export.
While the unsaved-pipeline import prompt is pending, mapping edit controls and
shortcuts stay disabled until **Keep editing** or cancellation releases the
request. Close an existing **Run Pipeline** setup before starting an MFD
pipeline import or export.

The pipeline editor's **Export MFD (Ferrule)** and **Export native MFD** actions
export a snapshot of the current applied, validated pipeline edits. Saving the
pipeline JSON first is optional. Apply staged stage-ID or host-name text edits
before exporting. Paths are rebased from the pipeline location to the selected
design location, and the existing serial-chain preflight and selected export
profile apply before publication. Export does not save the pipeline document
or replace the main mapping canvas. The snapshot stays fixed while the output
chooser is open; cancelling it writes no artifacts. Diagnostics identify the
pipeline's stage IDs and preserve native warning text and available file or
component provenance. Native import currently rejects a chain with stage
warnings; export warnings do not carry a separate per-stage owner.

The Run Pipeline dialog can run, preview, or debug a saved pipeline on a worker.
**Preview pipeline** returns all intermediate and final outputs in the run
report without saving files. Its format paths are separate from the selected
Run output paths and start from stored target paths when available.
**Debug Preview** provides the same stage-aware stepping controls and retains
the completed trace for Node History, Source Rows, and Replay. Cancelling or
failing a preview discards its results without changing output files.

Debug
pauses before ordinary target-field writes, labels each pause with its stage
and target, and supports **Step**, **Continue**, **Pause at next write**, and
**Cancel pipeline**. Its breakpoint selector can choose one declared static
field and scope in one stage and target. An optional typed scalar condition
narrows that breakpoint to one complete pending value; strings beyond the
debugger's 160-character preview cannot match. An optional source-field
condition probes one immediate field in active frame 0–3 (0 is innermost) and
compares its complete typed scalar value; it can reach beyond the shallow
eight-field frame preview. An optional active-item position condition can be
combined with these filters. A value-node ID condition narrows the pause to a
target write driven by that node in the selected stage; it does not pause at
intermediate graph evaluation. **Step** and **Pause at next write** still stop at
the next ordinary write. Ordinary runs can also be cancelled before publication.
File Run and Debug pipeline
wait for all stages to finish before publishing selected outputs together; the
dialog and app close wait while publication is
in progress. Runs with no ordinary target-field write finish without a live
pause.
The completed run report retains a bounded trace with stage labels. Its Node
History, Source Rows, and Replay views can select a stage, keeping node IDs
from different stages separate while preserving global event order.
