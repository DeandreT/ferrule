# In-memory mapping pipelines

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

This is the typed, in-memory foundation for ordered N-to-M mappings. File and
service endpoint loading, artifact publication, stage-specific mapping paths,
and GUI authoring still need host integration. Ordinary `Project` JSON and
single-project execution remain valid.
