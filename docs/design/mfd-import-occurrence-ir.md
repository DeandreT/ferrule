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

Scope bindings are canonical by authored occurrence rather than by
expression, so duplicated target entries fed by one expression keep one
binding each (see [Binding canonicalization](#binding-canonicalization)).

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

- `ScopeBuilder::add_binding` (`import/scope.rs:266`) receives the
  authored `BindingOccurrence` (`import/scope.rs:39`) beside each binding and
  records it in the builder, not in the scope; the installed `Binding`
  carries only the field and node. The final binding loop
  (`import.rs:3015-3056`), concatenated branch segments
  (`import/target_iteration.rs:215`, `:347`), implicit clone lowering
  (`import/target_iteration/implicit_branches/lowering.rs:96`), sparse child
  construction and XML text group projection pass their authored input.
- `TargetLeaf` (`import/scope.rs:10`) carries only a target path; its
  occurrence travels separately.
- `ScopeBuilder::ensure_scope` (`import/scope.rs:130`) merges scopes by
  target field name, and anchors are keyed by target path
  (`import/scope.rs:86-92`, `:176-177`). Cloned occurrences remain separate only
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

## Refusals keyed by interned bindings

| Refusal | Proof over interned keys |
| --- | --- |
| `nested cloned target … cannot prove complete placeholder binding preservation` | `DeclaredBranches::collect_retained_within` (`branch_owners.rs:272`) refuses when the retained bindings for one `(path, NodeId)` in a scope cannot be matched one-to-one with the authored origins they stand for. |
| Implicit JSON/CSV clone segments | `json_implicit::bindings_preserved` (`implicit_branches/json_implicit.rs:247-280`) requires each `(path, NodeId)` to be retained once per authored origin; the candidate is dropped at `implicit_branches/lowering.rs:103-107`. |
| `nested cloned target … has ambiguous declared ancestor ownership` | `enclosing_anchors` (`branch_owners.rs:79`) returns `None` when no unique declared owner exists (`import/target_iteration.rs:179-186`, `:309-316`). |
| Duplicate singular feeds ([#241](https://github.com/DeandreT/ferrule/issues/241)) | The engine validator refuses repeated bindings to a singular field. Duplicate entries fed by one expression reach it as separate bindings, exactly like distinct feeds. |

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
3. `apply_level` (`branch_owners.rs:434`) emits the diagnostics, moves each
   proved child segment into its owning parent segment, and records the
   placement.

`Placements` (`branch_owners.rs:37`) is the import-local occurrence record.
For each distributed child path it keeps, per declared parent segment, the
declared child branches placed beneath it. During an ancestor proof,
`placed_occurrences` (`branch_owners.rs:395`) pairs a distributed child's
scopes with those recorded occurrences. When the retained bindings for one
`(path, NodeId)` in a scope still map to several origins, the innermost
recorded occurrence that holds that key decides which origins they are,
provided it holds exactly one origin per retained binding. The occurrence
record never adds an origin; it only narrows an existing ambiguous set. An
occurrence whose sibling entries share one expression retains one binding per
entry and is proved; one that holds more authored origins than retained
bindings remains refused.

Planning before mutation keeps the warning order unchanged: deeper wrappers
still emit first, and diagnostics at one level follow child declaration order.

### Comparison

| Shape | Conservative recognizer | With occurrence record |
| --- | --- | --- |
| Sibling clones bound to distinct expressions | Admitted | Admitted, identical project |
| Sibling top-level clones bound to one expression | Admitted (separate segments) | Admitted, identical project |
| Nested sibling clones bound to one expression | Refused as unproved placeholder | Admitted with the same values as the distinct-expression design |
| One clone occurrence whose sibling entries share one expression | Refused, then the enclosing wrapper refused | Distributed; its singular duplicate refuses exactly like distinct feeds |

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

`ScopeBuilder` keys bindings by authored occurrence, not by
`(target_field, NodeId)` ([#898](https://github.com/DeandreT/ferrule/issues/898)).
`BindingOccurrence::authored` (`import/scope.rs:54`) makes a target input its
own occurrence when its entry is one of several same-named sibling entries of
one parent entry occurrence: its last ancestor is a duplicate-entry marker
whose recorded branch path is exactly the input's target path, and the
remaining ancestors identify the parent. Each such input keeps its own
binding even when one interned expression feeds several of them; installing
one input again is idempotent. Every other binding stays canonical by
`(target_field, NodeId)`:

- entries whose duplicated ancestors import merges into one scope, such as
  XML type alternatives, which describe one output occurrence;
- descendants projected from one group input, including copy-all group,
  structured UDF, constructed-record and join projections;
- JSON scalar alternatives, which schema extraction does not record as
  occurrences.

A repeating scalar target therefore emits one value per duplicate entry in
entry-tree order, and duplicate singular entries reach the engine validator
as separate bindings, so identical feeds refuse exactly like distinct feeds
while #241 keeps singular selection semantics. Producer interning, node
numbering and the `Project` wire format are unchanged. The fixtures are
`tests/fixtures/nested_clone_owners/shared_repeating_values.mfd`, checked
natively and in generated Rust/C# against `binding_occurrence_cases.json`,
and `tests/fixtures/input_guard/shared-singular.mfd`, whose refusal matches
the distinct-constant `competing-singular.mfd` control.

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
