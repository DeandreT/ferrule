<a id="ordered-scalar-filtermap-proposal"></a>

# Ordered scalar filter/map

`filter_map_v1` evaluates an inclusive integer range, retains items for
which a scalar predicate returns true, and maps each retained item through
a second project-local scalar function. The result is eager, ordered and
private to its consumer. [MFD export refuses this descriptor](../conformance/scalar-sequence-native-controls.md).

## Current source boundary

The [model](../../crates/mapping/src/filter_map.rs),
[interpreter](../../crates/engine/src/filter_map.rs) and
[common lowering](../../crates/codegen/src/lower.rs) implement the descriptor.
A Generate range defaults its lower bound to 1, is empty for reversed
bounds or a short-circuiting Null/JsonNull argument, and admits at most
1,000,000 items. Stage calls have stricter result types than ordinary scalar
UDFs: successful adaptation alone can still leave a null marker.

<a id="finite-model-and-wire-proposal"></a>

## Saved descriptor

```json
{
  "kind": "filter_map_v1",
  "source": { "kind": "generate", "from": 1, "to": 2, "item": 10 },
  "item": 11,
  "predicate": 100,
  "mapper": 101,
  "output_type": "int",
  "captures": [{ "node": 20, "ty": "int" }]
}
```

The numbers are graph and function IDs, not literals. Admitted consumers
are a generated scope, `SequenceExists`, `SequenceItemAt` and
`SequenceAggregate`. A sequence failure rule refuses the transform; its
legacy generators are unaffected. Each reached consumer builds its own
sequence.

| Boundary | Initial admission |
| --- | --- |
| Source | Exactly one existing `Generate`; optional `from`, required `to`; no transform nesting or other generator variants |
| Input item | `Int` with the existing generator's argument coercion and item cap |
| Predicate | Existing project-local UDF, declared `Bool` output; actual adapted result must be `Bool` |
| Mapper | Existing project-local UDF; declared output equals `output_type` |
| Output item | Exactly `Int`, finite `Float`, `Bool` or `String`; exact actual tag must equal `output_type` after the ordinary UDF return adapter |
| Captures | 0–16 ordered parent graph nodes, each declared as one of the four scalar types and checked for an exact tag; `Float` must be finite |
| Stage parameters | Exactly `[item: Int, source_position: Int, capture_0, ...]`, with stable ordered parameter IDs and matching capture types |
| Stage body graph | `Const`, `Unconnected`, `FunctionParameter`, `Call`, `If`, `Raise`, acyclic `UserFunctionCall` |
| Builtins in a stage's transitive UDF graphs | Binary `add`, `multiply`, `divide`, `equal`, `not_equal`, `less_than`, `greater_than`, `concat`, preserving their existing scalar/error semantics |
| Nested UDF signature | At most 18 parameters; the same body/builtin admission applies transitively; ordinary scalar adapters remain in use |

Body intermediates can contain existing scalar absence/null markers and a computation can produce a nonfinite float. They are not successful stage outputs. `Null`, `JsonNull` and `XmlNil` remain distinct; none is an implicit filter result, emitted item or coercion to an empty string. `Group`, `Repeated`, `MappedSequence` and `DocumentSet` are not scalar stage inputs/outputs.

`SourceField`, `Position`, runtime values/parameters, reducers, source documents, joins and other context-sensitive nodes are rejected inside the stage's transitive UDF graphs. Parent data enters only through captures. This restriction applies to UDFs used by this descriptor; it does not narrow ordinary UDF admission elsewhere. A valid `Raise` in an unselected branch is admitted and stays lazy.

The source's `item` and the output `item` must be different, uniquely owned, unframed empty-path `SourceField` node IDs. The inner ID is reserved by the source descriptor and cannot be read by captures or downstream expressions. Stage input is supplied explicitly as a UDF parameter. Only the output ID is exposed to the downstream consumer. Both IDs must participate in duplicate/missing-owner validation; treating `item()` as the output ID cannot silently omit validation of the source ID.

## Evaluation and position contract

Static admission visits the whole descriptor and its transitive UDF graphs, including both sides of an `If`. Runtime selection can skip a valid descriptor; it cannot make an invalid signature, missing callee or prohibited node admissible.

For a reached descriptor, the following order is fixed:

1. Evaluate `from` when present, then `to`, in the parent context. Preserve the existing absent-argument short circuit. Coerce the completed bound values afterward, in lower/upper order; thus an upper expression's error precedes an earlier lower value's coercion error. Validate the primitive range cap, reserve the shared item/work budget, then materialize its integers.
2. Evaluate captures once each, left to right, still in the parent context. Check each exact tag immediately before evaluating the next capture. This happens even when the generated sequence is empty. Parent capture results are retained for the invocation; cloning by ordinary UDF argument adapters is still possible.
3. For each source integer in increasing generator order, invoke the predicate with item, original 1-based source position and the captured values. Apply its ordinary return adapter, then the strict `Bool` boundary check. On `false`, skip the mapper entirely.
4. On `true`, invoke the mapper with the same arguments. Apply its ordinary return adapter, then check exact output tag and float finiteness; append one output item. Stop at the first failure. Complete the whole sequence before starting its consumer.
5. The consumer sees the output owner and dense 1-based output positions. Its existing filter/reducer/index argument order and lazy behavior are unchanged, after eager sequence completion.

