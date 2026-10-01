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
stdout is reserved either for the result report or raw mapped bytes from
`ferrule run PROJECT - -`.

## Publication and Failure Behavior

Events stream to a private staging file beside the requested trace. Ferrule
flushes and atomically renames that file only after mapping execution and output
publication succeed. If validation, input loading, or execution fails, the
staging file is removed and an existing trace remains unchanged.

A trace path cannot be a directory, symlink, special file, or one of the
mapping's published output paths. Missing parent directories are created before
execution. Trace records can contain source values and paths; treat trace files
as potentially sensitive data.

Standard-output runs finalize the trace after successful mapping serialization
and before writing raw artifact bytes. This preserves an empty stdout when trace
publication fails. A later stdout write failure, such as a broken pipe, can
therefore leave a trace for the successfully evaluated mapping.

## JSON Lines Envelope

Every line is one JSON object:

```json
{
  "schema_version": 4,
  "sequence": 0,
  "event": {
    "kind": "node_value",
    "node": 12,
    "positions": [],
    "value": {
      "type": "string",
      "preview": "accepted",
      "truncated": false
    }
  }
}
```

`sequence` is zero-based, contiguous, and follows deterministic interpreter
evaluation order. Consumers must reject unsupported `schema_version` values and
ignore unknown fields within a supported version.
Version 4 adds function-qualified value and input-delivery events for reusable
function bodies. It leaves existing event payloads unchanged and records the
function ID separately from its local node IDs. Version 3 added graph
input-consumption events; previously saved trace files retain their declared
versions. JSON diagnostics on stderr use a separate schema.

Scalar previews retain their Ferrule domain and are capped at 160 Unicode
scalar values:

| `type` | `preview` representation |
| --- | --- |
| `null` | `"null"`, meaning no value |
| `json-null` | `"json-null"`, meaning an explicit JSON null |
| `xml-nil` | `"xml-nil"`, meaning an explicit `xsi:nil` value |
| `bool` | `"true"` or `"false"` |
| `int` | Decimal integer text |
| `float` | Finite or non-finite float text |
| `string` | The bounded string prefix |

`truncated` is true when a string result exceeded the preview bound.

## Event Kinds

The `event.kind` tag selects the event payload:

| Kind | Purpose |
| --- | --- |
| `node_value` | Successful graph-node result with active positions |
| `node_input_value` | Successful value delivered from a graph node to one input pin of a consuming node |
| `function_node_value` | Successful result of a node inside a reusable function body, qualified by `function` ID |
| `function_node_input_value` | Successful value delivered to one function-body input pin, qualified by `function` ID |
| `scope_started` | Scope identity, iteration source, and parent positions |
| `iteration_candidate` | Candidate ordinal, raw source positions, and optional bounded source-row preview |
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
All scalar previews are Unicode-safe and bounded, including graph and
function-body node input/output records.

For source and dynamic-document iterations, `iteration_candidate.source_row`
captures the current source item before filters and sorting. This optional
version-3 field is absent for once, generated, and join iterations. A scalar
row has `kind: "scalar"` and a `value` preview. A group row has up to eight
ordered immediate `fields`, each with a bounded `name`, `name_truncated`,
`kind`, and an optional scalar `value`. `omitted_fields` counts additional
fields. Names and value previews together use at most 512 UTF-8 bytes per row,
so a row preview can truncate a scalar earlier than the general 160-character
limit. The field is additive: existing version-3 readers can ignore it.
Source-row previews can contain fields the mapping did not use.

