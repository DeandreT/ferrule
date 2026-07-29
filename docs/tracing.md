# Execution Tracing

Ferrule can record the interpreter decisions behind a filesystem mapping run:

```sh
ferrule run \
  --project mappings/order.json \
  --target primary \
  --trace-json traces/order.trace.jsonl
```

Tracing does not change the mapping output, the human result report on stdout,
or `--diagnostics json` records on stderr. `--trace-json -` is rejected because
stdout is reserved for the result report.

## Publication and Failure Behavior

Events stream to a private staging file beside the requested trace. Ferrule
flushes and atomically renames that file only after mapping execution and output
publication succeed. If validation, input loading, or execution fails, the
staging file is removed and an existing trace remains unchanged.

A trace path cannot be a directory, symlink, special file, or one of the
mapping's published output paths. Missing parent directories are created before
execution. Trace records can contain source values and paths; treat trace files
as potentially sensitive data.

## JSON Lines Envelope

Every line is one JSON object:

```json
{
  "schema_version": 1,
  "sequence": 0,
  "event": {
    "kind": "node_value",
    "node": 12,
    "positions": [],
    "value": {
      "type": "string",
      "value": "accepted"
    }
  }
}
```

`sequence` is zero-based, contiguous, and follows deterministic interpreter
evaluation order. Consumers must reject unsupported `schema_version` values and
ignore unknown fields within a supported version.

Scalar values retain their Ferrule domain:

| `type` | JSON representation |
| --- | --- |
| `absent` | `null`, meaning no value |
| `json_null` | `null`, meaning an explicit JSON null |
| `xml_nil` | `null`, meaning an explicit `xsi:nil` value |
| `bool` | JSON boolean |
| `int` | JSON integer |
| `float` | JSON number, or `"NaN"`, `"Infinity"`, `"-Infinity"` |
| `string` | JSON string |

## Event Kinds

The `event.kind` tag selects the event payload:

| Kind | Purpose |
| --- | --- |
| `node_value` | Successful graph-node result with active positions |
| `scope_started` | Scope identity, iteration source, and parent positions |
| `iteration_candidate` | Candidate ordinal and its source positions |
| `filter_decision` | Predicate node, control phase, and boolean result |
| `sort_candidate` | Evaluated ordered sort keys and bounded value previews |
| `sort_position` | Stable post-sort output index |
| `group_produced` | Group mode, size, optional key preview, and retention |
| `window_applied` | Evaluated window and before/after item counts |
| `target_field_written` | Successful static/dynamic binding or child insertion, source nodes, output shape, and optional bounded scalar preview |
| `target_produced` | Produced target kind and optional document path |
| `scope_finished` | Candidate count, produced count, and final output kind |

Every scope identity includes its primary or named target, semantic target path,
and structural index path. Structural paths distinguish concatenate segments
and repeated sibling names without depending on runtime values.

`target_field_written` is emitted only after a field is inserted successfully.
Its `binding.kind` is `static_binding`, `dynamic_binding`, `static_child`, or
`dynamic_child`; binding objects include the applicable `key_node` and
`value_node` identifiers. `field` is capped at 160 Unicode scalar values.
`output_kind` describes the inserted instance, while `value` is present only
for a scalar or singleton repeated-scalar write. Groups and larger collections
are never copied into this event.

Position records contain the source collection path, one-based index, grouping
state, optional join identity and tuple position, and optional document path.
Sort and grouping previews are Unicode-safe and bounded; `node_value` records
retain the complete scalar result.
