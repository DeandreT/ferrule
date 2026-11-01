# Owned collection aggregates

`owned_aggregate` reduces one collection of exactly one explicitly owned
endpoint to one scalar. It sits beside the unchanged legacy `aggregate` node,
whose wire, outward collection fallback and admission do not change.
Generated-sequence reductions (`sequence_aggregate`) and joined-tuple
reductions keep their own contracts.

## Wire form

```json
{"kind":"owned_aggregate","function":"join",
 "collection":{"owner":{"kind":"named","name":"Rates"},"path":["Rates","Rate"]},
 "value":["Code"],"arg":4}
{"kind":"owned_aggregate","function":"sum",
 "collection":{"owner":{"kind":"primary"},"path":["Rows"]},"expression":5}
```

`function` is one of the legacy reducers: `count`, `sum`, `avg`, `min`, `max`,
`join` and `item_at`. `collection` is an
[`OwnedSourceRef`](source-ownership.md#ownership-rules). Each item's value is
either `value`, a path relative to the selected item (the empty path reads a
scalar item itself), or `expression`, a graph node evaluated in the item's
context. Exactly one of the two is present. `arg` is the optional parent-context
argument: the `join` separator or the 1-based `item_at` index. The payload is
strict: unknown keys, a missing or legacy-array `collection`, both or neither
value forms and empty named owners are refused. Readers that predate the kind
refuse it rather than reinterpreting it.

Validation requires a declared static owner and a collection path that names a
repeating collection in that owner, or the empty path for the endpoint root. A
value path must name a scalar inside the item without crossing another
repetition. Count never reads its value path, so its empty path also counts
group items. A dynamic named owner is unavailable before target evaluation and
is refused.

## Evaluation

The collection is qualified once, with the walk rules of
[owned search](owned-lookup-find.md#selection-and-items):

- The latest active frame of the same owner whose collection prefixes the path
  anchors the walk. Inside an owned Primary `Rows` row, a Primary `Rows/Lines`
  aggregate sees only that row's lines and a Primary `Rows` aggregate sees that
  row; inside an owned group or fixed block, an aggregate of the grouped
  collection sees only its members. Without such a frame the walk starts at the
  endpoint root, flattening every repetition it crosses. Frames of other owners
  never anchor it.
- An absent, empty or null selected collection yields no items. Another
  endpoint with the same path never supplies items. An unsupplied owner is a
  typed `SourceUnavailable` failure raised before any item value or argument.
- A repeated root yields its items; a Group root with the empty path is one
  identity item.

After the complete walk is admitted, item values are produced in source order. A
value path is an owned read of the item; an absent value is `Null`. An
expression sees the item as the latest occurrence of its owner: owned fields
and positions read it, owned reads of any other static endpoint or pinned parent
frame keep their ordinary meaning, and legacy fields resolve from the item
outward, as inside an owned scope. `arg` then evaluates once in the parent
context, also when no item exists; item value failures precede argument
failures.

The reducers are the legacy ones. Count counts items, not non-null values. An
empty collection reduces to count 0, sum 0 and Null for avg, min, max and
item_at, and join yields the empty string. Numeric strings parse, other values
are skipped by numeric reducers, join omits Null values, and integer overflow
and non-finite results are the typed aggregate failures.

Owned aggregates are Basic scalar expressions. User functions, owned and legacy
failure predicates and messages, legacy aggregate, lookup and find item
expressions, generated-item contexts and the other specialized contexts keep
them closed.

## Native editor

The **from one input** actions offer **Count**, **Sum**, **Average**, **Minimum**
and **Maximum** on primary and named-target canvases. These create an explicit
static owner and no scalar argument. Count stores an empty value path
and offers collections even when their group items have no scalar fields. The
numeric reducers offer only scalar item paths that do not cross another
repetition. Input and collection changes retain a valid local value path or
select the first field of that exact owner; they never use another owner's
fields.

The properties editor switches between these operations and selects the
input, collection and (except for Count) value path. Legacy aggregates stay
legacy until **Use an exact input** is clicked. Computed values, arguments,
other operations and nonempty Count value paths remain read-only. Merely
inspecting missing owners or invalid saved paths never repairs them.

All five forms share the Lookup owned-expression admission gate across
conversion, wires, Inspector bindings, controls, defaults and indirect dependencies. Grouped
subtrees and descendants, generated contexts, isolated functions and dynamic or
document-set inputs are not authorable. Later grouping changes cannot move an
existing owned expression into a closed context. Locks, typed history and the
layout sidecar apply as for other graph edits.

These are ordinary numeric reducers, not decimal arithmetic: Decimal operands
produce the existing unsupported-scalar error; integer overflow and non-finite
results keep their existing typed failures. This GUI surface makes no new
generation or external-runtime claim.

Average returns a float; Minimum and Maximum retain the selected integer or
float value. Numeric strings participate, while missing and nonnumeric values
are skipped. Empty collections or collections without numeric values yield Null
for Average, Minimum and Maximum, rather than Sum's zero. Operation changes
retain valid owner-local selections and do not convert saved legacy nodes merely
by opening their properties.

## Generation profile

`codegen::lower_with_basic_owned_collection_aggregates` admits owned aggregates
together with the complete [owned search profile](owned-lookup-find.md#generation-profile):
owned Lookup and CollectionFind, the owned recursive-filter companion chain and
owned IntoBlocks grouping. Owned aggregates are reductions over the same owned
item walk, so the companion extends owned search rather than an earlier
profile.

Every other profile, including the owned search companion, the [dynamic-field
companion](owned-dynamic-field.md) and XML-serialization companion built beside
it and the generated-sequence aggregate companion, refuses owned aggregates with
its typed `AggregateLookup` diagnostic, and the ordinary emitters, directly
constructed Program validation and `.mfd` export keep refusing
them. Joins, documents,
dynamic sources, the legacy aggregate and specialized controls remain closed in
the Basic profiles, and SequenceExists, owned dynamic source fields and owned XML
serializers stay closed in this profile. An owned aggregate does not make its
endpoint an eager named input.

Both generated runtimes reuse their existing owned cohort walk, owned field
reads and legacy reducers: Rust `walk_owned_source_cohort`, `owned_field` and
`aggregate`, C# `WalkOwnedSourceCohort`, `ResolveOwnedField` and
`FerruleAggregates.Apply`. A walk or item-read failure carries the aggregate
node; nested item expression and argument failures keep their own nodes.

## Qualification

The [portable corpus](../../conformance/basic-owned-collection-aggregates/README.md)
pairs same-path Primary and named rows with disjoint values and records complete
expected outputs and typed failures. The CLI `code_generation` tests run every
call through the interpreter; the ignored host test builds generated Rust and
C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_aggregate447_generated_rust_and_csharp_match_expected -- --ignored
```