Structural source rows also have an optional `structure` tree in version-4
traces. It retains ordered nested groups, repeated and mapped-sequence items,
portable document-member paths, and the distinct scalar absence/null/nil states.
Each tree has at most 64 nodes, a maximum depth of eight child edges, and eight children
per container. All tree names and scalar previews share an additional 512-byte
UTF-8 budget. `omitted_children` counts values not captured; `depth_limited`
identifies depth truncation. Name and value truncation remain explicit. Flat
scalar-only rows retain their existing representation. The GUI Source Rows
details and Replay show the nested snapshot and omissions; row search includes
retained nested values and respects the selected pipeline stage.
Completed-report search caches each normalized query and selected stage.
Bounded GUI collectors decline new source snapshots once their retained event
prefix is full, including across pipeline stages; omitted-event counts still
advance. Streaming CLI traces continue to capture every source candidate.

`node_input_value` identifies the `consumer` node, its zero-based `input_index`,
and the producing `input` node. It is emitted immediately after the producer
returns a value, before the consumer continues. Ordinary built-in and user
function calls, conditional expressions, value maps, lookups, and dynamic
source-field keys emit these events. Aggregate and join-aggregate graph
expressions emit one input event for each successfully evaluated collection
item or joined tuple, with that item's position; their optional scalar argument
emits one event afterward in the parent context, even for an empty collection.
Input indices match visible pins: the expression is 0, and the argument is 1
when an expression is present or 0 otherwise. Direct aggregate value paths are
not graph inputs and do not emit input events. An untaken conditional branch
and a producer that errors do not emit an input event. Generated-sequence,
mixed-content, and collection-search inputs are recorded at the visible pins
that are evaluated. The native
GUI's History tab has graph-node and source-row views. The source-row view
shows retained candidate rows by scope, ordinal, and raw source position, with
their bounded field previews; it does not link them to node evaluations whose
positions may change during sorting or grouping. The Replay tab navigates the
completed trace one recorded event at a time (first, previous, next, or next
recorded input or output of a selected graph node). Its detail shows the selected event's
positions and, for a source candidate, that same event's bounded row fields.
Trace rows, node-history occurrences, and selected source-row details can open
Replay at their exact retained event, including when the report is filtered.
This is navigation of recorded history, not a live pause or re-execution. The
GUI retains at most 50,000 trace events and reports when later events were
omitted; replay ends at the retained prefix. Run and Preview currently show
this trace only after successful completion.

`function_node_value` and `function_node_input_value` use the same bounded
values, positions, and zero-based pin indexing inside an isolated reusable
function body. Their `function` ID qualifies each local node ID, including in
nested calls; the ordinary `node_value` and `node_input_value` events continue
to identify nodes in the project graph. Only successfully evaluated nodes and
delivered inputs are recorded, so untaken conditional branches and failed
expressions have no value event. Saved GUI History and Replay show these
function-qualified events. Live expression breakpoints can select one function
ID and node ID and pause after that body node succeeds, with the caller's
positions and no function-local source frames. Function-body input-pin
breakpoints are not yet supported.

Library hosts can opt into a synchronous pre-insertion control point with
`ExecutionContext::with_debug_hook`. For ordinary static and dynamic target
bindings and child fields, the hook receives the pending field's bounded value
preview, source positions, the innermost four active source frames with up to
eight immediate fields each, and at most eight already inserted fields from
the current scope draft. Long names and scalar values are truncated; private
XML metadata fields are hidden. A host may also request one exact immediate
source field from frame 0–3 (0 is innermost); that probe can reach a field
outside the eight-field snapshot without expanding the snapshot. It can wait
for a host decision and resume, or
cancel with the typed `EngineError::DebugCancelled` before that field is
inserted. Existing post-insertion trace events are unchanged. Scalar, copy,
recursive, and other special constructors do not use this ordinary-field hook.
The callback's partial draft is not a complete target document.

