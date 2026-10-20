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
generation profiles keep their own capability sets. Ending, fixed-block and
specialized owned grouping have separate gates.

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

By, AdjacentBy and StartingWith use the same private authenticated GroupMembers
occurrence. It identifies one exact owner and endpoint-local collection. Scalar
reads use the first surviving ordered member; position reads use the group
ordinal. Nested iteration of that collection enumerates only the current group's
members with their own one-based positions. Other owners remain independently
addressable.

A group retains its first member's intermediate source prefix, even when later
members cross parent collections. Wrapper labels do not substitute for an owned
path. The latest exact-owner occurrence governs root pins: a grouped occurrence
at `[]` reads the immutable endpoint root for an explicit empty-frame pin, while
a newer ordinary repeated-root member retains active-item pin behavior. An
unrelated owner's member cannot shadow it. See the shared
[source-ownership rules](source-ownership.md#group-member-occurrence-identity).

The marker is minted only by the three authenticated paired grouping operations.
Ordinary containers, inherited grouping and wrapper-name collisions do not grant
group-member identity. Clones, ranked frames, legacy bridges and immutable C#
publications preserve the same distinction.

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

The conformance corpus in `conformance/basic-owned-group-starting-with/` holds
105 public profiles with 381 calls whose expected complete trees and typed errors
are authored independently, plus refusal Projects, direct Programs and an
ordinary saved project. `basic_owned_group_starting_with` in the CLI
`code_generation` tests checks that every profile lowers, validates, keeps its
expected named-input catalogue and emits both backends. It also checks the
refusals, the earlier generation profiles' gates, and every interpreter call
against its expected result.

The ignored `basic_starting443_generated_rust_and_csharp_match_expected` test
builds one Rust host linking every generated model and one C# host per model,
runs all calls through the public generated APIs, and compares each result and
the unchanged inputs with the same expectations:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_starting443_generated_rust_and_csharp_match_expected -- --ignored
```

It needs Cargo and the .NET 10 SDK. Private resource and callback-order controls
in the runtime unit and C# smoke tests cover separate component obligations.