Each stage or nested UDF call first evaluates its entire ordered argument vector, then adapts parameters in declaration order, then evaluates the body and adapts its return. A later argument's `Raise` therefore wins over an earlier argument's adaptation failure. Static call-depth/signature admission precedes runtime. Existing runtime cycle/depth checks occur on call entry after arguments are evaluated. The new stage boundary checks happen after the ordinary return adapter, so an ordinary adapter failure keeps its original cause rather than being replaced by an output-tag diagnostic.

The stage's `source_position` is the pre-filter index. It never changes to the dense output position. A parent `Position` node used as a capture resolves before any stage call and retains its parent value. The stage does not push a source frame into a UDF or make an implicit `Position` available there; its diagnostic envelope records the source index. A downstream `Position` resolves through the existing consumer frame. No capture may depend on either private owner or a foreign private sequence owner.

For source `[1,2,3,4,5]`, parent position `7`, predicate `item > 2` and mapper `item * 10 + source_position + parent_position`, the result is `[40,51,62]`. The stage positions are `[3,4,5]`; the consumer positions are `[1,2,3]`. The linked literal fixtures retain these separately.

False predicates and unselected `If` branches suppress their runtime errors. Eager completion means `exists` cannot accept the first output and hide a later mapper failure. A failed invocation has no successful partial sequence; any already computed diagnostic values may be retained as evidence, not consumed or published as success. Mapping exception message absence and an explicitly empty message remain different.

<a id="limits-and-cancellation-proposal"></a>

## Limits and cancellation

Host or test limits may be lower than the fixed ceilings. A mapping cannot
raise them. Reached descriptors, repeated scopes and primary/named targets
share the same invocation counters; ordinary generators keep their own limits.

| Control | Limit and charge point |
| --- | --- |
| Primitive source length | Existing `Generate` cap of 1,000,000; check before allocation, preserving its original error |
| Cumulative admitted source items | 1,000,000 across the run; reserve the complete range count before allocation; an empty or absent range reserves zero |
| Work | 10,000,000 units across the run; charge one before every reached graph-node evaluation belonging to this descriptor's source arguments, captures or transitive UDF body, one before each UDF call entry, and one per admitted source item reserved before allocation |
| Stage output items | At most the admitted source count; filter/map cannot expand the sequence |
| UDF depth | Existing 64-call limit; static cycles rejected; no resetting depth for nested calls |
| Composition depth | Exactly one; `source` cannot be another transform |

Repeated graph-node references charge on each evaluation; no memoization is implied. The descriptor dispatch, scalar adaptation and budget comparisons themselves add no units. A single range reservation is atomic: primitive item limit, then cumulative item limit, then remaining work check; failure consumes neither reservation. Successful reservation charges both counters, even if a later capture fails. Later node/call charges refuse before evaluation when `used + 1` exceeds the work limit; the refused charge does not change `used`. Limits do not replace a previously returned original evaluation error.

For explicit range `1..=3`, no captures, constant-true predicate and identity mapper, source argument nodes cost 2 and source items cost 3. Each item costs predicate call/body 2 and mapper call/body 2. With work limit 8, the first mapper's parameter-node charge refuses at `used=8`, `requested=1`; the expected fixture retains that exact boundary. Consumer expressions outside the descriptor are not charged to this feature's work counter.

The invocation checks cancellation before each charged node or call, range
reservation and stage item. Cancellation wins over a budget test at the
same boundary. It cannot interrupt a scalar builtin already running or
replace a previously returned error. Post-node debug hooks are separate.

An item count and charged traversal budget are not a total-RAM or hard-time bound. The source and completed derived sequence are materialized, parent strings can be large, UDF arguments can clone strings, and consumer/target/writer lifetimes remain ordinary. Static validation and a single scalar builtin's internal work are outside these counters. No streaming, precise peak-memory attribution or prompt interruption claim follows from these limits.

## Diagnostics and compatibility

Static errors identify the descriptor and offending node or function. Check
source form and ownership, capture declarations, predicate and mapper
signatures, then their complete transitive bodies (predicate first, node-ID
order, callees depth-first). Preserve the call path for cycles and depth
errors. An invalid unused branch is still structurally invalid.

Runtime comparisons distinguish source, capture, predicate and mapper, and
retain owner, applicable capture index or original position, expected/actual
tags or nonfinite bits, and the original cause. A failed charge retains
limit, amount used and request; cancellation identifies its boundary. A
missing exception message differs from an explicitly empty one.

The new tag rejects unknown fields without tightening old descriptors.
Readers that do not know the tag refuse it. Lowering must retain both stage
functions and their transitive callees even if no main-graph call uses them.

<a id="literal-qualification-and-follow-on-ownership"></a>

## Fixtures and interchange

The [37 design recipes and two legacy controls](fixtures/scalar-filter-map-v1.json)
are literal expectations, not serialized Projects. Compare the [engine tests](../../crates/engine/src/tests/filter_map.rs),
[Rust tests](../../crates/codegen-rust/src/tests/filter_map.rs) and
[C# host tests](../../crates/cli/tests/code_generation/filter_map_csharp.rs)
for the stages each exercises.
