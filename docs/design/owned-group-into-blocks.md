# Owned fixed-block grouping

Owned `group_into_blocks` chunks selected candidates into consecutive blocks of
a fixed positive size. It uses the existing mapping field and authenticated
source-cohort contract; saved-project grouping and ownership fields retain their
representation.

## Explicit generation profile

Use `codegen::lower_with_basic_owned_group_into_blocks`, followed by
`codegen_rust::emit_with_basic_owned_group_into_blocks(&program, &options)` or
`codegen_csharp::emit_with_basic_owned_group_into_blocks(&program)`. Direct
Program hosts use `validate_program_with_basic_owned_group_into_blocks` and
`validate_basic_owned_group_into_blocks_emission`; named-input discovery uses
`legacy_named_input_admission_plan_with_basic_owned_group_into_blocks`.

This profile admits static Basic By, AdjacentBy, StartingWith, EndingWith and
IntoBlocks scopes alongside the existing filter, sort, window and
member-retention controls. Scalar producer consumption and primary-row failure
eligibility remain independent permissions. The parent-context
[ItemAt contract](owned-sequence-item-at.md) still applies; ItemAt remains
unavailable in owned failure predicates and messages. Earlier generation
profiles, including the [Ending profile](owned-group-ending-with.md), keep their
own capability sets and refuse owned IntoBlocks with their existing typed
Grouping diagnostic. Specialized owned grouping has separate gates.

Generated libraries retain the existing typed Basic execution entries,
mandatory five-field policy and selected-target validation precedence. For a
design without owned IntoBlocks, the IntoBlocks profile emits the same artifacts
as the Ending profile.

## Block size

The block size is a parent expression, like a window bound. It is evaluated
once in the receiving parent after sorting and after every explicit window
bound, and before any candidate control, including for an empty cohort. A read
of the grouped collection's current item therefore has no active row and fails
with the ordinary owned-source access error.

The value converts with the item-count rules shared by windows: integers are
used directly, finite floats truncate toward zero, strings must contain an
integer, and negative counts clamp to zero. A count of zero is the typed
`InvalidBlockSize` error at the size node; Null, Boolean, non-finite and
non-integer string values are the typed `NotAnItemCount` error. Both errors
precede every candidate filter, retention and target callback.

## Partition and callback order

Selected candidates fill each block in order. A block closes when it holds the
size's number of members; the last block may be partial. Seven candidates in
blocks of two partition into `[1,2]`, `[3,4]`, `[5,6]`, `[7]`. An empty cohort
produces no block.

The operation authenticates the complete original cohort and admits known
metadata before semantic callbacks. Selection filtering and stable sorting keep
their existing phase rules. For each reached candidate, any remaining selection
predicate runs first, then member retention. Only selected candidates occupy
block slots. Retention callbacks observe candidate positions.

Member retention evaluates every reached member and keeps a whole block when any
member succeeds. Dropping a block never moves its members into a neighbor or
refills it. Windows select retained blocks afterward and assign compact one-based
group ordinals. Only selected blocks evaluate bindings and children; global
failures retain their earlier precedence.

## Owner identity and members

IntoBlocks uses the same private authenticated GroupMembers occurrence as By,
AdjacentBy, StartingWith and EndingWith. Scalar reads use the first ordered
member; position reads use the block ordinal. Nested iteration of that
collection enumerates only the current block's members with their own one-based
positions, including a nested IntoBlocks scope over those members. Other owners
remain independently addressable, so different owners with the same collection
and size path partition independently, each with its own parent-evaluated size.
Root-pin and prefix rules are the shared
[source-ownership rules](source-ownership.md#group-member-occurrence-identity).

## Runtime and resource contract

Rust exposes `BasicGroupIntoBlocksControls`, `map_group_into_blocks_source_cohort`
and `positive_block_size`. The size callback receives the parent and returns a
`NonZeroUsize`. C# exposes `IFerruleBasicGroupIntoBlocksControls` and
`MapGroupIntoBlocksSourceCohort`; its `BlockSize` callback returns a positive
`ulong`, which generated controls produce with
`FerruleSequences.PositiveBlockSize`. Both reuse the unchanged six-field
`BasicGroupByMetadata`/`FerruleBasicGroupByMetadata` describing static phases.

The paired implementation shares the By/Adjacent/Starting/Ending preallocated
buckets, member links, templates and output backing. The existing n-key backing
remains reserved and Null-filled; a block size consumes no grouping-key text
credits. Published C# contexts and member arrays remain immutable.

The interpreter keeps its separate WorkBudget accounting. Native tracing records
IntoBlocks with its size node and evaluated size and no grouping key, and records
only the PostGroupMember filter phase for each member's retention decision.

## Qualification

The conformance corpus in `conformance/basic-owned-group-into-blocks/` holds
public profiles whose expected complete trees and typed errors are written by
hand, together with refusal Projects, a direct Program refusal and an ordinary
saved project using legacy `group_into_blocks`. `basic_owned_group_into_blocks`
in the CLI `code_generation` tests checks that every profile lowers, validates,
keeps its named-input catalogue and emits both backends; that refusals and every
earlier generation profile's gate hold; that the saved project round-trips; and
that every interpreter call matches its expected result.

The ignored `basic_blocks445_generated_rust_and_csharp_match_expected` test
builds one Rust host linking every generated model and one C# host per model,
runs all calls through the public generated APIs, and compares each result and
the unchanged inputs with the same expectations:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  basic_blocks445_generated_rust_and_csharp_match_expected -- --ignored
```

It needs Cargo and the .NET 10 SDK. Runtime unit tests and the C# smoke tests
cover callback order, retention, windows and block-size storage.
