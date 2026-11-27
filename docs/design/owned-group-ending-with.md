# Owned ending-boundary grouping

Owned `group_ending_with` partitions selected candidates at Boolean end
boundaries. It uses the existing mapping field and authenticated source-cohort
contract; saved-project grouping and ownership fields retain their representation.

## Explicit generation profile

Use `codegen::lower_with_basic_owned_group_ending_with`, followed by
`codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options)` or
`codegen_csharp::emit_with_basic_owned_group_ending_with(&program)`. Direct
Program hosts use `validate_program_with_basic_owned_group_ending_with` and
`validate_basic_owned_group_ending_with_emission`; named-input discovery uses
`legacy_named_input_admission_plan_with_basic_owned_group_ending_with`.

This profile admits static Basic By, AdjacentBy, StartingWith and EndingWith
scopes alongside the existing filter, sort, window and member-retention controls.
Scalar producer consumption and primary-row failure eligibility remain
independent permissions. The parent-context [ItemAt contract](owned-sequence-item-at.md)
still applies; ItemAt remains unavailable in owned failure predicates and
messages. Earlier generation profiles, including the
[Starting profile](owned-group-starting-with.md), keep their own capability sets
and refuse owned EndingWith with their existing typed Grouping diagnostic.
The [IntoBlocks profile](owned-group-into-blocks.md) and specialized owned
grouping have separate gates.

Generated libraries retain the existing typed Basic execution entries,
mandatory five-field policy and selected-target validation precedence. For a
design without owned EndingWith, the Ending profile emits the same artifacts as
the Starting profile.

## Boundaries and callback order

A true boundary closes the current group, and that candidate belongs to it. The
next selected candidate opens a new group. Candidates after the last true
boundary form one trailing group, and a true last boundary adds no empty group.
Thus `false,false,true,true,false` partitions five candidates into `[1,2,3]`,
`[4]`, `[5]`. An empty cohort produces no group. The boundary is evaluated for
every selected candidate, including the last, even though the last boundary
cannot change the partition.

Use the [same admission, sorting and window-bound phases as StartingWith](owned-group-starting-with.md#boundaries-and-callback-order):
authenticate the full cohort, sort as configured and evaluate parent bounds
in stored order, even for empty input. Unsorted FilterThenSort adds no
eager predicate pass.

For each reached candidate, any remaining selection predicate runs first, then
the end boundary, then member retention. Only selected candidates close groups;
a filtered-out candidate's boundary is never evaluated. Boundary and retention
callbacks observe candidate positions. A non-Boolean boundary reports the
existing typed Boolean error at the original predicate node. A boundary failure
precedes a retention failure at that candidate.

Member retention evaluates every reached member and keeps a whole constructed
group when any member succeeds. Dropping a group does not move its members into
a neighbor or reopen a closed group. Windows select retained groups afterward
and assign compact one-based group ordinals. Consequently, `First(0)` does not
hide boundary or retention errors. Only selected groups evaluate bindings and
children; global failures retain their earlier precedence.

## Owner identity and members

The [group-member rules](source-ownership.md#group-member-occurrence-identity)
apply: first member for scalar reads, compact group ordinal for positions.
A nested Ending walk enumerates only this group's members. Same-path owners
still evaluate their own boundaries independently.

## Runtime and resource contract

Rust exposes `BasicGroupEndingWithControls` and
`map_group_ending_with_source_cohort`. C# exposes
`IFerruleBasicGroupEndingWithControls` and `MapGroupEndingWithSourceCohort`.
Their end callback returns a Boolean. Both reuse the unchanged six-field
`BasicGroupByMetadata`/`FerruleBasicGroupByMetadata` describing static phases.

The paired implementation shares the By/Adjacent/Starting preallocated buckets,
member links, templates and output backing. The only additional state is one
local closed-group flag. The existing n-key backing remains reserved and
Null-filled; Boolean boundaries consume no grouping-key text credits. Published
C# contexts and member arrays remain immutable.

The interpreter keeps its separate WorkBudget accounting. Native tracing records
EndingWith and its predicate node with no grouping key, and records the
GroupEnding filter phase before each member's retention decision.

## Qualification

The [EndingWith corpus](../../conformance/basic-owned-group-ending-with/)
separates hand-authored trees/errors, rejected Projects/Programs and the
legacy saved project. Interpreter and emission checks do not invoke the
ignored generated hosts. To run those with Cargo and .NET 10:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_ending444_generated_rust_and_csharp_match_expected -- --ignored
```
