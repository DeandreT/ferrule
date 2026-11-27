# Computed scalar keys for joins

A computed join input contributes the value of a scalar expression, or the
ordered tokens of an admitted sequence, instead of reading a collection port.
Its key is evaluated in the receiving context. The reference observations
below concern only the self-authored designs in the [fixture set](#frozen-fixtures).

## Current behavior

The [mapping model](../../crates/mapping/src/iteration.rs) distinguishes
collection and computed sources. The [engine](../../crates/engine/src/join.rs)
evaluates the computed key once before selecting any collection, even when
another side is empty. Collections retain their correlation with an exact
active frame; the computed member adds no frame. Generated Rust/C# admit
scalar computed keys in eligible join scopes and joined aggregates, but
refuse sequence-valued keys. Owned join sources remain path-only.

## Native semantics

These are black-box observations on the [authored designs](fixtures/join-computed-keys/).
Valid designs in the observed cohort were saved, reopened, saved again and
executed with the built-in language. `unlisted-function` was not run.

### Wire form

- A join input fed by a function output or a constant takes an ordinary
  connection. The application names its input entry `value`, whatever name the
  design gives it. A copy-all connection there fails validation: "cannot be
  connected via a copy-all connection".
- A join input fed directly by a source scalar, or by a runtime input
  parameter, takes a copy-all connection, like a repeating input. An ordinary
  connection there fails validation: "cannot be connected via a target driven
  connection". This also applies to the existing singleton form, so a
  two-input singleton design whose singleton input has an ordinary connection
  does not validate. A parameter-fed input entry takes the parameter's name.
- A scalar input is keyed by the root entry of `keypaths` (an empty key path),
  in both forms. Saving drops the `input-index` attributes of `keypair` keys
  and an unconnected tuple `outkey`.

### Evaluation

- **Per enclosing item.** Inside a target iteration over `Order`, a key
  computed from the current order's field is evaluated for each order:
  `uppercase(CustomerNumber)` gives `c1` → `Ada` for one order and `c2` →
  `Grace`, `Hopper` for the next.
- **Once at the root.** In a root-context join (the joined tuples drive a root
  target collection), a key computed from a non-repeating root field, a
  constant or a parameter is one value for the whole run.
- **Several values.** A sequence-valued key contributes one member per value,
  in sequence order. `tokenize("C2 C1 C2", " ")` gives the tuples of `C2`, then
  `C1`, then `C2` again. A key computed from a repeating field outside any
  iteration of it is the sequence of that field over every item. At the root,
  `uppercase(Order/CustomerNumber)` contributes one value per order that has a
  value.
- **Constants and parameters** give every enclosing item the same tuples. The
  application executes a runtime parameter with its preview value.

### Values

- **Empty sequence.** When the key's source field is absent, the key is empty
  and the join yields no tuples. This holds for `uppercase`, `lowercase`,
  `concat` (even with a constant second argument), `add` and `tokenize`.
- **Nil.** An `xsi:nil` source field yields no tuples, whether it feeds the
  join directly or through `uppercase`, `lowercase` or `concat`.
- **Empty string.** An empty, non-nil field is the value `""`, and it matches
  a right key that is `""`. This holds directly and through `uppercase`,
  `lowercase` and `concat`. `tokenize` of `""` gives one empty token, which
  matches `""` too.
- **Type mismatch.** A key compares with the other side by canonical text
  whenever one side is a string, for direct and computed keys alike, and a
  mismatch is never an error:
  - An integer key `7`, or the decimal result of `add(7, 0)`, matches the
    string `"7"` but not `"07"` or `"7.0"`. The lexical integer `07` is the
    value 7.
  - A string key `"7"` matches an integer right key `7`. The strings `"07"`,
    `"7.0"`, `" 7"`, `"+7"`, `"C1"` and `""` match no integer, and the run
    succeeds.

### Order and multiplicity

Tuples are left-major by input index, with duplicates on both sides. With
the key as input 0 and `Customer` as input 1, `tokenize("C2 C1 C2")` gives
`Grace, Hopper, Ada, Grace, Hopper`. With `Customer` as input 0 and the key as
input 1, it gives `Ada, Grace, Grace, Hopper, Hopper`, in customer order.
For equal values, a computed single-valued key yields exactly the tuples of the
existing singleton form: `C1` gives `Ada` either way, and `""` gives `Blank`.

### Unobserved

The following were not observed. Each is refused below rather than inferred:

- the result of a key computation that raises an error;
- reading the computed key back from the joined tuple. The
  [`key-read`](fixtures/join-computed-keys/designs/key-read.mfd) design feeds
  `Match/Number` from an output on the computed input, and it fails
  validation with "The component join has missing items". No accepted form
  was found;
- two computed inputs in one join, computed inputs in joins of three or more
  inputs, and computed inputs feeding a joined aggregate rather than a target
  iteration;
- computed keys over named secondary or dynamic sources;
- functions other than `uppercase`, `lowercase`, `concat`, `add` and
  `tokenize`. The
  [`unlisted-function`](fixtures/join-computed-keys/designs/unlisted-function.mfd)
  design uses `normalize-space` and was not run.

## Contract

### Computed source

In the scalar form, one graph node supplies the key. The bounded sequence
form contributes one member per token in order, retaining duplicates and empty
strings. An absent or nil input gives no tokens.

- **Context.** The node is evaluated in the join's receiving context: for a
  join scope, the context of its parent scope; for a joined aggregate, the
  context of the aggregate's consumer. That is once per enclosing item for a
  join nested in an iteration, and once per run for a root-context join.
- **Timing.** The node is evaluated once per join execution, with the other
  sources and before the first comparison, even when another source is empty.
  A node error is an ordinary node error and fails the join's scope or
  aggregate. Native error behavior is unobserved, so the MFD import gate
  admits only functions whose native results were observed.
- **Members.** `Null`, `JsonNull` and `XmlNil` give no member, and the plan
  then yields no tuples. Any other value, including `""`, gives one member at
  raw position 1.
- **Equality.** Conditions keep the current rules: Null-like keys never match,
  and other values use `equal`. The computed member's key is its value; its key
  path is always empty. `Decimal` keys stay a typed failure, as today.
- **Order.** The computed source occupies its place in the left-deep plan. As
  the first source, it is the outer loop of the first stage. As a stage's right
  source, every left tuple pairs with it when the condition holds. Tuple order
  and duplicate multiplicity follow from the existing stage rules.
- **Reads.** A computed member has no collection, so no `JoinField` names it.
  `JoinPosition` keeps counting tuples. A computed source never correlates with
  an active frame.
- **Admitted nodes.** The node, and everything it depends on, must be
  admissible in the receiving context today: source fields, constants, calls,
  conditionals, value maps, runtime values and runtime parameters. Validation
  refuses any source read, at any depth, whose path crosses a repeating level
  not active in the receiving context, using the existing singleton rule.
  Ferrule reads the first item there, while the native key would be the whole
  sequence. Aggregates, lookups, sequences, user functions, joined aggregates
  and reads of any join's tuple are refused in the key.
- **Bounds.** A plan holds at most one computed source and at least one
  collection source.

### Model

Collection sources keep their path and optional singleton cardinality.
A computed source has a plan-local ID independent of its expression node;
it serializes as `{"computed":{"id":1,"value":12}}` or with `sequence`
instead of `value`. No plan can have two computed sources, a duplicate ID
or no collection source. A computed left condition uses `left_computed`
instead of `left_collection` and `left_path`; its ID must refer to a prior
source. Every condition on a computed right source has an empty `right_path`.

Saved projects require `join-computed-keys-v1`, and a sequence-valued key
additionally requires `join-computed-sequence-keys-v1`. See the [file
capabilities](../project-files.md#file-capabilities). Older readers refuse
the unknown form rather than treating the key as a collection.

### Refusals

These stay refused, each with a specific reason:

| Case | Route | Reason |
| --- | --- | --- |
| Two computed inputs in one join | model, import | unobserved; bounded to one |
| A computed input in a join of three or more inputs | import | unobserved |
| A computed input feeding a joined aggregate | import | unobserved (the model and engine admit it) |
| Reading the computed key from the tuple (`key-read`) | model, import | fails native validation; a computed member has no readable collection |
| A key node reading across an inactive repeating level (`root-repeating`) | model, import | the native key is a sequence |
| A sequence-valued key other than `tokenize`, a `tokenize` delimiter that is not a non-empty string constant, or tokens of a non-string | engine (other sequences), import, export | unobserved |
| A key over a named secondary or dynamic source | import | unobserved |
| A key with a function outside the admitted list (`unlisted-function`) | import | unobserved empty-input behavior |
| A computed input with a copy-all connection (`copy-all-computed`) | import | the native application refuses it |
| A computed key on an owned join | model | owned plans stay path-only |
| A `Decimal` key value | engine | `equal` refuses decimals, as today |

<a id="follow-up-scopes-and-route-gates"></a>

**MFD import and export.** In a two-input target iteration one input is a
repeating collection. The other may be a constant, parameter or a bounded
`uppercase`, `lowercase`, `concat` or `add` chain, or a directly connected
`tokenize` with a nonempty constant delimiter. Both input orders are
meaningful. For a missing or nil field, import guards the computation so that
`concat` cannot turn absence into `""`. Export writes the computation below
the guard: the native functions already produce no key. A constant or
function output uses an ordinary connection to `value`; a parameter uses a
copy-all connection to its named input. Unsafe operand types, shared or
inexact guards and topologies that would re-import differently remain
refused; see [MFD interop](../mfd-interop.md).

## Frozen fixtures

The [expected tuples](fixtures/join-computed-keys/expected.json) identify
each design, input, oracle and route. The [primary input](fixtures/join-computed-keys/input.xml)
separates absent, nil and empty fields, and includes two distinct `C2`
customers. The [numeric-text input](fixtures/join-computed-keys/input-numeric-text.xml)
distinguishes `7` from `07`, `7.0`, `+7` and ` 7`. The literal examples above
show why a scalar singleton, a token stream and a repeating-root key cannot
be substituted for one another. The reference accepted `root-repeating`;
Ferrule refuses it because the read crosses an inactive repetition.

<a id="acceptance-plan"></a>

[Interpreter cases](../../crates/engine/src/tests/computed_join.rs),
[generated scalar hosts](../../crates/cli/tests/code_generation/computed_joins.rs),
[MFD import](../../crates/mfd/tests/join_import/computed_keys.rs) and
[MFD export](../../crates/mfd/tests/join_export/computed_keys.rs) check their
respective boundaries against the same fixtures.
