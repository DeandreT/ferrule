# Scalar sequence saved-design interchange

This design is the bounded deliverable for [#203](https://github.com/DeandreT/ferrule/issues/203). It inventories a possible saved-design representation of the existing scalar `filter_map_v1` operation. It changes no importer, exporter, model, engine, backend, or GUI code.

**No `filter_map_v1` saved-design form is qualified by this proposal.** Ordinary external create/save/reopen and external execution comparisons are UNRUN. The current exporter must retain `MfdError::UnsupportedSequenceComposition { item }` for every such descriptor, including an unselected or otherwise valid one. XML parsing, a warning-free preflight, or a successful Ferrule reimport cannot establish behavior in another authoring application.

## Evidence and statuses

The inventory comes from repository source only. No external designs or installation material informed the synthetic literals. The source snapshot includes the current declared-ancestor ownership implementation for nested cloned targets; this design does not replace or flatten it.

Statuses have distinct meanings:

- **Source path exists** means a current Ferrule importer/exporter branch represents a legacy construct. It is not a new save/reopen or external execution observation.
- **Candidate, UNVERIFIED** means native components or scalar interfaces suggest an experiment, but complete descriptor and execution preservation has not been shown.
- **Rejected** means current admission refuses the form, or this proposal excludes it until an explicit representation is justified. It never means that a failed external experiment has been observed here.

Every new `filter_map_v1` cell below is currently **Rejected by the exporter**, regardless of the tentative native vocabulary.

| Cell | Source evidence | Interchange status |
| --- | --- | --- |
| Legacy inclusive Generate, explicit bounds | `export/node.rs` emits `core/generate-sequence` with lower/upper ordered pins; `import.rs::sequence_expr` reconstructs `Generate` | Source path exists; fresh save/reopen UNRUN |
| Legacy omitted lower bound | Export leaves the lower connection absent; import retains optional `from`; native engine defaults to 1 | Source path exists; disconnected-input default in the authoring application UNVERIFIED |
| Reversed or absent/null bounds | Native engine yields an empty sequence, with existing expression short circuits | Source result fixed; external empty/null/coercion/error order UNVERIFIED |
| Dynamic bounds and bound errors | Both bound expressions complete before coercion, except an absent bound short circuit | Candidate UNVERIFIED; a later upper expression error must precede an earlier lower coercion error |
| Primitive item cap | Existing Generate refuses more than 1,000,000 items before materialization | Native invariant; no inspected saved component establishes that same refusal |
| Predicate scalar interface | `export/udf.rs` retains ordered typed input ports and scalar output; filter has true/false outputs | Vocabulary exists; invocation order, exact Bool boundary and descriptor reconstruction UNVERIFIED |
| Total predicate constants/arithmetic | Existing UDF scalar bodies admit ordinary constants/calls | Small candidate probes only; this does not admit a transform |
| Predicate/mapper `If` and nested scalar UDF | Export emits lazy conditional and nested UDF wires with cycle/depth admission | Interfaces exist; execution, argument adaptation order, and transitive stage identities UNVERIFIED |
| Reached or unselected `Raise` in a stage | Filter/map admits lazy Raise; current scalar UDF exporter rejects that node as context-sensitive or non-scalar | Rejected by current UDF export and by the transform guard; no substitution with an empty value |
| Zero ordered captures | Stage signature still has item and original source position | Candidate UNVERIFIED |
| One or multiple parent captures | Native evaluates once, in declaration order, checking each exact tag before the next capture | Candidate UNVERIFIED; ordinary fan-out wires do not prove once-only evaluation |
| Captures on an empty range | Native still evaluates captures after an empty admitted source | Required distinction; UNVERIFIED in saved-design form |
| Parent Position capture | Resolves in its actual parent document/frame before stages | Candidate UNVERIFIED; cannot replace with an inner or compacted position |
| Private or foreign owner capture | Model/native lexical admission forbids either private owner in captures/source bounds | Rejected; no implicit context fallback |
| Mapper exact Int/Bool/String/finite Float | Ordinary return adapter precedes strict tag/finiteness checks | Scalar ports exist; complete strict boundary and original causes UNVERIFIED |
| Null/JsonNull/XmlNil or nonfinite stage output | Native rejects it with the original adapted value or binary64 bits | Rejected as successful output; saved null/absent collapse is insufficient |
| Source owner versus output owner | Two different uniquely owned unframed empty-path source IDs; only output is downstream-visible | Required descriptor relation; current native wire vocabulary has no qualified two-owner reconstruction |
| Original source position | Stages receive 1-based pre-filter index; it is not the generated integer when lower is not 1 | Candidate UNVERIFIED |
| Dense output position | Consumer sees 1-based position of the completed mapped sequence | Candidate UNVERIFIED; cannot wire the original source position to it |
| Generated scope rows | Legacy export wires producer to a row-shaped target entry and positions to its iteration stage | Source path exists for legacy; transformed scalar row/target reconstruction UNVERIFIED |
| Exists consumer | Legacy import recognizes a filter whose value feed is the direct generator and whose predicate depends on it | Source path exists for that legacy shape; transformed/mapped input is not a supported new producer |
| ItemAt consumer | Legacy import requires the direct generated producer; index is a parent expression | Source path exists for direct legacy form; filtered/mapped input and eager transform-before-index behavior UNVERIFIED |
| Aggregate consumer | Legacy import recognizes one generated dependency with an optional filter and computed value expression | Source path exists; it reconstructs a legacy reducer, not a two-owner filter/map descriptor |
| Eager completion before any consumer | Filter/map fully generates, captures, predicates and maps before Exists, ItemAt or Aggregate begins | Required; pulling only an early match/item is not equivalent |
| Shared source/work limits and cancellation | Run-wide native state reserves atomically and checks synchronous boundaries before charged work | No inspected native saved-design component carries this host contract; remain Rejected until a faithful policy is established |
| Repeated invocations or two consumers | Each reached consumer evaluates a fresh descriptor; run controls remain shared | Candidate UNVERIFIED; neither memoization nor duplicate per-item capture evaluation is allowed |
| FailureIteration::Sequence with transform | Initial model contract explicitly refuses this consumer | Rejected; legacy failure generators unchanged |
| Other generators, nested transforms, sorting/grouping/joins or structured stage values | Outside the initial transform's admitted source/body/output domain | Rejected; not widened by interchange |
| Nested cloned target branches | Current importer uses declared target lineage and exact binding ownership, preserving placeholder slots | Existing ownership source remains authoritative; copied target/scope interchange is outside these probes |

The relevant source paths are [`export.rs`](../../crates/mfd/src/export.rs), [`export/node.rs`](../../crates/mfd/src/export/node.rs), [`export/udf.rs`](../../crates/mfd/src/export/udf.rs), [`export/position.rs`](../../crates/mfd/src/export/position.rs), [`import.rs`](../../crates/mfd/src/import.rs), [`import/sequence_scalar.rs`](../../crates/mfd/src/import/sequence_scalar.rs), [`import/udf/scalar.rs`](../../crates/mfd/src/import/udf/scalar.rs), and [`import/target_iteration/branch_owners.rs`](../../crates/mfd/src/import/target_iteration/branch_owners.rs). The exact execution contract is in [`engine/filter_map.rs`](../../crates/engine/src/filter_map.rs) and the [ordered sequence design](scalar-sequence-composition.md). Historical comments or proposal prose saying implementation is unsupported do not override the current reached execution code.

## Frozen synthetic controls

The [24 complete descriptor recipes](fixtures/scalar-sequence-saved-design203.json) contain seven legacy controls and seventeen filter/map controls. They freeze the complete parent node table, source/output owners, stage FunctionIds, ordered typed parameter IDs, stage bodies and transitive callees, captures, consumer expressions, parent frame identity, and independently chosen typed outcomes or error envelopes. They were frozen as descriptor/materialization recipes, rather than serialized Projects or saved-design XML files. The bounded public tests below materialize complete Projects and inputs from them. External create/save/reopen and external execution remain UNRUN.

The generated integers deliberately start at 3. The position control generates `[3,4,5,6,7]`, captures parent position 7 once, retains items greater than 4, and maps `item * 10 + source_position + capture`. Its values are `[60,71,82]`, stage positions `[3,4,5]`, and consumer positions `[1,2,3]`. Equal numeric values elsewhere do not establish frame or owner identity.

The parent-position recipe’s `root_shell` is a descriptive frozen annotation, not the serialized Project used by the public materializer. That materializer supplies seven ordinary `Parents` rows; only the seventh contains one empty `Driver` child. Its multihop `Parents/Driver` iteration has no filter, sort or window, preserving `Parents` position 7 and `Driver` position 1. The capture therefore reads ordinary parent Position 7, and the frozen values remain `[60,71,82]`.

The controls distinguish a false predicate suppressing Raise; a late mapper Raise before Exists or ItemAt can return an earlier value; captures on an empty range; first-capture tag refusal before a later Raise; upper expression failure before lower coercion; atomic source reservation and cancellation; nested all-argument evaluation before adaptation; exact four scalar output tags; and a strict Null-output refusal. The repeated-row and scalar target shells have no unrelated sorting, grouping, filtering, failure rules or named targets. The parent-position case describes its parent invocation explicitly; the public-run materializer must preserve that frame, not silently substitute a constant or compacted Position.

Literal typed expectations are written before any saved-design candidate. Complete unexpected successes, values, origin tags/payloads, binary64 bits, errors and ordered cause chains must be retained before comparison. These recipes freeze stage entry/result order, not every internal debug-hook event. A diagnostic prefix is evidence of work already done, never a successful partial sequence.

## Bounded public qualification

The [ignored public qualification tests](../../crates/mfd/tests/scalar_sequence_saved_design203.rs)
use the complete [descriptor recipes](../../crates/mfd/tests/fixtures/scalar_sequence_saved_design203_recipes.json)
and [independent public expectations](../../crates/mfd/tests/fixtures/scalar_sequence_saved_design203_expected.json).
The tests keep materialization and execution as separate gates:

- `materialize_frozen_scalar_sequence_saved_design203_public_projects` constructs
  all 24 Projects and input shells, retains complete public codec results, checks
  exact re-encoding, and compares independently constructed typed inputs. It
  performs no engine or MFD export calls.
- `qualify_frozen_scalar_sequence_saved_design203_public_recipes` makes 24 native
  engine calls and compares complete typed results or original typed errors and
  causes. It also checks the available public trace and cancellation-checker
  observations against the frozen expectations. Complete Project, input, result
  and observation originals are retained before comparisons.
- For each of the 17 filter/map descriptors, that second test separately checks
  preflight, Ferrule-extension export and `NativeMfd` export: 51 controls must
  return `MfdError::UnsupportedSequenceComposition { item: 111 }` and publish no
  artifacts. The seven legacy controls are native baselines; this gate does not
  claim fresh legacy export or external save/reopen qualification.

Both tests are ignored by default. Run them explicitly with a fresh, absent,
normalized absolute `FERRULE_SAVED_DESIGN203_EVIDENCE_DIR`, using a distinct
output directory for each gate. A complete public qualification requires all
24 native comparisons and all 51 typed refusal controls to pass; a partial or
failed run does not qualify the cohort. The saved originals remain the evidence,
not a summary flag alone.

Public previews and checker boundaries do not expose every unused raw stage
value, internal counter or intermediate sequence. Internal unit observations in
[#253](https://github.com/DeandreT/ferrule/issues/253) and ordinary external
create/save/reopen in [#263](https://github.com/DeandreT/ferrule/issues/263) remain
separate, UNRUN qualification work. Native engine agreement and typed export
refusal establish neither an external saved-design representation nor native
MFD interoperability.

## Minimum experiment before changing interchange

A prospective probe may use the existing native vocabulary: Generate; a scalar predicate UDF with ordered `[item, source_position, captures...]` ports; the true filter output; a separate scalar mapper UDF; then one consumer. This is an experiment topology, not a chosen faithful representation. In particular, mapper-before-filter wires, extra source-position feeds, and ordinary parent capture fan-out can have different sequencing or alignment.

Begin with the constant-true identity recipe and the legacy controls. In an ordinary authoring application, create each small design through visible controls, save it, close it, reopen it, save a second complete copy, and retain both complete files and all siblings. Observe actual parameter order, datatype declarations, library/function identities, pin directions and true/false output selection. Never invent unseen control coordinates or infer a component's evaluation policy from its name.

Compare both complete saved component/pin/edge structures against the frozen recipe under an explicit bijection. Numeric IDs may be newly allocated on reopen, but the two private owners must remain distinct; ordered signatures, capture identity, parent frame/document, selected consumer and all reference relations must be preserved. No lossy normalization may remove an error branch, unused admitted stage parameter, capture, source position, output position or consumer edge. An existing importer returning a legacy reducer is not exact `filter_map_v1` reconstruction.

Then compare complete executable outputs and original errors for the positive and distinguishing controls. Observe the empty-range capture and eager-late-error cases before admitting captures or eager consumers. Retain actual originals before comparison; establish a complete public-run materializer separately before comparing native execution. Save/reopen alone is descriptor preservation, not execution qualification, and parsing alone establishes neither.

The host item/work/cancellation contract remains a blocker to general native interchange even if small constant examples agree. No new policy opt-out, weaker host limit, hidden custom component, or metadata-only success claim is introduced here. A Ferrule-specific annotation that only Ferrule reads would be a distinct extension design and cannot be described as ordinary native interoperability; it is outside this proposal.

## Admission, refusals and follow-on ownership

No production admission is recommended yet. Preserve the existing global typed transform refusal before artifact preparation in both compatibility and native export entry points. Preserve legacy import/export behavior and current nested-clone ownership. Do not fall back to a bare Generate, scalar map, legacy reducer, empty result, warning-only dropped filter, or partial design.

No importer/exporter implementation issue is justified by this source-only inventory. After reviewed ordinary save/reopen and complete execution evidence establish a faithful bounded form, the minimum disjoint follow-ons would be:

1. A focused importer recognizer in a new sequence-composition module, with a small fallible call seam in the graph/sequence importer. It would require a unique complete topology, exact stage/capture signatures and transitive UDFs, explicit source/output owner reconstruction, and typed refusal for ambiguous or unsupported forms. It must preserve declared target lineage and allocate no accidental shared private owners.
2. A focused exporter planner/renderer in a new sequence-composition module, with the existing preflight refusal replaced only for the qualified form. Preflight must finish before any siblings/design are published; unrelated forms retain the same typed refusal. It depends on the reviewed representation and the importer round-trip controls, with no shared module edits in parallel.

These are conditional file-ownership boundaries, not created issues or authorization to implement them. If the experiment cannot preserve captures, positions, strict/error order, eager completion or host controls, document that finite limitation and keep refusal; do not broaden the feature to manufacture compatibility. Engine, model, generated backends, GUI authoring, generic interchange, annotations and JSON5 remain outside this design.

- [x] Inventory source vocabulary and current typed refusals.
- [x] Freeze synthetic descriptors, identities, ordering, positions and logical outcomes.
- [x] Independently review this design and complete literals.
- [x] Materialize all 24 complete public Projects and input shells.
- [x] Qualify all 24 native comparisons and 51 separate typed export-refusal controls.
- [ ] Perform ordinary create/save/reopen and complete descriptor/execution comparisons.
- [ ] Establish any faithful supported form and its limits.
- [ ] Create small implementation issues only after that evidence and review.
