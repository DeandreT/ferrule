# Invocation-owned JSON projection documents

The interpreter and generated Rust/C# mappings reuse a completely validated
JSON document across main-graph `json_parse_array_length`,
`json_parse_array_field`, `json_parse_field`, and `json_parse_object_exists`
demands. These helpers have the same graph-function document validation
profile. Projection-specific descriptor, path, type, index and absence checks
still run for each operation, in their original order.

## Ownership and lifetime

Each execution creates fresh state at its invocation entry, even when the
project, generated mapping, or host execution metadata is reused. Row and child
scope contexts share only that invocation's state. The state is not global,
thread-local, attached to a reusable project, or persisted after execution.

A slot belongs to one main-graph input-expression identity. Equal-valued
independent input expressions do not share slots. Input and schema descriptor
contents are compared exactly on every lookup; changed contents require full
validation before a projection can use the replacement. Duplicate equal schema
constants do not create separate slots. The graph-function profile is fixed by
the state type and dispatcher; host endpoint parsers with different limits are
not admitted. User-function body graphs keep ordinary stateless helper dispatch
and cannot alias main-graph node IDs.

Each source owner retains at most one validated document, its schema and the
input/descriptor strings used to identify it. It retains no history of changed
inputs. Validation failures are not cached; their typed error is attributed to
the operation that demanded validation. The number of slots is bounded by the
main graph's demanded input-expression owners, not by record count or connected
field count. No public project format, mapping expression or capability changes.

Projections return owned scalar values or booleans, never references or
enumerators into cache storage. Rust clones the selected scalar; C# instances
own immutable values after the decoder's `JsonDocument` is disposed. Replacing
a slot cannot invalidate an earlier result. Rust contexts keep their existing
single-threaded shared-context model; C# synchronizes document-state access
when callers share a context across threads. Independent executions never share
that state.

C# context construction requires an explicit state argument, including
target-typed constructors. Only the three actual root factories allocate it;
sorting/grouping selection views, completed group targets, and filter/map
callback boundaries forward it like other descendant views. Rust context
literals require the state field and context clones retain its shared owner.
Construction guards and generated sorted-grouping consumers cover these paths.
Filter/map callback-boundary controls also exercise the actual runtime directly;
they are not a claim of generated-graph filter/map conformance.

## Validation and demand

Reuse is of the complete validated representation, not a selected field or an
unvalidated JSON subtree. Malformed late records, unused invalid fields,
disallowed nulls, required properties and invalid duplicate-property tokens are
checked before any projection of a newly demanded document succeeds. Allowed
JSON null remains distinct from absence; array order and first-match lookup
selection are unchanged.

Unselected lazy branches and undemanded parser sources remain undemanded. An
empty primary iteration does not itself demand an independent lookup source;
a connected root-field demand still validates that source even when the array
or primary iteration is empty. No ordering is promised between independent
bindings. Cached documents do not suppress errors from unrelated demanded
expressions.

## Work bound and limits

For one main-graph source owner with unchanged input and schema, there is one
full document parse/validation attempt per successful invocation, independent
of projected row and field counts. Changed input/schema pairs and independent
owners require separate validation. Stateless direct helper calls and
user-function bodies retain their ordinary per-call parsing behavior.

This is a full-parse call-count bound, not a total CPU or allocation bound.
Each lookup can still compare the complete input/descriptor strings. Descriptor
decoding, field/path selection, lookup scans, expression evaluation and output
construction still have their own costs. The entire validated input is
materialized and retained until invocation end. There is no streaming or
large-array elapsed-time/peak-memory claim.

Existing limits and import refusals remain in force: required global String
inputs keep the 8 MiB parameter-string ceiling, generated sequences keep the
1,000,000-item ceiling, and declared schema constraints and JSON depth/keyword
limits still apply. Standalone graph helpers do not gain the host JSON endpoint
byte or general instance-node caps. This optimization does not admit previously
refused parser compositions, nested/scalar array adapters, nullable containers,
mixed feeds or unsupported lookup predicates.

## Instrumentation

The opt-in Rust `json-projection-instrumentation` feature (forwarding
`format-json/document-dom-instrumentation`) and C#
`FERRULE_JSON_PROJECTION_INSTRUMENTATION` build symbol count attempts at the
actual input-document DOM decoder call site, including failed attempts.
Counters retain no input data and are isolated to the calling thread. They do
not count wrapper entries, hits, schema/descriptor decodes, raw `uniqueItems`
or other keyword prepasses, elapsed time or memory. Refusals before the DOM
decoder legitimately count zero attempts. The synthetic count profile has no
raw uniqueness prepass; its count is not a claim about all internal parsing or
validation work for other schemas.

`functions` projection unit tests compare stateless and invocation-owned call
counts. `mfd`'s `json_projection_work` tests construct synthetic graphs and
independently authored complete outputs while varying rows and fields. Its
opt-in generated test compiles instrumented Rust and C# Release consumers and
checks complete outputs, typed failures, repeated executions and concurrent
invocations. `json_array_parser` retains the importer refusal, optional-parent
and error-profile regression cases.
