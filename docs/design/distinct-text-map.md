# Stable distinct-key text mapping

`Node::DistinctTextMapLast` evaluates an ordinary source collection in two
phases. First it reads the item-relative `key` path for every candidate and
keeps the first item for each exact String key. Then it evaluates `expression`
in those retained item contexts, in first-seen order, and returns the last
present String result.

For keys `A, B, A`, the expression runs twice: on the first A and on B. It does
not run on the last A. This differs from mapping every item and then reducing
duplicate results. In particular, clock calls, counters and failures in the
skipped duplicate do not occur.

## Value and context rules

- A missing key or the `Null` absence sentinel contributes no candidate.
- Empty String and whitespace keys remain present and distinct. Equality is
  ordinal and case-sensitive; no Unicode normalization is applied.
- JSON null, numbers, booleans and structured key values are not String keys.
- The mapped expression receives the first item's source frames, physical
  source identity and original position. It does not receive a synthetic group
  containing all equal-key items.
- A mapped `Null` is absent and does not replace an earlier result. An empty
  String does replace it. Other mapped value kinds are refused.
- Empty selection, or only absent mapped values, produces `Null`.

All keys are selected before any mapped expression runs. A late invalid key
therefore precedes even an earlier candidate's mapping failure. Once mapping
starts, a failure stops later mapped expressions; prior effects are not rolled
back.

The key table is limited to one million distinct keys and 64 MiB of retained
UTF-8 key text. Duplicates do not consume another entry or text budget. A refused
insertion leaves the previous table unchanged. Rust shares this membership
policy between the interpreter and generated runtime.

## Admission and persistence

The wire kind is `distinct_text_map_last`, with `collection`, `key` and
`expression` fields. The expression is one graph dependency and remains live
during pruning. The canvas presents one mapped-value input and editable
collection/key paths; rewiring does not change those paths.

The interpreter and both generated languages support the ordinary source
profile. Existing owned and isolated-function profiles do not inherit this
operation. Native MFD export refuses it, including an unused stored occurrence,
until a complete export translation is qualified. This node does not itself
grant an MFD import profile or a filesystem capability.

When used in a document filename, normal visitor ordering still completes the
body before evaluating the filename expression. The explicit filename clock
then runs once for each retained key that reaches a clock node, rather than once
after the final key has been selected.
