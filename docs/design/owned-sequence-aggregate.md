# Owned sequence aggregates

The explicit [sequence aggregate generation profile](../code-generation.md#explicit-owned-sequence-aggregate)
admits endpoint-owned reads in the producer arguments, predicate, value
expression and argument of `SequenceAggregate` over four producers: Generate,
Tokenize, TokenizeByLength and TokenizeRegex. It composes the SequenceExists
profile: the ItemAt and Exists consumers, the By, AdjacentBy, StartingWith and
EndingWith grouping controls, filter/sort/window controls and their
primary-row-failure permission. The interpreter admits the same consumer in
Basic owned contexts.

## Evaluation context and order

Structural validation, owner declaration checks, target selection and named-input
admission precede evaluation. At each reached consumer:

1. Evaluate the producer's ordered scalar arguments in the receiving context and
   materialize the complete sequence using its existing helper and limits.
2. Visit the items in source order. Each item evaluates the predicate, when
   present, in a context that appends one private scalar item frame to the
   receiving context. A non-Boolean predicate fails with a typed error naming
   the predicate node.
3. For each selected item, evaluate the value expression in that same item
   context, or use the item itself when there is no expression. Rejected items
   never evaluate the expression.
4. After every item, evaluate the argument once in the receiving context, even
   when the sequence is empty or every item was rejected.
5. Reduce the selected values with the existing count, sum, avg, min, max, join
   or item_at rules.

A producer error prevents every later phase, an item-phase error prevents later
items and the argument, and Null or JsonNull producer arguments finish
production with an empty sequence. An empty reduction gives count 0, sum 0, an
empty join and Null for avg, min, max and item_at. Numeric reducers skip values
that are not numbers, keep exact integer results, retain the integer or float
tag of a mixed min or max, and fail with typed integer-overflow or non-finite
errors. Join uses the argument as its separator and item_at as its one-based
index.

## Item and endpoint identity

The generated item is lexical sequence metadata, not an endpoint occurrence. In
the predicate and expression, the item declaration and empty-path ordinary reads
resolve the item, and an empty legacy Position resolves its raw one-based
position in the produced sequence; rejected items keep their positions. Other
ordinary reads fall back outward exactly as they do in ordinary generated-item
contexts. The argument does not see the item.

Owned field and position reads ignore the item frame. They resolve against the
receiving context's exact owner, root and collection pins, group representative
or member and source positions. A named owner never borrows a colliding primary
field, and an unavailable selected owner fails only when a reached phase reads
it. Nested eligible aggregates, Exists and ItemAt consumers keep this rule; each
item phase sees only its own item.

## Admission boundaries

The consumer permission is orthogonal to target-control and failure policies.
Only the explicit profile admits it; other public generation wrappers, including
the SequenceExists, recursive-filter, owned search and owned dynamic-field
profiles, keep their closed consumer policy. Legacy and owned
RecursiveCollect, FilterMap and other producers stay closed, as do collection
aggregates over source paths ([#447](https://github.com/DeandreT/ferrule/issues/447))
and joined aggregates ([#450](https://github.com/DeandreT/ferrule/issues/450)).
A phase containing a closed operator, such as a collection aggregate, lookup or
user function call, does not gain permission from an owned read in another
phase. Owned failure predicates and messages retain their separate scalar
eligibility, and generated-sequence scopes stay closed.

Named admission visits producer arguments and the argument in the receiving
environment and the predicate and expression in the generated-item environment.
Owned-only names remain optional until a reached read; potential legacy reads
retain their required catalogue entries.

## Generated runtimes

The Rust and C# Basic contexts copy the receiving paired view once per item and
append the item as an empty-path legacy collection frame, reusing the
generated-item contexts of the [Exists contract](owned-sequence-exists.md). Each
copy is admitted against the caller's five-field owned-source policy when it is
created. Owned frames and origins are copied unchanged. The pure producers and
reducers reuse their established runtime helpers.

The [portable corpus](../../conformance/basic-owned-sequence-aggregate/README.md)
defines genuine Projects, complete typed outcomes and closed-profile controls.
Native export, GUI authoring and observer eligibility require their own
contracts.
