# MFD import occurrence provenance

This note decides how `.mfd` import preserves authored target occurrence
identity until lowering has proved ownership
([#793](https://github.com/DeandreT/ferrule/issues/793)). It covers the
target-side importer in `crates/mfd/src/import`; the public `Project` wire
format is unchanged.

## Decision

ferrule does not introduce a separate import-local occurrence IR. Import
keeps the authored identities it already reads, and each recognizer that
needs occurrence identity owns a narrow occurrence record and proves a
complete candidate before mutating shared import state. The interned
expression graph remains the producer representation; it never stands in for
authored occurrence identity.

One targeted gap remains outside this decision: duplicated target entries
fed by the same expression collapse into one scope binding (see
[Binding canonicalization](#binding-canonicalization)). It is a separate
follow-up because closing it changes output cardinality and refusal behavior.

## Identities import already preserves

| Identity | Where it lives | Notes |
| --- | --- | --- |
| Authored target occurrence | input key of each target entry, `SchemaComponent::ports` (`import/schema/mod.rs:246`) | Every declared clone has its own input keys. |
| Lineage | non-input ancestor markers, `SchemaComponent::input_ancestors` (`import/schema/mod.rs:249`) | `branch_owners::lineage` selects one marker chain and returns `None` when chains disagree. |
| Ambiguous lineage | `SchemaComponent::ambiguous_branch_inputs` (`import/schema/mod.rs:255`) | Recorded at schema extraction; consumed by `unique_constructed_driver` (`import/target_iteration.rs:413-451`). |
| Binding witness | `(TargetLeaf, feed, input)` tuples collected by `build_target_scope` (`import.rs:2848-2949`) | A tuple index is the authored binding origin used by every branch recognizer. |
| Driver | `TargetIteration { feed, target_port, additional_feeds }` | Per-branch drivers stay paired with their branch marker until `build_concatenated`. |
| Producer | `NodeId`, interned per source path (`import/graph.rs:84-96`) and per function component (`import/scalar_function.rs:13-23`) | One producer may feed many consumers; sharing a node is correct. |

Authored occurrence identity is therefore available, at the recognizer
boundary, as input keys, lineages and binding indices. A dedicated IR would
duplicate these values without adding information that import does not
already read.

## Where occurrence identity is lost

Identity is lost only when consumer edges are installed into
`mapping::Scope`, which deliberately has no occurrence field:

- `ScopeBuilder::add_binding` (`import/scope.rs:202-215`) drops a binding
  whose `(target_field, node)` already exists. Two authored entries fed by one
  interned producer become one `Binding`. Callers include the final binding
  loop (`import.rs:3055`), concatenated branch segments
  (`import/target_iteration.rs:215`, `:346`) and implicit clone lowering
  (`import/target_iteration/implicit_branches/lowering.rs:99`).
- `TargetLeaf` (`import/scope.rs:9-12`) carries only a target path; the final
  install loop (`import.rs:3018-3056`) discards the input key when it adds the
  binding.
- `ScopeBuilder::ensure_scope` (`import/scope.rs:71-91`) merges scopes by
  target field name, and anchors are keyed by target path
  (`import/scope.rs:38-41`, `:117-118`). Cloned occurrences remain separate only
  because branch recognizers build each segment in its own `ScopeBuilder` and
  snapshot its anchors into `DeclaredBranch` (`branch_owners.rs:14-19`).

Recognizers that run after these points reconstruct ownership through side
tables keyed by the interned binding:

- `BindingOrigins` (`branch_owners.rs:8`) maps `(target path, NodeId)` to the
  set of authored binding indices that produced it.
- `DeclaredBranches::layouts` records declared branches in the same order as
  the concatenated segments built for that path; `partition`
  (`branch_owners.rs:551`) relies on that alignment.
- `order_repeating_scalar_bindings` (`import.rs:3114-3147`) restores entry-tree
  order for repeated scalar clones from `input_ancestors`.

## Refusals that depend on the loss

| Refusal | Proof that depends on interned keys |
| --- | --- |
| `nested cloned target … cannot prove complete placeholder binding preservation` | `DeclaredBranches::collect_retained_within` refuses when one retained binding maps to more than one authored origin (`branch_owners.rs:319`). |
| Implicit JSON/CSV clone segments | `json_implicit::bindings_preserved` (`implicit_branches/json_implicit.rs:247-278`) requires exactly one origin per `(path, NodeId)`; the candidate is dropped at `implicit_branches/lowering.rs:102-105`. |
| `nested cloned target … has ambiguous declared ancestor ownership` | `enclosing_anchors` (`branch_owners.rs:79`) returns `None` when no unique declared owner exists (`import/target_iteration.rs:179-186`, `:308-315`). |
| Duplicate singular feeds ([#241](https://github.com/DeandreT/ferrule/issues/241)) | The engine validator refuses repeated bindings to a singular field, but only after `add_binding` has collapsed identical ones. |

Each refusal keeps the repair-oriented import draft: ordinary import retains
the partially built scopes and appends the diagnostic, while the executable
profile rejects the design.

## Cloned-target branch owners

Nested clone distribution is the recognizer where interning caused a false
refusal. Two sibling clones of a nested group, both bound to one shared
expression, produced the same `(path, NodeId)` key. The ancestor proof
gathered origins from every declared descendant branch under the owner, so
each retained binding appeared to stand for both authored bindings and the
whole enclosing wrapper was refused. Distinct expressions in the same shape
were admitted.

The recognizer now works as follows:

1. `distribute_at` (`branch_owners.rs:104`) resolves deeper wrappers first,
   then calls `plan_level` for the current wrapper.
2. `plan_level` (`branch_owners.rs:129`) reads only. It returns a `LevelPlan`
   holding each candidate `Distribution` (the child wrapper index and the
   declared child branches each parent segment owns) and diagnostics in child
   declaration order. A candidate is admitted only after
   `preserves_placeholders` (`branch_owners.rs:189`) proves that every
   replaced placeholder binding is retained exactly once.
3. `apply_level` (`branch_owners.rs:415`) emits the diagnostics, moves each
   proved child segment into its owning parent segment, and records the
   placement.

`Placements` (`branch_owners.rs:37`) is the import-local occurrence record.
For each distributed child path it keeps, per declared parent segment, the
declared child branches placed beneath it. During an ancestor proof,
`placed_occurrences` (`branch_owners.rs:376`) pairs a distributed child's
scopes with those recorded occurrences. When a retained binding still maps to
several origins, the innermost recorded occurrence that holds that binding
decides which origin it is. The occurrence record never adds an origin; it
only narrows an existing ambiguous set to one member. An occurrence that itself
holds several authored origins for one interned binding remains refused.

Planning before mutation keeps the warning order unchanged: deeper wrappers
still emit first, and diagnostics at one level follow child declaration order.

### Comparison

| Shape | Conservative recognizer | With occurrence record |
| --- | --- | --- |
| Sibling clones bound to distinct expressions | Admitted | Admitted, identical project |
| Sibling top-level clones bound to one expression | Admitted (separate segments) | Admitted, identical project |
| Nested sibling clones bound to one expression | Refused as unproved placeholder | Admitted with the same values as the distinct-expression design |
| One clone occurrence holding two authored edges to one field | Refused, then the enclosing wrapper refused | Refused in the same order |

The fixtures are `tests/fixtures/nested_clone_owners/shared_constant_cells.mfd`,
`shared_constant_rows.mfd` and `aliased_cell_values.mfd`. Their native values,
executable-profile admission and generated Rust/C# values are checked against
`occurrence_cases.json`.

## Candidate-before-mutation across recognizers

- `sparse_child::build` is transactional: refused candidates leave builder
  and ownership state unchanged (`import/target_iteration.rs:45-56`).
- Implicit clone branches build against a cloned `GraphBuilder`
  (`implicit_branches/lowering.rs:23`) and commit graph, scopes, claimed
  bindings and declared branches together.
- Nested clone distribution plans each level before applying it.
- `build_concatenated` and `build_ordered_branches`
  (`import/target_iteration.rs:90-376`) still call `build_one` for each
  segment against the shared `GraphBuilder` before later segments prove
  ownership, so an ownership refusal keeps earlier segments' nodes and
  diagnostics. Making them transactional changes node numbering and warning
  content, so it belongs with a change that requalifies those designs.

## Binding canonicalization

`add_binding` collapses duplicated target entries fed by one expression.
For a repeating scalar target, two authored entries bound to one constant
produce one value, while two entries bound to distinct constants produce two.
For a singular target, identical feeds collapse silently while distinct
feeds keep the duplicate-binding refusal. Keying binding canonicalization by
authored input occurrence instead of `(target_field, NodeId)` closes both
cases without a wire change: a repeated install of one authored input stays
idempotent, distinct authored entries remain distinct bindings, and
singular duplicates fall under the existing #241 refusal. That change alters
output cardinality and refusal outcomes for designs that import today, so it
is qualified separately with complete native and generated oracles in
[#898](https://github.com/DeandreT/ferrule/issues/898).

## Reasons

- The authored identities a new IR would carry are already present at every
  recognizer boundary. The losses are confined to scope installation and to
  side tables keyed by the interned binding.
- Producer interning is correct and widely relied on; replacing it would
  change node numbering in every imported project.
- A general IR would run in parallel with the existing `GraphBuilder` and
  scope pipeline during migration, and every recognizer would need
  requalification against complete native and generated oracles.
- The demonstrated false refusal is fixed by a recognizer-local record with
  no change to admitted projects or diagnostics elsewhere.

Revisit this decision when a recognizer needs occurrence identity across
more than one target scope tree, or when a refusal's proof needs identity
that input keys, binding indices and declared lineages cannot supply at the
recognizer's own boundary.
