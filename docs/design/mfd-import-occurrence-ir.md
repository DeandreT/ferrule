# MFD import occurrence provenance

Two target entries can share both a path and a producer. The MFD importer
must distinguish those authored occurrences until it proves who owns them.

## Decision

Keep occurrence records in the recognizers that need them. A shared
expression keeps its `NodeId`; a declared duplicate entry keeps its own
binding. Neither a new intermediate representation nor a `Project` wire
change is required ([design decision](https://github.com/DeandreT/ferrule/issues/793)).

## Identities import already preserves

| Identity | Witness |
| --- | --- |
| Authored entry | Its own input key in `SchemaComponent::ports`. |
| Lineage | Non-input ancestor markers in `input_ancestors`; disagreement gives no unique chain. `ambiguous_branch_inputs` prevents an ambiguous constructed driver. |
| Binding origin | Index of the `(TargetLeaf, feed, input)` tuple in [target-scope construction](../../crates/mfd/src/import/target_scope.rs). |
| Driver | `TargetIteration` keeps the feed, target port and additional feeds with their branch marker. |
| Producer | The source path or function component's interned `NodeId`; one producer may feed many entries. |

## Where occurrence identity is lost

`TargetLeaf` is a path; an installed `Binding` is a field and a node.
Neither carries an occurrence. The [scope builder](../../crates/mfd/src/import/scope.rs)
receives the authored occurrence separately. Because it merges scopes by
field name and keys anchors by target path, each cloned branch needs its
own builder and a snapshot of its anchors.

`BindingOrigins` maps `(target path, NodeId)` to all original binding indices.
`DeclaredBranches::layouts` retains branch order so partitioning can pair
each segment with its declaration. `order_repeating_scalar_bindings` uses
entry ancestry to recover repeated-scalar order; numeric pin IDs are not
positions.

## Refusals keyed by interned bindings

| Refusal | Proof over interned keys |
| --- | --- |
| Incomplete placeholder preservation | The retained bindings for a `(path, NodeId)` must account one-to-one for the authored origins in that scope. |
| Implicit JSON/CSV clone segments | Drop a candidate that does not retain each `(path, NodeId)` once per authored origin. |
| Ambiguous declared ancestor | `enclosing_anchors` requires exactly one owner in the nearest declared ancestor layout. |
| Duplicate singular feeds | Shared and distinct expressions both reach validation as separate bindings and [refuse](../conformance/singular-target-feeds.md#disposition-by-target-shape). |

Each refusal keeps the repair-oriented import draft: ordinary import retains
the partially built scopes and appends the diagnostic, while the executable
profile rejects the design.

## Cloned-target branch owners

When nested siblings share a producer, `(path, NodeId)` alone makes each
binding appear to stand for both. The [branch recognizer](../../crates/mfd/src/import/target_iteration/branch_owners.rs)
resolves deeper wrappers first. At each level it plans without mutation,
proves every replacement retains its placeholders exactly once, then moves
proved segments and records their placements. Diagnostics follow the same
depth-first, then child declaration order.

An ancestor uses the innermost recorded placement to narrow the set of
possible origins. It cannot invent an origin: if two origins still map to
one retained binding, the candidate refuses.

### Comparison

| Shape | Conservative recognizer | With occurrence record |
| --- | --- | --- |
| Sibling clones bound to distinct expressions | Admitted | Admitted, identical project |
| Sibling top-level clones bound to one expression | Admitted (separate segments) | Admitted, identical project |
| Nested sibling clones bound to one expression | Refused as unproved placeholder | Admitted with the same values as the distinct-expression design |
| One clone occurrence whose sibling entries share one expression | Refused, then the enclosing wrapper refused | Distributed; its singular duplicate refuses exactly like distinct feeds |

The [literal cases](../../crates/mfd/tests/fixtures/nested_clone_owners/occurrence_cases.json)
bind `shared_constant_cells.mfd`, `shared_constant_rows.mfd` and
`aliased_cell_values.mfd` to their expected admission and values.

## Candidate-before-mutation across recognizers

Sparse children leave builder and owner state unchanged on refusal. Implicit
clone branches commit graph, scopes, claimed bindings and declared branches
together. Conditional presence drivers first prove *every* declared clone
of a target path; a refused path keeps its ordinary lowering, gets one
diagnostic and cannot be claimed as an implicit clone.

In contrast, [`build_concatenated` and `build_ordered_branches`](../../crates/mfd/src/import/target_iteration.rs)
build each segment against the shared graph. A later refusal retains earlier
nodes and diagnostics. Making these operations transactional would change
node numbering and warning content.

## Binding canonicalization

`ScopeBuilder` keys bindings by authored occurrence, not by
`(target_field, NodeId)` ([#898](https://github.com/DeandreT/ferrule/issues/898)).
`BindingOccurrence::authored` makes a target input its
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

A repeating scalar emits one value per present entry in entry-tree order;
the [binding cases](../../crates/mfd/tests/fixtures/nested_clone_owners/binding_occurrence_cases.json)
include `shared_repeating_values.mfd`. The [shared singular control](../../crates/mfd/tests/fixtures/input_guard/shared-singular.mfd)
refuses just like two distinct constants.

## Reasons

Input keys, binding indices and declared lineages are already available
where the proof runs. Replacing producer interning would change node IDs
in other designs; a parallel IR would require requalifying every recognizer.
Revisit the decision if a proof crosses target scope trees or needs
identity unavailable at its own boundary.
