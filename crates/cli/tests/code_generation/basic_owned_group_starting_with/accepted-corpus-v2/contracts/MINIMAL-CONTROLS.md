# #443 concrete minimal API and private control contract

Proposed, unimplemented; subject to independent review after accepted #442 merge. Names follow its current companions and issue #443's existing `group_starting_with` field, read with GitHub CLI read-only. No new wire/policy/producer abstraction is needed.

## Public generation companions — six functions only

```rust
// codegen
pub fn lower_with_basic_owned_group_starting_with(project: &Project)
    -> Result<Program, LowerError>;
pub fn validate_program_with_basic_owned_group_starting_with(program: &Program)
    -> Result<(), ProgramValidationError>;
pub fn validate_basic_owned_group_starting_with_emission(program: &Program)
    -> Result<(), ProgramValidationError>;
pub fn legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
    program: &Program,
) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError>;

// codegen_rust
pub fn emit_with_basic_owned_group_starting_with(program: &Program, options: &Options)
    -> Result<ArtifactSet, EmitError>;
// codegen_csharp
pub fn emit_with_basic_owned_group_starting_with(program: &Program)
    -> Result<ArtifactSet, EmitError>;
```

Both emitters currently return `ArtifactSet` (`codegen-rust/src/lib.rs:290-305`, `codegen-csharp/src/lib.rs:168-175`); these concrete signatures retain that type. Preserve generated Rust `execute_with_basic_owned_sources` / `execute_selected_with_basic_owned_sources` and C# `ExecuteWithBasicOwnedSources` / `ExecuteSelectedWithBasicOwnedSources` (`owned_basic.rs:1128-1135`), mandatory five-field policy and exact selected-target precedence. No new execution entry or public configurable permission object.

## Runtime controls — Boolean API, unchanged metadata shape

Reuse **unchanged** `BasicGroupByMetadata` / `FerruleBasicGroupByMetadata`. Despite the historical name, every field describes shared static candidate/sort/window/post phases, not a key or partition policy. Its six fields already suffice; document its new permitted operation explicitly. No `mode`, predicate NodeId, generic policy, additional public metadata fields, renamed public type or new required method on existing By implementations.

Add exactly one narrow controls trait/interface and operation in each runtime:

```rust
pub trait BasicGroupStartingWithControls<E> {
    fn metadata(&self) -> BasicGroupByMetadata<'_>;
    fn filter(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
    fn sort_key(&mut self, key: usize, context: &BasicScopeContext<'_>) -> Result<Value, E>;
    fn window(&mut self, bound: usize, parent: &BasicScopeContext<'_>)
        -> Result<SequenceWindow, E>;
    fn starts_group(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
    fn retain_member(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
}

impl<'ctx> BasicScopeContext<'ctx> {
    pub fn map_group_starting_with_source_cohort<E, C, T>(
        &self, original: &RetainedSourceCohort<'ctx>, controls: &mut C, target: &mut T,
    ) -> Result<Vec<Instance>, E>
    where
        E: From<RuntimeError>,
        C: BasicGroupStartingWithControls<E>,
        T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>;
}
```

```csharp
public interface IFerruleBasicGroupStartingWithControls
{
    FerruleBasicGroupByMetadata Metadata();
    bool Filter(FerruleBasicScopeContext context);
    FerruleValue SortKey(int key, FerruleBasicScopeContext context);
    FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent);
    bool StartsGroup(FerruleBasicScopeContext context);
    bool RetainMember(FerruleBasicScopeContext context);
}
// On FerruleBasicScopeContext:
public IReadOnlyList<FerruleInstance> MapGroupStartingWithSourceCohort(
    FerruleRetainedSourceCohort original,
    IFerruleBasicGroupStartingWithControls controls,
    Func<FerruleBasicScopeContext, FerruleInstance> target);
```

Keep C# interface in existing `FerruleBasicGroupBy.cs`; adding a new runtime file would change the embedded 78-path ordinary artifact inventory and requires a separate reviewed inventory policy. Export the new Rust trait through existing `context/basic.rs`, `context/mod.rs`, `lib.rs` chain. Existing BasicGroupByControls, both keyed operations and all old signatures remain source-compatible and retain their exact semantics.

### Why not a public key adapter?

Taking Starting through public `group_key()->Value` would let callers return arbitrary strings/tags, hide boolean enforcement behind a historically keyed name, and tempt erroneous duplicate-key comparison/text charging. Starting takes a real bool at its public boundary. Generated controls evaluate the existing predicate expression then `require_bool(predicate_node, value)` / `FerruleFunctions.RequireBoolean(value,predicateNode)` so NotABool retains the exact source node. The runtime does not guess a node or string-coerce values.

Private adapter/shared-dispatch implementation is permitted **only without new retained storage**: Rust stack wrappers may implement an internal common callback interface returning an internal `Key(Value) | Start(bool)` local result; no clone/cache/Vec or extra retained record. C# may have two private overloads taking either interface and route to a shared seed/prepare/execute implementation with two borrowed interface parameters (one null) and a private enum, or duplicate only the callback forwarding. Do not allocate a controls-adapter class, box a new struct adapter or store new arrays/fields inside the operation under an unchanged accounting claim. An internal bool-to-Value adapter is acceptable only if confined to a private local, with Starting dispatch explicitly consuming Bool, never calling key text admission/comparison or populating n-key storage; the typed public interface remains the same.

