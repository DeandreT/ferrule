# Closed primary-source safety guard

`Project.execution_contract` optionally declares `ClosedPrimarySourceV1`:

```json
"execution_contract": { "kind": "closed_primary_source_v1" }
```

Missing means legacy execution. The guard is an execution admission rule, not
an XML reader setting or full XSD validation. It applies to programmatic input
trees as well as parsed documents. It neither changes source resolution nor
coerces values.

## Model and owner proof

The current model is validated whenever guarded execution is requested. A saved
claim, precomputed shape or alleged list of protected node IDs is not trusted.
The entire guarded project must fit this bounded profile:

- One nonrepeating ordinary primary group with unique unqualified field names.
  Slots are scalars, singleton groups or repeated groups. Alternatives, open
  fields, wildcard/virtual names, repeated scalars and recursive schemas refuse.
- Constructed static receiving scopes, with no generated/owned/join iteration,
  filtering, sorting, grouping, windows, copying or dynamic context creation.
  Each iterating scope names one direct repeated group child of the primary
  root or immediately active source item. Empty and multi-segment drivers refuse.
- An unpinned source read has one exact root scalar path with no repetition.
  Its first segment is absent from every active item schema, and no active
  collection prefix captures it. This proof includes readable fields that have
  no mapping ports; matching runtime values do not supply authority.
- A pinned source read has the exact absolute nearest repeated ancestor and an
  exact scalar suffix with no uniterated repetition. The existing latest
  equality/suffix frame match must select that ancestor, using the concrete
  local collection labels. An outer Row pin in Row/Row refuses; a proved
  Row/Cell ancestor read can remain valid.
- Constants and ordinary scalar expressions retain their existing evaluation
  behavior. Scalar UDF bodies remain isolated, with eager arguments and lazy
  conditional branches. The guard does not evaluate bodies to validate them.
  Secondary source declarations, host-parameter nodes, source-dependent UDF
  bodies and failure-rule iteration are outside this initial guarded profile.

Ordinary projects without the member retain their existing behavior. Removing
the guard deliberately is a mapping change, not a supported downgrade or a
tamper-proofing mechanism.

## Actual input admission

Before graph/failure evaluation, source loading or trace/debug value events,
guarded entry points reject nonempty supplied secondary bindings and runtime
parameters. This check depends on the supplied count, not on declared names or
reachability. Empty binding/parameter containers are allowed; clock/path
metadata, tracing and execution controls remain independent.

Primary document and filename APIs check model/bindings before selection,
loading, parsing or filename evaluation. A directly acquired document receives
the actual-tree check after parsing. Row-only and document-set acquisition
cannot supply this contract's singular primary group and refuse before loading.

The primary input must fit the closed declared tree:

- Unknown and duplicate group fields refuse.
- Every repeated group slot is explicitly present as `Repeated`, including
  optional slots. `Repeated([])` is valid. Absence and scalar Null are not
  substitutes and are never normalized into an empty list.
- Repeated group members are ordinary groups, not Null, nested arrays,
  document sets or mapped sequences. Singular slots cannot acquire repetition.
- Present scalars have the declared domain without coercion. Null remains
  scalar absence; XmlNil is retained only for nillable scalar slots. Foreign
  scalar domains and nonfinite floats refuse. Missing scalar fields and absent
  singleton groups retain their existing nullable/presence behavior.

Failures use `SourceContractViolation` with a bounded declared path and reason,
not input values. Admission failure can precede a lazy expression's existing
error. This is an invocation-level input contract, not an order-processing
failure-isolation policy.

The checker borrows scalar payloads. Its iterative traversal counts occurrences,
including shared subtrees supplied by a host. Limits are 256 declared path
segments, 4,096 path-name bytes, 65,536 shape entries, 32 MiB shape/model metadata
and 1,000,000 validation work visits. Every actual node and every reserved group
schema slot counts; sparse wide groups cannot multiply unchecked metadata work.
Existing lower file/import/host limits still apply.
Exhaustion is a typed refusal, not a partial validation or fallback.

Static owner proofs use a shared 1,000,000-work and 32 MiB text/copy reservation
ledger. Before read or driver traversal, it reserves each possible child lookup
as up to 17 binary-search comparisons, query-byte comparisons, ancestor work
and path copies. Prefix inventories are reserved before traversal/allocation.
Repeating a deep binding therefore consumes the same bounded model ledger; an
empty actual collection does not authorize unbounded static proof work.

## Persistence and interchange

Supported Project, embedded Pipeline and DocumentProject file codecs require
version 3 with `primary-source-safety-guard-v1` whenever the member is present,
even for source-free or unused expressions. Missing, stale or unknown
capabilities, plain guarded files and version-2 downgrades refuse. The contract
object itself is strict and rejects unknown members or variants.

Raw `Project` Serde is the legacy wire API, not the safe saved-file codec. Older
raw readers cannot be promised to understand an optional new member. Use the
supported file codecs to retain fail-closed capability negotiation.

Guarded MFD export is refused in **both** NativeMfd and FerruleExtensions modes,
including package and pipeline exports, before any design or sibling schema is
published. There is no ignorable XML guard marker or guarded MFD round trip.
Use the capability-bearing project/pipeline codec instead.

Generated backends must carry the contract through Program lowering and enforce
it at every admitted public entry. Direct Program construction must validate
the same profile rather than stripping the member. Existing owned-source UDF
capability refusals remain in force; the guard does not add owned-UDF execution.
