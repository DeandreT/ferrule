# #443 pinned APIs, unchanged backing and future disjoint ownership

Bound to actual merge `f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971`, tree `2748a46873ef858c9c498d49f8d865e005b3366a`. All additions below are **required future shapes**, not implemented APIs. The accepted v2 contract and v1 MINIMAL-CONTROLS govern without amendment.

## 1. Exactly six generation companions

```rust
// codegen, reexported through its existing lib.rs
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

Do not add execution entries, a public mode/permission/configuration object, DTO/GroupingPlan fields, a renamed public metadata type, or mandatory methods on old keyed controls. Generated Rust `execute_with_basic_owned_sources` / `execute_selected_with_basic_owned_sources`, C# `ExecuteWithBasicOwnedSources` / `ExecuteSelectedWithBasicOwnedSources`, five explicit policy fields, and selected-target failure precedence remain unchanged.

New target profile = explicit `{By,AdjacentBy,StartingWith}` with existing filters/sort/windows/primary-row failures, independently paired with `BasicScalarProducers`. Old wrappers keep their exact original pairs. Exhaustive allowlists must reject Ending/Blocks. Common private scoped-group helper renames are allowed, not an `any grouping` condition. Rust `Profile::GroupStartingWith` and C# `TargetControls::ByAndAdjacentAndStarting` / new TargetExtension must select this pair first for validation, catalogue, mapping route, retained inventory, scope marking and emission. Eligible ItemAt does not open siblings or inherited closed contexts or owned failures.

## 2. Exactly one bool controls/operation pair in each runtime

Reuse the existing six metadata fields unchanged: Rust `directions`, `filter_before_sort`, `explicit_window_count`, `implicit_first`, `has_filter`, `has_post_filter`; C# `Descending`, `FilterBeforeSort`, `ExplicitWindowCount`, `ImplicitFirst`, `HasFilter`, `HasPostFilter`. Only their shared-operation documentation expands.

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

Export the Rust trait through existing `context/basic.rs:27`, `context/mod.rs:27-31`, `src/lib.rs:39...` alongside the unchanged keyed controls/metadata.

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

Keep C# in existing `FerruleBasicGroupBy.cs`, preserving75 embedded runtime paths /78 ordinary package paths. Existing By/Adjacent public signatures stay unchanged. Starting generated controls use `require_bool(predicateNode, value)` / `FerruleFunctions.RequireBoolean(value,predicateNode)` before returning bool, preserving precise NotABool node/cause. They must not expose `group_key()->Value` as the new public boundary. Preserve old keyed generated names/bytes; only Starting emits its new bool controls class/trait and operation.

Private three-case dispatch may share preparation. Rust stack callback adapters may return private `Key(Value)|Start(bool)` locals without retained copies. C# can borrow either interface via parameters/private overloads and a private enum; no new retained adapter object, boxed struct, field array, vector or extra retained operation fields. No Starting key text admission, equality or population of the reserved n-key backing. No runtime predicate NodeId is invented.

## 3. Complete private GroupMembers rename and mint/copy/use map

| Current private spelling -> required spelling | Merge-bound constructors, uses and copy paths |
|---|---|
| Native `OccurrenceKind::KeyedGroup` -> `GroupMembers` | Definition `owned_context.rs:426-445`; mint `eval_scope.rs:1848-1853`; representative `:445`, exact root pin `owned_context.rs:514-518`; trace `trace.rs:1368`. `active_identity` at `owned_context.rs:608-615` must remain Item-only. |
| Native `SourceOriginKind::KeyedGroupWrapper/Members` -> `GroupWrapper/GroupMembers` | Definitions `owned_context.rs:450-455`; actual wrapper redirect `:628`; mints `eval_scope.rs:1866-1878`. All frame/origin clones preserve the enum and actual object identity. |
| Common `Ancestor::KeyedGroup` -> `GroupMembers` | `validate/owned_sources.rs:310`, owns `:325`, explicit paired mint `:807`; full `Visit` namespace/closed/scope/ancestry/node identity remains. Rename `supports_keyed_groups` / `permits_keyed_group` honestly if shared, while retaining explicit per-mode gates at all occurrences. |
| Rust `OwnedOccurrenceKind::KeyedGroup` -> `GroupMembers` | Definition `context/mod.rs:732-735`; mint `basic_group_by.rs:381-386`; exact empty-root resolver `resolve.rs:705-728`; ordinary identity gate `walk.rs:884-907` remains Item-only; owned/selection/frame clones preserve kind. |
| Rust `OriginKind::KeyedGroupWrapper/Members` -> `GroupWrapper/GroupMembers` | `basic.rs:501-502`, bridge path preflight `:837`, actual legacy redirect `:1883`; mint `basic_group_by.rs:396-416`; template prefix and selection clones `:327-350,422-433`; rank changes only indices `:731-739`. |
| C# `Occurrence/Observation.KeyedGroup` -> `GroupMembers` | `ScopeContext.cs:1725-1727`, `FrameAt:1739`, latest exact-root resolver `:1790-1796`, copies `:1889`, Item identity exclusion `:1918`; template prefix/selection copies `FerruleBasicGroupBy.cs:148,266`, mint `:204`; `with` rank updates preserve marker. |
| C# `BasicOriginKind.KeyedGroupWrapper/Members` -> `GroupWrapper/GroupMembers` | Enum `ScopeContext.cs:2290`; actual legacy bridge `:2937,2985-2986`; mints `FerruleBasicGroupBy.cs:198,202`; origin path copies preserve kind. |
| C# `CollectionIdentity.BasicKeyedGroup` -> `BasicGroupMembers` | Private record `ScopeContext.cs:1214`; authenticated grouped bridge acceptance `:2314`; mint `FerruleBasicGroupBy.cs:205`; collection association clones `:159,275`, in-place prepaid rank operation `:64-70` preserve flag. Ordinary Grouped data remains rejected. |
| C# `FromBasicKeyedGroupMembers/Wrapper` -> `FromBasicGroupMembers/Wrapper` | Internal factories `FerruleInstance.cs:135,170`; calls `FerruleBasicGroupBy.cs:197,573`. Keep Claim/ConsumeMembers/ConsumeWrapper/Finish one-shot template capability and immutable arrays; names do not grant constructor access. |

Only three explicitly admitted, authenticated paired operations may mint these markers. They are not public context kinds, generic Grouped authorization, ancestry inheritance shortcuts, or schema/payload-equality guesses. Nonempty C does not grant root access. Exact owner availability precedes pin resolution; newest ordinary same-owner [] member shadows group; foreign member does not. Selecting a real grouped C enumerates members, ordinary Item selecting itself stays identity. First-prefix behavior, raw legacy Alias versus canonical C=[], and strict ordinary descendant repetitions remain unchanged.

Old test references needing mechanical rename, without changing expected values/meaning: native `tests/owned_group_by.rs:1261`; Rust `basic_group_by_tests.rs:2008-2012`, `basic_group_adjacent_by_tests.rs:1092`; C# `BasicOwnershipGroupByTests.cs:762-763`, `BasicOwnershipGroupAdjacentByTests.cs:100,396-397`. The respective implementation lane owns these files, avoiding a separate cross-cutting rename agent.

## 4. Exact unchanged retained layout and equations

Rust actual backing: `Prepared` at `basic_group_by.rs:457-471`; admission `:474-558`; allocation `:559-590`; immutable/local result construction `:794-815`. C# backing `FerruleBasicGroupByOperation:287-321`; Seed `:343-367`; Footprint `:448-485`; publication `:561-578`; template copy costs `:211-278`. No source drift was found in any of these files.

For n original candidates, k sort keys, h explicit+implicit windows, s=1 iff sorting, a wrapped templates:

| Retained item | Exact admitted cells/records |
|---|---|
| Direction snapshot / sort cache / windows | k / nk /3h |
| Order, member-next, buckets, key cells, output cells | 5n, **including n Null key cells retained on Starting** |
| Stable merge | sn |
| Terminal plans | n |
| Payload worst case | Rust n+a; C#3n+4a (member references plus immutable payload/view publications and wrappers) |
| Templates and sorted views | Sum T plus s Sum U below; no template reused or new ticket per run |
| Fixed operation records | 3, plus separately seeded capacities/records below |

```text
fixed_R = k+nk+3h+(7+s)n+a+3 + sum(T_R) + s*sum(U_R)
fixed_C = k+nk+3h+(9+s)n+4a+3 + sum(T_C) + s*sum(U_C)
T_R=2+(o+1)+(l+1+w)+(q+1+w)+j
T_C=7+(o+1)+(l+1+w)+(q+1+w)+2(t+1)+j
U_R=2+o'+l'+q'+j'
U_C=6+o'+l'+q'+2t'+j'
```

o/l/q/t = owned/legacy/origin/collection prefix counts, w=wrapper flag; j counts every copied path cell. Primed counts are the full candidate for a sorted view, not just prefix. C# collection associations count reference+object; every actual canonical/raw path and wrapper name copy contributes known UTF8. Add root/receiving-parent/full original cohort seed, retained spare capacities and **exactly two C# ledger setup records** once, never per group. C# `LedgerSetupRecords=2:78`, admitted before both OwnedCounter objects at Seed `:348-351`; Rust spare counts `prepare:496-517`. Sort keys charge every returned string, including duplicates, at callback success -> count -> admit -> cache. Keyed By/Adjacent retain their existing key-return/before-post frontier. Native retains its own WorkBudget and retained-key frontier; do not impose Rust/C# counts on it.

At policy roots R, frames F, path segments P, metadata bytes B, walk W, unchanged checked ceilings are:

```text
Q=P+1; L=2F+2P+4; J=FP+FQ+LP
H_R=2+F+2L+J; H_C=7+3F+2L+J
M_R=E_R+2W*H_R+9W+3
M_C=E_C+W*(H_C+(H_C-1))+14W+5 = E_C+2W*H_C+13W+5

