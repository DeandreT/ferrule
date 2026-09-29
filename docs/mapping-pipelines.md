# Mapping pipelines

`mapping::Pipeline` connects complete Ferrule projects into a stage graph. A
stage's primary source and each static named source can read a host-owned input,
the primary target of another stage, or one of that stage's named targets.
References use stable stage IDs, so stages may appear in any declaration order.
`engine::run_pipeline_with_context` validates the whole graph, executes it in a
stable dependency order, and returns all stage outputs only after every stage
succeeds. `engine::run_pipeline` is the simpler entry point for projects that
do not need host runtime context.

The serialized envelope has this shape; each `project` is an ordinary existing
Ferrule project:

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
parameters. All stages currently share the pipeline file as their active
mapping path and one captured date-time value. Ordinary `Project` JSON and
single-project execution remain valid.
