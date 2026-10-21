# Owned join tuples

An owned inner join (`ScopeIteration::OwnedInnerJoin`) joins sources named by
an exact owner and endpoint-relative collection, beside the unchanged legacy
join. Its plan, identity checks and wire form are described in
[source ownership](source-ownership.md#owned-join-identity). This document
covers execution: tuple members, positions and the tuple reads.

## Wire form

```json
{"kind":"owned_join_field","join":7,
 "collection":{"owner":{"kind":"named","name":"Lookup"},"path":["Rows"]},
 "path":["code"]}
{"kind":"owned_join_source_position","join":7,
 "collection":{"owner":{"kind":"primary"},"path":["Rows"]}}
```

`owned_join_field` reads a scalar relative to one tuple member and
`owned_join_source_position` returns that member's raw source position. Both
name the member by its full owner and collection identity, so a Primary `Rows`
and a named `Rows` are two members. Both payloads are strict: unknown keys,
legacy collection arrays and empty named owners are refused, and readers that
predate these kinds refuse them. The flattened tuple position reuses the
existing `join_position` node: it names only the `JoinId`, which is independent
of owner identity, so an owned counterpart would carry nothing new.

Validation binds every read to the owned join that dominates it. A read of an
inactive join, of a legacy join, or of an identity that is not one of the
join's sources is a typed error; a repeating member's field must be a
nonrepeating scalar below its collection, and a singleton member is read with
an empty path. A legacy `join_field` never reads an owned join.

## Members and tuples

Each source contributes members only from its declared endpoint, selected like
an owned walk:

- The latest active frame of the same owner whose collection prefixes the
  source anchors it. Inside an owned Primary `Orders` row, a Primary `Orders`
  source is that one active order and `Orders/Lines` walks only its lines.
  Without such a frame the walk starts at the endpoint root and flattens every
  repetition it crosses. Frames of other owners never anchor it.
- An absent, empty or null collection yields no members, and another endpoint
  with the same path never supplies any. An unsupplied owner is a typed
  `SourceUnavailable` failure of the join scope.
- A singleton source is one owned scalar read in the receiving context.

Every source of a stage is selected before its first comparison. Stages are
left-deep: tuples keep left-major order and duplicate multiplicity. A
condition compares the owned read of its left member with the owned read of
the new member; absent, Null, JSON-null and XML-nil keys never match, and
other values use the ordinary `equal` coercion. The tuple count is bounded by
the owned walk-context limit.

## Positions

A member's raw source position is its position within its own repetition, or
the active occurrence's current position when it is that occurrence; a
singleton and an endpoint-root identity are position one. Raw positions never
change. The flattened tuple position starts as the tuple's ordinal and follows
the legacy join phases: a filter before sorting and every sort key see the
original position, sorting renumbers it, a filter after sorting sees the
sorted position, window bounds evaluate once in the receiving parent, and
bindings and static descendants see the compacted output position.

## Tuple context

Filters, sort keys, bindings, scalar constructions and static descendant
groups evaluate in the tuple. They read the tuple only through owned join reads
and `join_position`, plus pure scalar nodes (constants, calls, conditionals,
value maps, runtime values and parameters, raise). Ordinary and owned source
reads, searches, aggregates, sequences, user functions and joined aggregates
stay closed with the typed `Join` capability, as do iterating or specialized
descendants. Grouping an owned join is a validation error. Owned joined
aggregates have no representation yet and remain closed.

## Generation profile

`codegen::lower_with_basic_owned_joins` admits owned joins with the owned
Lookup and CollectionFind profile; the owned dynamic source-field companion,
built on the same profile, the owned XML serialization companion built on it
and the owned collection-aggregate companion stay separate siblings, and their
reads stay closed here. Every earlier
lowering route refuses an owned join at its preflight, and every earlier validator, named-input planner and
emitter, including the ordinary ones and directly constructed Program
validation, refuses it with the `Join` capability. Owned join sources keep
their endpoints optional in the named-input catalogue.

Generated Rust calls `BasicScopeContext::owned_inner_join`, which walks each
source with `walk_owned_source_cohort`; generated C# calls
`FerruleBasicScopeContext.OwnedInnerJoin`. Both read members with their owned
field resolvers and renumber tuples with `with_owned_join_position` /
`WithOwnedJoinPosition`. `.mfd` export and GUI authoring stay closed; the GUI
shows saved owned join reads read-only.

## Qualification

The [portable corpus](../../conformance/basic-owned-joins/README.md) records
complete expected outputs and typed failures for same-path sources, duplicate
and null keys, multi-stage and singleton joins, controls, correlation with an
active owned row, empty sides and refusals. The CLI `code_generation` tests run
every call through the interpreter; the ignored host test builds generated Rust
and C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_joins449_generated_rust_and_csharp_match_expected -- --ignored
```
