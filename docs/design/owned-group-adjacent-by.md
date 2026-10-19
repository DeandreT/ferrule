# Owned adjacent grouping

Owned `group_adjacent_by` partitions the selected source candidates into contiguous
runs. It shares the source-ownership and paired-cohort contract of
[owned By grouping](owned-group-by.md), while using a separate operation and
generation profile. The mapping model's existing adjacent key and scope fields
are sufficient; no new serialized ownership or grouping fields are needed.

## Explicit profile

Use `codegen::lower_with_basic_owned_group_adjacent_by`, followed by
`codegen_rust::emit_with_basic_owned_group_adjacent_by(&program, &options)` or
`codegen_csharp::emit_with_basic_owned_group_adjacent_by(&program)`. Direct Program
validation uses `validate_program_with_basic_owned_group_adjacent_by` and
`validate_basic_owned_group_adjacent_by_emission`; named-input analysis uses
`legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by`.

The profile admits static Basic By and AdjacentBy scopes with the existing
filter, sort, window, member-retention and primary-row-failure controls. Its
separate scalar-consumer permission includes the four-producer
[parent-context ItemAt contract](owned-sequence-item-at.md). ItemAt remains
ineligible in owned failure predicates and messages. Generated libraries retain
their typed Basic execution entries and mandatory five-field policy.

Default, By, ItemAt and earlier generation profiles retain their own admission
rules; they do not acquire owned AdjacentBy through this companion. Starting,
ending and fixed-block owned grouping, specialized contexts and grouped observer
snapshots retain separate capability gates. Entirely ordinary legacy subtrees
remain on their ordinary routes.

## Partitioning and phases

Compare a candidate's key only with the last constructed run. Equality is exact
tagged scalar equality: integers and floats differ, Null and JsonNull differ,
signed floating zeros compare equal, and NaN does not equal itself. Strings use
exact text equality. `A,B,A` therefore forms three runs.

Selection filtering and stable sorting establish candidate order before run
partitioning. Removing the B candidate before grouping may turn `A,B,A` into one
A run. Post-group controls instead evaluate once for every reached member and
retain a whole run if any member succeeds. False members and the first false
representative remain in that run; later predicates are never skipped merely
because an earlier member succeeded. Dropping an all-false B run afterward leaves
two distinct A runs, not a merged group.

The paired control phases are:

1. Authenticate the complete original source cohort and admit known operation
   metadata before semantic callbacks.
2. With sorting and FilterThenSort, evaluate all selection predicates before sort
   keys. Otherwise sort keys precede the per-candidate selection phase. Sort keys
   observe structural positions; stable sorting supplies selected positions.
3. Evaluate every explicit window bound once, in stored order, in the receiving
   parent. Bounds run after sorting and before grouping keys, even for an empty
   cohort. FromTo evaluates its first bound before its last.
4. In selected order, evaluate any remaining selection predicate, then the key
   and post predicate for each retained candidate. Form or extend the last run.
   These callbacks observe candidate positions, not final group ordinals.
5. Remove unretained runs, apply the evaluated windows to the retained groups,
   and assign compact one-based group ordinals. Only selected groups evaluate
   target bindings and children.

Without sorting, FilterThenSort does not create an eager all-predicates phase.
`First(0)` does not suppress key or post errors. An earlier empty window does not
skip later bound evaluation. Global failure rules complete before target work;
bindings in dropped or window-excluded groups remain lazy.

## Owned occurrences and nested members

Each authenticated group retains the selected exact owner and absolute
endpoint-local collection. Its scalar representative is its first surviving
ordered member. Iterating that collection again enumerates only the current
run's members, with independent one-based member positions. Other owners and
incoming ancestor frames remain available under their own identities.

A run retains its first member's intermediate source prefix. If a flattened run
crosses two parent Batches, its nested members retain the first Batch's pinned
context; grouping does not reconstruct a separate original parent stack for each
member. Legacy wrapper labels remain distinct from canonical owned paths.

The latest exact-owner occurrence governs explicit pins. A synthetic group at
collection `[]` reads the immutable endpoint root for an explicit empty-frame
pin; a repeated root cannot be unwrapped as its representative. A newer ordinary
member at `[]` instead retains active-item pin behavior. A different owner's
member cannot shadow that group, and a group at a nonempty collection does not
alone create a root-pin exception. Existing declared-nonrepeating root rules
continue independently.

## Runtime and metadata contract

Rust exposes `map_group_adjacent_by_source_cohort`; C# exposes
`MapGroupAdjacentBySourceCohort`. Both accept the existing By metadata/control
record shapes. The existing By operations retain global first-seen bucket
partitioning. The shared private group-member occurrence represents an
authenticated member container, not a grant for arbitrary grouping modes.

An n-candidate cohort produces at most n runs. Adjacent partitioning shares the
complete-cohort preflight, preallocated buckets, keys, member links, templates
and output storage described in the By runtime design. It does not add a key
map, growing run list or per-group resettable budget. Retained capacity and every
known path/frame/text component remain charged. Published C# contexts and member
arrays must remain immutable when a later equal-key run is created.

Paired runtimes charge each returned string sort/group key after its callback
succeeds and before comparison, post evaluation or retention, including equal
keys. The native interpreter retains its separate WorkBudget accounting; these
representation-specific limits are not cross-engine numeric resource parity.
The five policy fields bound metadata and contexts, not source/output payload
allocation, process memory or CPU.

## Admission and evidence

Named-input discovery follows the actual receiving phase: bounds in the parent,
selection/sort/key/post in the candidate, and bindings/children in grouped or
member contexts. Owned-only inputs remain optional until reached; possible
legacy aliases remain required, dynamic slots remain unavailable, and declaration
ordinals and per-target catalogues remain complete. Invalid selected targets
retain their existing precedence over supplied-input and policy checks.

Functional qualification compares independently authored complete trees and
typed errors across the interpreter and compiled public Rust/C# consumers.
Private cohort, resource and callback-order probes remain separate evidence.
Emission, compilation and source review alone do not establish execution parity;
native export, vendor acceptance and GUI/debugging have independent gates.
