# Computed scalar keys for joins

Design for [#1236](https://github.com/DeandreT/ferrule/issues/1236). A join
input in a saved design can be fed by a function result, a constant or a
runtime parameter rather than by a source port. This design defines the
smallest exact contract for that *computed key*: one scalar value, evaluated in
the join's receiving context, that takes part in the join as its own member.
It changes no behavior by itself. Each route keeps its current refusal until
its own follow-up scope merges with the matching tests.

## Current behavior

These facts describe the code this design builds on.

- **Import.** `resolve_inputs` in `crates/mfd/src/import/join/mod.rs` accepts
  only a join input whose connected output is a port of a source component.
  That port must be a repeating group or a non-repeating scalar; the scalar
  becomes a singleton source, and its equality path must be empty. A function
  output, constant or runtime parameter feeding an input fails with "input *N*
  is not a plain source feed". The importer then warns "join \`name\` is
  unsupported: …; skipped", and `Registry::reject` marks every tuple and branch
  output of the join as rejected. The target scopes those outputs drive are
  skipped with it. Keyless joins resolve through the same function and refuse
  the same way.
- **Model.** `JoinSource` (`crates/mapping/src/iteration.rs`) is a collection
  path plus `JoinSourceCardinality` (`Repeating` or `Singleton`). `JoinKey`
  names its left member by `left_collection` and `left_path`, and the stage's
  new member by `right_path`. `JoinPlan` is left-deep, has at least two
  sources, and refuses a collection path used twice. `JoinSource` derives
  `Deserialize` without `deny_unknown_fields`, but its `collection` member is
  required. `OwnedJoinSource` (`crates/mapping/src/owned_join.rs`) has the same
  path-only shape, keyed by owner.
- **Engine.** `join::execute` (`crates/engine/src/join.rs`) selects every
  source in the receiving context before its first comparison. A singleton
  source is one scalar read in that context. A source whose exact collection
  is an active receiving frame is correlated to that one row. Stages keep
  left-major order and duplicate multiplicity. `conditions_match` excludes
  absent, `Null`, `JsonNull` and `XmlNil` keys on either side and compares the
  rest with the `equal` builtin. `equal` compares a string with a non-string
  by the other value's canonical text (`functions::scalar::text`, so a float
  `7.0` is `"7"`), compares numbers numerically, and refuses `Decimal` with a
  type mismatch.
- **Validation.** `validate_plan` (`crates/engine/src/validate/join.rs`)
  requires repeating sources to be repeating and singleton sources to be
  non-repeating scalars. It refuses a singleton path that crosses a repeating
  level not active in the receiving context.
- **Generation.** `codegen::JoinSource` (`crates/codegen/src/join.rs`) mirrors
  the path and cardinality, and `crates/codegen/src/validate/joins.rs` checks
  the same invariants for generated Rust and C#.
- **Export.** `crates/mfd/src/export/join.rs` writes kind-32 components whose
  inputs all come from source ports. A nested two-input plan must contain
  exactly one correlated singleton and one repeating source
  (`resolved_nested_source_paths` in `export/join/nested.rs`). Every input,
  repeating or singleton, at every input count, is written as a copy-all
  connection (`dataconnection type="2"`), as the [wire form](#wire-form)
  requires for source feeds.

## Native semantics

The reference application was observed as a black box, using only the
self-authored designs and inputs under
[`fixtures/join-computed-keys`](fixtures/join-computed-keys/). Each design was
validated. Each valid design was then saved, reopened, saved again and executed
with the built-in execution language, and its complete output was recorded. The
[frozen fixtures](#frozen-fixtures) below give every observed tuple.

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

A *computed source* is a join source whose members come from one scalar graph
node instead of a source collection.

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

Follow-up scope 1 adds:

```rust
pub enum JoinSource {
    /// The existing collection source; wire form unchanged.
    Collection(CollectionJoinSource),
    Computed(ComputedJoinSource),
}

pub struct CollectionJoinSource { collection: Vec<String>, cardinality: JoinSourceCardinality }

pub struct ComputedJoinSourceId(u32);

pub struct ComputedJoinSource { id: ComputedJoinSourceId, value: NodeId }

pub enum JoinKeyLeft {
    Collection { collection: Vec<String>, path: Vec<String> },
    Computed(ComputedJoinSourceId),
}
```

- A computed source serializes as `{"computed":{"id":1,"value":12}}`. A
  collection source keeps `{"collection":[…]}`, with `"cardinality":
  "singleton"` when it applies.
- A condition with a computed left member serializes as
  `{"left_computed":1,"right_path":["Number"]}`. The collection form keeps
  `left_collection`, `left_path` and `right_path`.
- When a stage's right source is computed, every condition of that stage must
  have an empty `right_path`.
- The id is local to the plan and independent of `NodeId`, so a later
  sequence-valued source can use the same identity.
- `JoinPlan::new` and `then` gain typed errors for these cases: a second
  computed source, a duplicate id, an unknown `left_computed`, a non-empty
  right path on a computed right source, and a plan with no collection source.
- Wire readers apply the same checks.
- `JoinSource` becomes an enum so that every consumer has to match the new
  variant: each route that does not support it refuses it explicitly instead
  of treating it as a collection.
- The file codec writes a new required capability, `join-computed-keys-v1`,
  whenever a plan holds a computed source. Older readers already fail on the
  missing `collection`, but the capability gives them a clear refusal.

The owned plan (`OwnedJoinPlan`) is not extended. Import never produces owned
joins, and owned plans keep their current path-only sources.

### Refusals

These stay refused, each with a specific reason:

| Case | Route | Reason |
| --- | --- | --- |
| Two computed inputs in one join | model, import | unobserved; bounded to one |
| A computed input in a join of three or more inputs | import | unobserved |
| A computed input feeding a joined aggregate | import | unobserved (the model and engine admit it) |
| Reading the computed key from the tuple (`key-read`) | model, import | fails native validation; a computed member has no readable collection |
| A key node reading across an inactive repeating level (`root-repeating`) | model, import | the native key is a sequence |
| A sequence-valued key (`tokens`, `tokens-second`) | import | single-valued keys only, until the sequence-key scope |
| A key over a named secondary or dynamic source | import | unobserved |
| A key with a function outside the admitted list (`unlisted-function`) | import | unobserved empty-input behavior |
| A computed input with a copy-all connection (`copy-all-computed`) | import | the native application refuses it |
| A computed key on an owned join | model | owned plans stay path-only |
| A `Decimal` key value | engine | `equal` refuses decimals, as today |

The current import refusals stay as they are until scope 4 merges, and so does
the bounded singleton form.

## Follow-up scopes and route gates

Each scope is a separate issue. A route keeps its current refusal until its
own scope merges with the matching tests.

1. **Mapping model.**
   - The `JoinSource`, `ComputedJoinSource` and `JoinKeyLeft` types, the wire
     form, the plan errors and the `join-computed-keys-v1` capability.
   - Fail-closed handling in every consumer: engine validation, codegen
     validation, the named-input planner, trace and debug output, GUI display
     (read-only) and MFD export each refuse a computed source with a typed
     reason. The engine refuses with "computed join sources are not yet
     executable".
   - No import change.
   - *Acceptance:* wire round trips, every plan error, the capability written
     and required, older-reader refusal, unchanged wire bytes for existing
     plans, and a refusal test in every consumer.
2. **Engine.**
   - Execution and validation of single-valued computed sources in
     `InnerJoin` scopes and `JoinAggregate`.
   - Computed members need owned scalar storage that lives for the join
     execution, because `JoinedRow` borrows source instances today.
   - *Acceptance:* hand-built projects equivalent to every computed-key
     fixture that scope 4 admits reproduce the expected tuples. For
     `parameter`, the host supplies `Key` = `C2`. The suite also covers
     validation refusals for crossing reads and refused node kinds, a key node
     error failing its scope, a Null-like key with an empty right side, the
     key as first source and as a stage's right source, and filter, sort and
     window controls over computed-key tuples.
3. **Generated Rust and C#.**
   - The codegen mirror types, validation and join runtimes in both
     languages, admitted only in profiles that already admit legacy joins.
   - *Acceptance:* the scope 2 projects, generated in both languages, match
     `engine::run`, including key-node failures.
4. **MFD import.**
   - Admit a two-input join in which one input is fed by a constant, a runtime
     input parameter, or a call chain of `uppercase`, `lowercase`, `concat`
     and `add` over constants, parameters and source fields admissible in the
     receiving context. The other input must be a repeating source collection.
     The computed input may be input 0 or input 1; `tokens-second` shows that
     tuple order then follows the input order.
   - The computed input has no outputs, and the join drives a target
     iteration.
   - Lowering must reproduce native empty-input behavior. When any source
     field the key depends on is absent, `Null` or `XmlNil`, the lowered key
     is `Null`. Ferrule's `upper` refuses `Null` and its `concat` treats it as
     `""`, so the plain call graph would not give the native result.
   - The connection kind and the input entry name are not significant on
     import.
   - Everything else keeps today's "not a plain source feed" refusal, and the
     specific refusals above report their own reasons.
   - *Acceptance:* every admitted fixture imports warning-free and executes
     to its expected tuples. Every refusal fixture keeps the join and its
     driven scopes out with its reason. The existing join and correlated
     singleton suites still pass unchanged.
5. **MFD export.**
   - Write a computed source as a join input fed by the exported node's
     output. Constants and calls use an input named `value` with an ordinary
     connection. A runtime parameter uses an input named after the parameter,
     with a copy-all connection.
   - Refuse atomically every plan that scope 4 would not import.
   - *Acceptance:* export and re-import of every scope 4 fixture without
     semantic drift, and atomic refusal of the rest.
6. **Sequence-valued keys.**
   - A `Sequence` variant of the computed source over the existing bounded
     `SequenceExpr` (`tokenize` first), with one member per item in order,
     raw positions from 1, and empty tokens kept as members.
   - Generated items count toward the generated-sequence limit, and pairings
     toward the join row budget.
   - Engine, generated code, import and export land in this scope or in its
     own sub-issues, with the same gates.
   - A key over a repeating field outside any iteration of it
     (`root-repeating`) stays refused until a sequence form can express a
     per-item mapping exactly.
   - *Acceptance:* the `tokens` and `tokens-second` fixtures.

[MFD interop](../mfd-interop.md) and the [project file
capabilities](../project-files.md#file-capabilities) change in the scope whose
behavior they describe.

## Frozen fixtures

The fixtures under
[`fixtures/join-computed-keys`](fixtures/join-computed-keys/) are
self-authored.

- `source.xsd` declares `Source`, which contains:
  - `Header/Code` (string);
  - repeating `Order` with `Id`, an optional nillable `CustomerNumber`, an
    optional `Codes` and an optional integer `Qty`;
  - repeating `Customer` with `Number`, `Name` and an integer `Rank`.
- `target.xsd` declares `Report`, with repeating `Order` (`Id`, repeating
  `Match` of `Number` and `Name`) and repeating root `Hit` (`Number`, `Name`).
- [`expected.json`](fixtures/join-computed-keys/expected.json) records, for
  each design, its input, key, context and connection kind, the native
  outcome, its expected tuples as `[Number, Name]` pairs, and its route.

Both inputs hold the same customers, in this order:

| # | Number | Name | Rank |
| --- | --- | --- | --- |
| 1 | `C1` | Ada | 1 |
| 2 | `C2` | Grace | 2 |
| 3 | `C2` | Hopper | 3 |
| 4 | `""` | Blank | 4 |
| 5 | `7` | Seven | 7 |
| 6 | `07` | ZeroSeven | 7 |
| 7 | `7.0` | SevenPoint | 8 |
| 8 | `8` | Eight | 9 |

`input.xml` holds these orders:

| Order | CustomerNumber | Codes | Qty |
| --- | --- | --- | --- |
| O1 | `c1` | `C2 C1 C2` | 7 |
| O2 | `c2` | `C9` | 8 |
| O3 | absent | absent | absent |
| O4 | `xsi:nil` | `C1` | `07` |
| O5 | `""` | `""` | 9 |
| O6 | `C1` | `C2` | 1 |
| O7 | `7` | `C1 C1` | 2 |

`input-numeric-text.xml` holds orders T1 to T5 with `CustomerNumber` values
`07`, `7.0`, ` 7`, `+7` and `7`, and no other order fields.

**Nested designs.** In each design, the target `Order` iterates source
`Order`. The join's tuples drive `Match`, with input 1 being the `Customer`
collection unless noted. Orders without a listed result have no matches.

| Design | Key (input 0) | Right key | Result |
| --- | --- | --- | --- |
| `direct` | `CustomerNumber`, direct (existing form) | `Number` | O5 Blank; O6 Ada; O7 Seven |
| `upper` | `uppercase(CustomerNumber)` | `Number` | O1 Ada; O2 Grace, Hopper; O5 Blank; O6 Ada; O7 Seven |
| `lower` | `lowercase(CustomerNumber)` | `Number` | O5 Blank; O7 Seven |
| `concat` | `concat(CustomerNumber, "")` | `Number` | O5 Blank; O6 Ada; O7 Seven |
| `constant` | constant `C2` | `Number` | every order: Grace, Hopper |
| `parameter` | runtime parameter `Key`, preview `C2` | `Number` | every order: Grace, Hopper |
| `tokens` | `tokenize(Codes, " ")` | `Number` | O1 Grace, Hopper, Ada, Grace, Hopper; O4 Ada; O5 Blank; O6 Grace, Hopper; O7 Ada, Ada |
| `tokens-second` | as `tokens`, but as input 1 after `Customer` | `Number` | O1 Ada, Grace, Grace, Hopper, Hopper; otherwise as `tokens` |
| `numeric` | `add(Qty, 0)` with a decimal constant | `Number` | O1 Seven; O2 Eight; O4 Seven |
| `integer-direct` | `Qty`, direct | `Number` | O1 Seven; O2 Eight; O4 Seven |
| `rank` | `uppercase(CustomerNumber)` | `Rank` | O7 Seven, ZeroSeven |
| `rank-direct` | `CustomerNumber`, direct | `Rank` | O7 Seven, ZeroSeven |
| `rank-text` | `uppercase(CustomerNumber)`, numeric-text input | `Rank` | T5 Seven, ZeroSeven |
| `rank-text-direct` | `CustomerNumber`, direct, numeric-text input | `Rank` | T5 Seven, ZeroSeven |

**Root designs.** The join's tuples drive the root `Hit` collection, and no
`Order` is produced.

| Design | Key | Result |
| --- | --- | --- |
| `root-constant` | constant `C2` | Grace, Hopper |
| `root-header` | `uppercase(Header/Code)` (`c2`) | Grace, Hopper |
| `root-repeating` | `uppercase(Order/CustomerNumber)` | Ada, Grace, Hopper, Blank, Ada, Seven |

**Refusal designs.**

- `copy-all-computed` is `upper` with a copy-all connection on the computed
  input. The native application refuses it at validation.
- `key-read` is `upper` with `Match/Number` fed from the computed input's own
  output. The native application refuses it at validation.
- `unlisted-function` is `upper` with `normalize-space`. It was not run, and it
  is refused.

The designs come in two groups. `direct`, `upper`, `lower`, `concat`, `constant`,
`parameter`, `numeric`, `integer-direct`, `rank`, `rank-direct`, `rank-text`,
`rank-text-direct`, `root-constant` and `root-header` are admitted by scopes 2
and 4. `tokens` and `tokens-second` wait for scope 6, and `root-repeating`
stays refused.

## Acceptance plan

- **Model scope.** Wire round trips for both source kinds and both condition
  kinds, every plan error, the capability, older-reader refusal, unchanged
  bytes for existing plans, and an explicit refusal in every consumer.
- **Engine scope.** Every admitted fixture row through `engine::run` with
  hand-built projects, plus the validation, failure, ordering and control
  cases listed under scope 2.
- **Generated code scope.** The engine scope's projects through generated Rust
  and C#.
- **MFD import scope.** Every fixture design and refusal design, plus the
  unchanged existing join suites (`join_import`, `correlated_scalar_join`,
  `hierarchical_join_import`).
- **MFD export scope.** Export and re-import of the admitted fixtures, and
  atomic refusal of the others.
- **Sequence scope.** The `tokens` and `tokens-second` rows end to end.