The GUI's **Debug preview** runs an in-memory mapping on a worker and pauses
before each ordinary target-field insertion. The paused view shows the
pending value, target scope, source positions, and up to eight fields already
inserted in that scope. It also shows the bounded active source frames, from
outer to inner, with omitted-frame and omitted-field counts. **Step** inserts
the pending field and pauses at the next ordinary write; **Continue** runs
until completion, and **Pause at next write** can stop a continued run at a
later write. **Cancel** discards the preview result. A plain Preview also runs
on a worker and remains responsive while it executes. The completed output
and trace appear in Preview results; the Replay tab remains a separate
navigation view over that recorded trace.
The optional Debug breakpoint selector pauses at one declared static target
field in one target-scope path. After **Continue**, it pauses again when that
field is written in a later source row; **Step** still stops at the next
ordinary write. Runtime-named dynamic fields are not listed in the selector.
An optional scalar condition pauses only when the pending write has the chosen
type and complete value; it combines with a selected field and scope. It
distinguishes absent, JSON, and XML nulls, normalizes entered finite numbers
and booleans, and does not match a truncated string preview. Strings over 160
Unicode characters cannot be matched this way. **Step** and **Pause at next
write** still stop at the next ordinary write regardless of the condition.
An optional 1-based active-item condition uses the innermost displayed position
after scope controls. It combines with the field and scalar conditions; writes
with no active collection position never match it. Invalid or zero item numbers
block the debug run before evaluation. **Step** and **Pause at next write**
still override the condition.
An optional source-field condition selects an immediate field in one of the
four innermost active frames and compares its complete typed scalar value.
It combines with the target, pending-value, and active-item conditions. Missing
fields, non-scalars, private XML metadata, and truncated scalar previews never
match. The paused view shows the exact field probe, including fields beyond
the shallow frame snapshot. **Step** and **Pause at next write** still stop at
the next ordinary write regardless of this condition.
An optional value-node condition pauses before a target write whose static or
dynamic binding reads the entered graph node ID. It can distinguish two nodes
that produce the same scalar value and combines with the other conditions.
The dynamic property-name node and child-scope writes do not match. It does
not pause during intermediate graph evaluation.
Target-write breakpoints do not pause runs with no ordinary target-field
insertion.
An independent expression breakpoint selects an exact graph node ID and may
also require a complete typed scalar value. It pauses after that node evaluates
successfully, including in a filter or pre-target rule that produces no target
write. Step from this pause advances to the next evaluated node. The pause
shows a bounded value, source-frame snapshot, and active positions; pipeline
expressions can be limited to one selected stage. Cancel at this point aborts
before output publication.
An independent input-pin breakpoint selects a consumer graph node ID and a
1-based input number, optionally with a complete typed delivered scalar value.
It pauses just after that input's `NodeInputValue` event is recorded, including
in filters that produce no target write. The pause identifies the producer,
consumer, delivered value, source context, and active positions. Step advances
to the next recorded input delivery, even in another pipeline stage; a pipeline
breakpoint itself can be limited to one stage. Cancel aborts before output
publication. Only graph pins that currently produce `NodeInputValue` events
participate: untaken lazy branches, target bindings, and scope-control edges do
not. The UI numbers pins from 1, while trace event `input_index` remains
zero-based.

**Debug Run** offers the same controls for a saved, file-backed mapping and
can select a static field in the primary or a named target. Ordinary Run also
executes on a worker. Evaluation finishes before the file host begins staging
outputs; Cancel before that publication boundary leaves existing target files
unchanged. Once publishing begins, the GUI waits for the worker to finish and
defers app close. Completed output and trace appear in the run report.
Saved pipeline runs offer the same live controls and conditions, with
pauses labeled by stage and target. A breakpoint can select a declared static
field and scope in one stage and target. Cancel before pipeline publication
preserves every selected output, including targets of earlier completed stages.
Once publication begins, the GUI waits for the worker and defers app close.
Completed pipeline reports retain one bounded, ordered trace across stages.
Trace rows show their stage; Node History and Source Rows can select a stage so
reused node IDs stay distinct. Selecting a stage in Replay jumps to its first
retained event. The same 50,000-event prefix limit and dropped-event count
apply to the whole pipeline, and the versioned trace-event format is unchanged.