K=3W; A=F+P+1; N=K+1; D=2P+2; T=2(F+1); AF=KF+2F
C=2R+1+AF+AFP+T+K+D
m=2NL+NLP+NF+NF(P+1)+5R+2A(L+F+2)(P+2)+2N
E_R=C+m+D+W+5
E_C=(C+2K+F+FP+T)+3m+D+(K+1+4N+A+4)+2W+5
```

Do not replace the existing checked intermediate order with merely algebraically equivalent unchecked expressions. Preserve native element/Vec and C# Array.MaxLength/element-size representability checks, original overflow fields and slot-before-known-text precedence before allocations/semantics. Original witness refusal precedes metadata in Rust, but private malformed terminal is after metadata; C# terminal validation is in Seed before metadata.

Starting uses the same arrays/templates and at most n disjoint nonempty runs whose member counts sum<=n. Each included run claims its distinct first candidate template. A bool boundary is local and adds **zero group-key UTF8 bytes**, never compares/stores a group key. Existing C# `FerruleValue.Null` is default, so unchanged default-initialized n keys are Null; Rust explicitly initializes `vec![Value::Null;n]`. No boundary vector, retained adapter, new per-run record, or replacement backing is admitted by this proof.

Numeric source-only recipe locks (not observed #443 results): R09 known bytes32 or64 without key text; R10=38+28+144=210, capacity5 error used39/request172/max210; R11 slots425 versus426 preserves slot-before-known-text; R12 C# Seed90/64 + footprint177/142 =267/206, ceiling3637, k1126 refusal90/3549/3637, ledger limit1 refuses0/2/1; R13 known206+sort8=214, not222, B213 fails212/2/213 at fourth sort return, before boundaries. All accepted R01-R16 remain required including every old keyed threshold and callback/error frontier.

The existing hash-unchanged Rust adjacent fixture's `:672` comment still says26 even though its `:674` assertion and full arithmetic are210. Treat that as a stale annotation, not an oracle; #443's fixed subtotal is explicitly28. No correction to an old test or threshold is performed by this reconciliation.

## 5. Precise future implementation ownership (no current assignments)

The parent grants one owner per row. Every named existing file belongs to exactly one lane below. New directories/files have the indicated exclusive owner. No agent may claim another row to perform a cross-cutting rename, registration or helper refactor; send a bounded request to that owner. The accepted old contracts, manifests and historical artifact bodies are immutable.

| Lane | Exclusive future write paths | Handoff / constraints |
|---|---|---|
| N — native | `crates/engine/src/{source_ownership.rs,eval_scope.rs,owned_context.rs,trace.rs,lib.rs}`, new `crates/engine/src/tests/owned_group_starting_with.rs`, mechanical marker reference in existing `tests/owned_group_by.rs` | Own native private rename, explicit three-mode admission/ancestry/trace, boundary-before-post, native accounting and registration. Ordinary validation/walker files are read-only absent a concrete reviewed defect and expanded grant. |
| C — common | `crates/codegen/src/{lib.rs,lower.rs,owned_admission.rs,tests.rs}`, `validate/{mod.rs,owned_sources.rs}`, new `tests/owned_group_starting_with.rs` | Four public companions/exports, exact profile+consumer pair, phase/context visits, full admission catalogues. Existing representation/structural validators need no anticipated edit. |
| RR — Rust runtime | `crates/codegen-runtime/src/lib.rs`, `context/{basic_group_by.rs,basic.rs,mod.rs,resolve.rs,walk.rs,basic_group_by_tests.rs,basic_group_adjacent_by_tests.rs}`, new `context/basic_group_starting_with_tests.rs` | Bool trait/operation/export chain; private kind/origin rename and copy/use closure; independent retained-layout/callback/resource receipts; own any new test registration in its existing modules. Old expected thresholds unchanged. |
| CR — C# runtime | `runtime/csharp/Ferrule.Runtime/{FerruleBasicGroupBy.cs,ScopeContext.cs,FerruleInstance.cs}`, smoke `Program.cs`, `BasicOwnershipGroupByTests.cs`, `BasicOwnershipGroupAdjacentByTests.cs`, new `BasicOwnershipGroupStartingWithTests.cs` | Exactly the provisionally three changed embedded runtime bodies; no new file/adapter/retained field. Immutable contexts; own private rename and smoke registration. This ownership is not runtime-delta approval. |
| RE — Rust emitter | `crates/codegen-rust/src/{lib.rs,owned_basic.rs,owned_basic_windows.rs,owned_basic_group_by.rs,tests.rs}`, new `tests/owned_basic_group_starting_with.rs` | Two-mode historical generated bytes preserved within their old profiles; bool controls only for new mode; all profile/catalogue/render/scope-marking matches updated together. |
| CE — C# emitter | `crates/codegen-csharp/src/{lib.rs,owned_basic.rs,mapping.rs}`, `mapping/owned_group_by.rs`, `tests/owned_basic.rs`, new `tests/owned_group_starting_with.rs` | New pair first, exact three-mode routing/marking/inventory; Starting bool RequireBoolean/class; no arbitrary string rewriting. `runtime.rs` remains read-only inventory authority. |
| F — finite corpus/oracle | New `conformance/basic-owned-group-starting-with/` including Projects/cases/closed controls, authored source/input/expected JSON and its dedicated historical-baseline package | Freeze literal products first; S23 isolated, X10 identity distinction, no generated actual as oracle. Own all files in this new corpus directory, including later copying an accepted historical baseline package into it. No old #441/#473/#442 fixture/manifest/receipt edits. |
| H — shared harness/baseline integration (parent-coordinated) | `crates/cli/tests/code_generation.rs`, new `crates/cli/tests/code_generation/basic_owned_group_starting_with.rs` and same-named host-fixture directory; narrowly reviewed extension to `legacy_artifact_policy.rs`; durable `docs/design/owned-group-starting-with.md`, `docs/design/source-ownership.md`, `docs/code-generation.md`, shared wording/links only in existing By/Adjacent docs if needed | Own root registration, complete source closures, old-profile checks, historical source capture and new reviewed-delta policy. Borrow existing supported codec/native/host-policy helpers unchanged. No automatic ownership of old #473/#442 harness or helpers. All compiler/execution lanes remain parent-serialized under separate authorization. |

Tests not expressly named above remain read-only. Adding a fourth C# runtime source, changing artifact inventory/metadata/backing, modifying old receipts, expanding common harness ownership, or discovering a new seam outside these rows requires a fresh explicit grant and relevant independent design/delta review. Mapping DTO/schema/native-vendor/GUI pipelines, dependency installation and CI definitions are outside scope.

Pre-edit sequencing: independent reconciliation review -> finite literal/source/baseline freeze -> explicit owner grants. Freeze source closures before any authorized preparation/emission and review actual bool controls/layout deltas before compiled public consumers. Components exchange the pinned APIs, not shared file ownership. Parent integrates and validates the exact final head; this file alone starts no implementation or actor.
