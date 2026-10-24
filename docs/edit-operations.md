# Headless edit operations

`edit-ops` is the platform-neutral authoring boundary for native and WebAssembly
hosts. It accepts intent, not native file bytes or caller-supplied patches:

```rust,ignore
let commands = edit_ops::plan(&project, &operation)?;
```

Planning leaves the input untouched. It builds and checks `editor_ui::GraphCommand`
records against an isolated working model, in order. `EditSession` applies an
operation atomically and keeps its commands as one undo step in `EditHistory`.
It exposes the effective operation list and a `RevisionTracker`; the redo branch
is discarded by the next applied edit. History retains commands, not a project
snapshot per step.

## Wire format

An `EditScript` has a `format` equal to `ferrule.edit.v1` and an ordered
`operations` array. Operation and nested object members are closed: unrecognized
members are errors, including on empty variant objects. Node IDs, function IDs
and structural indices are canonical unsigned decimal **strings**, never JSON
numbers. Node IDs and indices are 32-bit; function IDs are 64-bit. IDs for new
nodes in the shared project graph, including disconnected-input replacements,
come from the caller and must be unused, including identities already referenced
by that graph's owners. Function-body graphs keep their separate ID namespaces.

```json
{
  "format": "ferrule.edit.v1",
  "operations": [
    {
      "op": "edit_constant",
      "node": "12",
      "value": { "type": "int", "text": "9007199254740993" }
    }
  ]
}
```

Scalar values carry exact typed text. Supported types are `string`, `bool`,
`int` and finite `float`; `null`, `json_null` and `xml_nil` are distinct empty
variant objects. Rust parses the text. Integers are signed 64-bit; floating-point
values retain negative zero. Value-map defaults distinguish a disabled default
(`null`) from an enabled absent default (`{"type":"null"}`).

## Operations and ownership

The v1 vocabulary covers target binding creation/replacement, retargeting and
removal; node input connection/disconnection; constant, builtin call and value-map
properties; node removal; static child scopes; source-backed iteration, filtering,
ordered sort keys and grouping; and ordered failure rules.

Failure rules can use source collections or the ordinary integer-range and
text-tokenizer sequence forms. Their item identities and any new bound,
predicate or message expressions are explicit in the operation. Composed
filter/map sequences remain read-only.

Scope addresses identify a primary or named target and its ordered static-child
indices. A binding additionally names its scalar target field. Source collections
can use a legacy path or explicitly identify the primary/named owner. Moving a
failure rule plans adjacent swaps, preserving the order of every other rule.

`disconnect.replacement` names the new hidden node for a required input; an
optional Raise message uses `null` and allocates nothing. `remove_nodes` consumes
replacement IDs by ascending surviving consumer ID, then input-slot order.
Edges between requested nodes and optional messages consume none. Missing,
duplicate or excess replacement IDs are refused.

Every change is expressed as an existing graph action. Required disconnected
inputs become hidden `Unconnected` nodes. Orphan cleanup removes unreferenced
hidden source reads and disconnected inputs, not visible constants or calls.
Graph and scope traversal preserves owners in other targets, failure rules,
dynamic sources and unsupported constructs; a protected owner prevents deletion.
Inspector-style binding removal and retargeting retain disconnected expressions
for reuse, matching their existing graph actions. Wire replacement and node
removal perform the canvas's hidden-input cleanup.

Schema and boundary definitions, user-function definitions, join plans,
filter/map sequences, XML type bindings, dynamic fields and format settings are
not rewritten by these operations. Their native representation stays intact.
The file codec remains the only native-byte encoder; hosts still run authoritative
mapping validation before accepting or saving a result.

The `functions` crate's default `runtime` feature is unchanged for execution
hosts. `edit-ops` uses it without default features to read the same builtin
catalogue without linking runtime UUID or randomness providers.