The selected contract keeps existing n Value key backing allocated/reserved but unused and Null-filled on Starting. This preserves old footprint/thresholds and avoids a fabricated boundary vector; it does not claim an optimal minimum-memory implementation. New bool control does not consume UTF-8 key credits.

## Private mode/profile dispatch — exact closed sets

- Native execution: private `BasicGroupPartition::{By(NodeId),AdjacentBy(NodeId),StartingWith(NodeId)}` or equivalent typed choice. For Starting, use native `passes_scope_filter` with `TraceFilterPhase::GroupStarting`, then post; no keyed Value or new retained-string charge. Existing By/Adjacent exact equality and native budget frontier stay unchanged. Group trace uses existing `TraceGrouping::StartingWith { node }`, `key:None`, correct constructed-group ordinal/retention. Do not label Bool boundary a grouping key.
- Common profile: add one private variant such as `ByAndAdjacentByAndStartingWithAndWindowsAndSortAndFilter`. Rename private `supports_keyed_groups`/`permits_keyed_group` to honest scoped-group helpers if shared, but implement an exhaustive mode allowlist: By in prior keyed profiles; Adjacent only prior adjacent/new profile; Starting **only** new profile; Ending/Blocks false. Root/bounds/retained-expression/ordinary-island checks must select the same exact profile.
- Keep consumer policy **independent**: `(new target profile, BasicScalarProducers, existing primary-row-failures=true)`; prior wrapper pairs unchanged. No consumer-first fallback to old ItemAt/Adjacent validator or catalogue.
- Rust emitter: add `Profile::GroupStartingWith`; update `has_*groups`, `permits_grouping`, validation-pair match, catalogue-pair match, scope marking and render selection together. Existing paired renderer dispatches Starting to the new operation/new bool controls; By/Adjacent output must retain its old controls names/bytes for same-profile conservation. Ordinary grouped imports/routes remain ordinary.
- C# emitter: add `TargetControls::ByAndAdjacentAndStarting` and a new TargetExtension constant with the independent existing consumer policy. Validation, mapping-route selection, retained inventory and catalogue resolution dispatch the **new pair first**. `mapping.rs` route marking, operation and controls emission must all admit exactly the same three modes. Emit new `GroupStartingWithControls_<n>` only for Starting, using the bool interface. Do not text-replace user literals or old generated identifiers.
- Native/Common legacy selector: boundary-only/post-only owned demand still establishes an authenticated grouped occurrence for descendants; unrelated ordinary siblings gain no owned ancestry.

## Honest private member-container markers

Rename (private/internal only):

| Current #442 name | Proposed private name | Required propagation |
|---|---|---|
| Native OccurrenceKind::KeyedGroup | OccurrenceKind::GroupMembers | representative, exact empty-pin test, active-Item identity exclusion, native group construction |
| Native SourceOriginKind::KeyedGroupWrapper/Members | GroupWrapper/GroupMembers | actual legacy-wrapper redirect, grouping origins |
| Common Ancestor::KeyedGroup | Ancestor::GroupMembers | owns predicate, context visit ancestry, permitted paired-group minting |
| Rust OwnedOccurrenceKind::KeyedGroup | GroupMembers | representative resolution, root pin, walker identity shortcut, clones/views, finish |
| Rust OriginKind::KeyedGroupWrapper/Members | GroupWrapper/GroupMembers | legacy Basic bridge and actual wrapper/member associations |
| C# Occurrence/Observation.KeyedGroup | GroupMembers | every clone, FrameAt, observed publication, root resolver, IterateSource identity exclusion, prefix/selection copy |
| C# BasicOriginKind.KeyedGroupWrapper/Members | GroupWrapper/GroupMembers | bridge admission and actual wrapper redirects |
| C# CollectionIdentity.BasicKeyedGroup | BasicGroupMembers | only authenticated collection acceptance; clone/rank copies preserve flag |
| C# FromBasicKeyedGroupMembers/Wrapper | FromBasicGroupMembers/Wrapper | internal prepaid immutable payload factories and one-shot template capability |

These are **not** generic permission flags. Only the three explicit paired operations can create the marker, after complete source/cohort authentication and pre-admission. Ordinary Grouped/Repeated data, Ending/Blocks, name collisions and inherited “some outer grouping” never mint it. Ordinary Item remains separate, preserving latest exact-owner [] shadowing. Audit all current symbol references, including bridge/assertion tests; missed copies are semantic defects, not mere rename cleanup.

## Layout/API receipts future implementation must provide

1. Old/new public API inventory: exactly six generation companions + two runtime operation/controls pairs above; unchanged old metadata members/signatures/execution APIs and existing ArtifactSet emit product type.
2. Private marker mint/copy/use inventory, listing source functions for all three explicit constructors, representative/root selection, walker identity and legacy bridge. No unauthorized caller can obtain a marker by wrapping ordinary contexts.
3. Enumerated retained arrays/objects/capacities and known text copies before/after. Show same n-key backing retained but unused on Starting, no heap adapter or boundary vector, and existing C# setup records still exactly two. Validate numeric oracle frontiers from CONTRACT-DRAFT §7, not values read back from production Footprint.
4. Frozen old/new generated source manifests and runtime source binding under IMPLEMENTATION-SCOPE's reviewed-delta policy; controls shapes reviewed independently before public compilation/execution.
