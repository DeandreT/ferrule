# Owned starting-boundary grouping

Owned `group_starting_with` partitions selected candidates at Boolean start
boundaries. It uses the existing mapping field and authenticated source-cohort
contract; saved-project grouping and ownership fields retain their representation.

## Explicit generation profile

Use `codegen::lower_with_basic_owned_group_starting_with`, followed by
`codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options)` or
`codegen_csharp::emit_with_basic_owned_group_starting_with(&program)`. Direct
Program hosts use `validate_program_with_basic_owned_group_starting_with` and
`validate_basic_owned_group_starting_with_emission`; named-input discovery uses
`legacy_named_input_admission_plan_with_basic_owned_group_starting_with`.

This profile admits static Basic By, AdjacentBy and StartingWith scopes alongside
the existing filter, sort, window and member-retention controls. Scalar producer
consumption and primary-row failure eligibility remain independent permissions.
The parent-context [ItemAt contract](owned-sequence-item-at.md) still applies;
ItemAt remains unavailable in owned failure predicates and messages. Earlier
generation profiles keep their own capability sets. The
[Ending profile](owned-group-ending-with.md), the
[IntoBlocks profile](owned-group-into-blocks.md) and specialized owned
grouping have separate gates.

Generated libraries retain the existing typed Basic execution entries,
mandatory five-field policy and selected-target validation precedence.

## Boundaries and callback order

The first selected candidate creates the first group even when its boundary is
false. Each subsequent true boundary starts a new group containing that
candidate; false extends the current group. Thus `false,false,true,true,false`
partitions five candidates into `[1,2]`, `[3]`, `[4,5]`. An empty cohort produces
no group. The boundary is evaluated for the first candidate as well.

The operation authenticates the complete original cohort and admits known
metadata before semantic callbacks. Selection filtering and stable sorting keep
their existing phase rules: sorting with FilterThenSort evaluates selection
predicates before keys; without sorting, that mode does not add an eager
all-predicates pass. Explicit window bounds evaluate once, in stored order, in
the receiving parent after sorting and before boundary evaluation, including
empty cohorts. FromTo evaluates the first bound before the last.

For each reached candidate, any remaining selection predicate runs first, then
the start boundary, then member retention. Boundary and retention callbacks
observe candidate positions. A non-Boolean boundary reports the existing typed
Boolean error at the original predicate node. A boundary failure precedes a
retention failure at that candidate.

Member retention evaluates every reached member and keeps a whole constructed
group when any member succeeds. False members remain in that group, including
its first representative. Dropping a group does not move its members into a
neighbor or recompute boundaries. Windows select retained groups afterward and
assign compact one-based group ordinals. Consequently, `First(0)` does not hide
boundary or retention errors. Only selected groups evaluate bindings and
children; global failures retain their earlier precedence.

## Owner identity and members

A starting group uses the shared [group-member identity](source-ownership.md#group-member-occurrence-identity).
Only authenticated grouping mints it. The first ordered member supplies the
representative and intermediate prefix; exact-owner root pins, compact group
ordinals and nested member positions stay distinct through clones, ranking,
legacy bridges and immutable C# publication.

## Runtime and resource contract

Rust exposes `BasicGroupStartingWithControls` and
`map_group_starting_with_source_cohort`. C# exposes
`IFerruleBasicGroupStartingWithControls` and `MapGroupStartingWithSourceCohort`.
Their start callback returns a Boolean. Both reuse the unchanged six-field
`BasicGroupByMetadata`/`FerruleBasicGroupByMetadata` describing static phases.

The paired implementation shares the By/Adjacent preallocated buckets, member
links, templates and output backing. The existing n-key backing remains reserved
and Null-filled for StartingWith; boundaries do not add a retained vector, heap
controls adapter or resettable per-group budget. Boolean boundaries consume no
grouping-key text credits. String sort keys retain their existing accounting.
Published C# contexts and member arrays remain immutable.

The interpreter keeps its separate WorkBudget accounting. The five-field policy
bounds retained metadata and contexts, rather than source/output payloads or
process-wide CPU and memory. Native tracing records StartingWith and its predicate
node with no grouping key.

## Qualification boundaries

The [StartingWith corpus](../../conformance/basic-owned-group-starting-with/)
covers 105 profiles and 381 calls, including complete typed outcomes. The
ordinary suite also checks refused Projects/Programs, earlier presets, named
inputs and the saved legacy project. To execute the generated public APIs
against the same outcomes (Cargo and .NET 10):

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_starting443_generated_rust_and_csharp_match_expected -- --ignored
```
