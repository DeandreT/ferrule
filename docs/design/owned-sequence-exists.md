# Owned sequence Exists predicates

The explicit [sequence Exists generation profile](../code-generation.md#explicit-owned-sequence-exists)
admits endpoint-owned reads in the scalar producer arguments and predicate of
`SequenceExists` over four producers: Generate, Tokenize, TokenizeByLength and
TokenizeRegex. It composes the parent-context ItemAt permission, the By,
AdjacentBy, StartingWith and EndingWith grouping controls, filter/sort/window
controls and their primary-row-failure permission. The interpreter admits the same consumer
in Basic owned contexts.

## Evaluation context and order

Structural validation, owner declaration checks, target selection and named-input
admission precede evaluation. At each reached consumer:

1. Evaluate the producer's ordered scalar arguments in the receiving context and
   materialize the complete sequence using its existing helper and limits.
2. Visit the items in source order. Each item evaluates the predicate in a
   context that appends one private scalar item frame to the receiving context.
3. Return true at the first true predicate. Later items, including predicates
   that would fail, are not evaluated. An empty sequence returns false without
   evaluating the predicate.

A producer error prevents every predicate evaluation. Null or JsonNull producer
arguments finish production with an empty sequence and suppress later producer
arguments. A predicate that is not Boolean fails with a typed error naming the
predicate node.

## Item and endpoint identity

The generated item is lexical sequence metadata, not an endpoint occurrence. In
the predicate, the item declaration and empty-path ordinary reads resolve the
item, and an empty legacy Position resolves its one-based item position. Other
ordinary reads fall back outward exactly as they do in ordinary generated-item
contexts.

Owned field and position reads ignore the item frame. They resolve against the
receiving context's exact owner, root and collection pins, group representative
or member and source positions. A named owner never borrows a colliding primary
field, and an unavailable selected owner fails only when a reached predicate
reads it. Nested eligible Exists and ItemAt consumers in a predicate keep this
rule; each Exists predicate sees only its own item.

## Admission boundaries

The consumer permission is orthogonal to target-control and failure policies.
Only the explicit profile admits it; older public generation wrappers keep their
closed consumer policy. Legacy and owned RecursiveCollect, FilterMap and other producers stay
closed. A predicate containing a closed operator, such as a collection
aggregate, lookup or user function call, does not gain permission from an owned
read beside it; a SequenceAggregate qualifies only under the
[aggregate profile](owned-sequence-aggregate.md). Owned failure predicates and messages retain their
separate scalar eligibility, and generated-sequence scopes stay closed.

Named admission visits producer arguments in the receiving environment and the
predicate in its generated-item environment. Owned-only names remain optional
until a reached read; potential legacy reads retain their required catalogue
entries.

## Generated runtimes

The Rust and C# Basic contexts copy the receiving paired view once per reached
item and append the item as an empty-path legacy collection frame. Each copy is
admitted against the caller's five-field owned-source policy when it is created,
so an existential consumer stops before later items allocate view metadata.
Owned frames and origins are copied unchanged. The pure producers reuse their
established runtime helpers.

The [portable corpus](../../conformance/basic-owned-sequence-exists/README.md)
defines genuine Projects, complete typed outcomes and closed-profile controls.
Native export, GUI authoring and observer eligibility require their own
contracts.
