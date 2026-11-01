# Owned Lookup and CollectionFind

`owned_lookup` and `owned_collection_find` search one collection of exactly one
explicitly owned endpoint. They sit beside the unchanged legacy `lookup` and
`collection_find` nodes, whose wire, outward collection fallback and admission
do not change.

## Wire form

```json
{"kind":"owned_lookup",
 "collection":{"owner":{"kind":"named","name":"Rates"},"path":["Rates","Rate"]},
 "key":["Code"],"matches":3,"value":["Value"]}
{"kind":"owned_collection_find",
 "collection":{"owner":{"kind":"primary"},"path":["Rows"]},"predicate":4,"value":5}
```

`collection` is an [`OwnedSourceRef`](source-ownership.md#ownership-rules): a
Primary or exact named owner and its endpoint-relative path. Lookup `key` and
`value` are paths relative to each selected item. Both payloads are strict:
unknown keys, a missing or legacy-array `collection`, a missing node reference
and empty named owners are refused. Readers that predate these kinds refuse
them rather than reinterpreting them.

Validation requires a declared static owner and a collection path that names a
repeating collection in that owner, or the empty path for the endpoint root.
Lookup keys and values must name scalars inside the item without crossing
another repetition. A dynamic named owner is unavailable before target
evaluation and is refused.

## Selection and items

A lookup evaluates `matches` once in the receiving parent context before the
walk. Both searches then walk the collection with the owned iteration rules:

- The latest active frame of the same owner whose collection prefixes the path
  anchors the walk. Inside an owned Primary `Rows` row, a Primary
  `Rows/Lines` search sees only that row's lines; inside an owned group or
  fixed block, a search of the grouped collection sees only its members.
  Without such a frame the walk starts at the endpoint root, flattening every
  repetition it crosses. Frames of other owners never anchor it.
- An absent, empty or null selected collection yields no items. Another
  endpoint with the same path never supplies items, so a search misses even
  when the other endpoint would match. An unsupplied owner is a typed
  `SourceUnavailable` failure raised after the needle.
- A repeated root yields its items; a Group root with the empty path is one
  identity item.

The complete walk is admitted before the first key read or predicate. Items are
then scanned in source order and the first match wins; a miss is `Null`.

A lookup key matches when its owned read of the item equals the needle and is
not Null-like, so absent, Null, JSON-null and XML-nil keys never match. The
value is the owned read of the matching item; an absent value is `Null`.

A find evaluates `predicate` for each item until one returns `true`, then
returns `value` for that item. `Null`, JSON null and XML nil skip an item; any
other non-Boolean is the typed Boolean error at the predicate node. Predicates
and values see the item as the latest occurrence of its owner: owned fields and
positions read it, owned reads of any other static endpoint or pinned parent
frame keep their ordinary meaning, and legacy fields resolve from the item
outward, as inside an owned scope.

Owned searches are Basic scalar expressions. User functions, owned and legacy
failure predicates and messages, legacy aggregate, lookup and find item
expressions, generated-item contexts and the other specialized contexts keep
them closed.

## Generation profile

`codegen::lower_with_basic_owned_lookup_find` admits owned Lookup and
CollectionFind together with the owned recursive-filter companion, which
composes owned recursive filters, ItemAt over owned recursive collection walks,
owned adjacency-tree rows and path-hierarchy input collections and the
[owned failure-source profile](owned-failure-sources.md), and with owned
IntoBlocks grouping: By/AdjacentBy/StartingWith/EndingWith/IntoBlocks target
controls, scalar-producer and owned recursive ItemAt, primary-row failures and
owned failure walks.

Every earlier profile, including the path-hierarchy, adjacency-tree, IntoBlocks,
recursive-collect, SequenceExists and recursive-filter companions, refuses owned
search with its typed `AggregateLookup` diagnostic, and the ordinary emitters,
directly constructed Program validation and `.mfd` export keep refusing it.
The native GUI authors bounded owner-qualified Lookup nodes for Primary and
addressable static named inputs; CollectionFind and unsupported contexts remain
read-only. Opening properties does not convert legacy searches or repair saved
owner/path metadata. Joins and legacy
Lookup or CollectionFind remain closed in the Basic profiles, and owned
aggregates stay closed in this profile; the
[owned aggregate companion](owned-collection-aggregates.md) extends it.
An owned search does not make its endpoint an eager named input. SequenceExists
stays closed in this profile; the separate SequenceExists companion admits it.

Both generated runtimes reuse their existing owned cohort walk and owned field
reads: Rust `walk_owned_source_cohort` and `owned_field`, C#
`WalkOwnedSourceCohort` and `ResolveOwnedField`. A walk or item-read failure
carries the search node; nested needle, predicate and value failures keep their
own nodes.

## Qualification

The [portable corpus](../../conformance/basic-owned-lookup-find/README.md)
pairs same-path Primary and named rows with disjoint codes and records complete
expected outputs and typed failures. The CLI `code_generation` tests run every
call through the interpreter; the ignored host test builds generated Rust and
C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_search446_generated_rust_and_csharp_match_expected -- --ignored
```
