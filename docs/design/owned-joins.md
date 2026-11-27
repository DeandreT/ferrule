# Owned join tuples and joined aggregates

An owned inner join (`ScopeIteration::OwnedInnerJoin`) joins sources named by
an exact owner and endpoint-relative collection, beside the unchanged legacy
join. Its plan, identity checks and wire form are described in
[source ownership](source-ownership.md#owned-join-identity). This document
covers execution: tuple members, positions, the tuple reads and the owned
joined aggregate that reduces a join's tuples.

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
descendants. Grouping an owned join is a validation error.

## Joined aggregates

```json
{"kind":"owned_join_aggregate","function":"join","join":8,
 "plan":{"first":{"collection":{"owner":{"kind":"primary"},"path":["Rows"]}},
   "second":{"source":{"collection":{"owner":{"kind":"named","name":"Lookup"},
     "path":["Rows"]}},"conditions":{"first":{"left_collection":
       {"owner":{"kind":"primary"},"path":["Rows"]},"left_path":["id"],
       "right_path":["id"]}}}},
 "expression":4,"arg":5}
```

`owned_join_aggregate` reduces the tuples of one owned plan with `count`,
`sum`, `avg`, `min`, `max`, `join`, `item_at` or `decimal_sum`.
The legacy `join_aggregate` is unchanged. The payload is strict: an owned plan, a
`JoinId`, an optional tuple `expression` and an optional `arg`; legacy plans,
unknown keys and empty named owners are refused, and readers that predate the
kind refuse it.

The plan executes once in the aggregate's receiving context with the member
rules above, which also define its active-source correlation:

- A source whose owner has an active frame whose collection prefixes the
  source is anchored there. A source naming the active owned row's own
  collection is that row, at its current position; a deeper source walks only
  that row's descendants.
- Inside an owned By, AdjacentBy, StartingWith, EndingWith or IntoBlocks
  group, a source naming the grouped collection walks only the group's
  members.
- Correlation compares owner and absolute collection together. A same-path
  source of another owner is never correlated by the active frame and walks
  its own endpoint from the root; the legacy aggregate, which names
  collections by path alone, correlates by path.
- An unsupplied owner is a typed `SourceUnavailable` failure of the aggregate
  node.

The expression, when present, evaluates once per tuple in left-major order with
only this join's members and flattened tuple position active; it admits the
same closed scalar subset as a join tuple, so source reads, searches,
aggregates, sequences, user functions and nested joined aggregates are refused
with the `Join` capability, and reads of another or an enclosing join are
inactive. Without an expression each tuple contributes Null. The argument then
evaluates once in the receiving context, after every tuple value and even when
there are no tuples, where the aggregate's own tuple reads are inactive.
With no tuples, `count` and `sum` return integer zero, `join` returns the
empty string, and `avg`, `min`, `max` and `item_at` return Null with valid
arguments. `decimal_sum` returns decimal zero. It folds each projected
Decimal before visiting the next tuple; Null is an error, not a skipped term.

A joined aggregate is admitted only in Basic contexts. A join tuple, failure
rules, generated items, legacy item contexts, isolated functions and dynamic
owners keep it closed. Its `JoinId` is independent of the enclosing scopes and
of its source identities.

## Generation profile

`codegen::lower_with_basic_owned_joins` extends the
[owned search profile](owned-lookup-find.md#generation-profile). Both join
scopes and joined aggregates use the same tuple walk and positions
(`BasicScopeContext::owned_inner_join` / `FerruleBasicScopeContext.OwnedInnerJoin`).
Aggregates evaluate the tuple expression there and use the shared reducer.
The [decimal fold](decimal-mapping-operations.md) stops at its first
overflow or non-Decimal operand before visiting a later tuple.

Endpoints remain optional until reached. Ordinary emitters and presets
without `Join` refuse scopes and joined aggregates, including direct Program
validation. Dynamic fields, XML serialization/mixed content and collection
aggregates have separate capabilities. Native MFD export refuses owned joins;
the editor displays saved reads and aggregates read-only.

## Qualification

The [corpus](../../conformance/basic-owned-joins/README.md) covers duplicate
and null keys, singleton/multi-stage joins, active rows and group members,
parent arguments, empty sides and typed failures. Compare compiled calls
separately from interpreter results:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_joins449_generated_rust_and_csharp_match_expected -- --ignored
```
