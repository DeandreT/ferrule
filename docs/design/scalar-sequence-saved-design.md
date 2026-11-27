# Scalar sequence saved-design interchange

**All 17 `filter_map_v1` transform recipes refuse MFD export**, even if the
descriptor is unselected. The [native controls](../conformance/scalar-sequence-native-controls.md)
separate original and dense positions, eager and partial consumption, and
capture failure on nonempty and empty ranges. None reproduces the whole
descriptor. The seven legacy controls retain their own admission.

## Evidence and statuses

The [Ferrule execution contract](scalar-sequence-composition.md) and the
[export guard](../../crates/mfd/src/export.rs) answer different questions.
Generate, Position and scalar UDF components exist in saved designs; that
does not connect two private owners or reproduce ordered captures and
eager errors. [Direct producer positions](generated-source-position.md)
supply an original ordinal, not a dense post-filter consumer position.

“Unverified” below describes a reference behavior or descriptor relation.
“Reject” is the Ferrule interchange decision. A saved candidate can agree
on output values while failing on datatype, owner, error order or work limits.

## Frozen synthetic controls

The [24 complete descriptor recipes](fixtures/scalar-sequence-saved-design203.json) contain seven legacy controls and seventeen filter/map controls. They freeze the complete parent node table, source/output owners, stage FunctionIds, ordered typed parameter IDs, stage bodies and transitive callees, captures, consumer expressions, parent frame identity, and independently chosen typed outcomes or error envelopes. They were frozen as descriptor/materialization recipes, rather than serialized Projects or saved-design XML files. The bounded public tests below materialize complete Projects and inputs from them. Exact external descriptor/execution qualification for these recipes remains unrun; the later limited native controls do not silently replace their types, owners or expected errors.

The generated integers deliberately start at 3. The position control generates `[3,4,5,6,7]`, captures parent position 7 once, retains items greater than 4, and maps `item * 10 + source_position + capture`. Its values are `[60,71,82]`, stage positions `[3,4,5]`, and consumer positions `[1,2,3]`. Equal numeric values elsewhere do not establish frame or owner identity.

The parent-position materializer supplies seven `Parents` rows; only the
seventh has a `Driver` child. Thus its genuine parent position is 7, not a
substituted constant or compact child index. The recipes also distinguish
lazy `Raise`, bound/capture error order, empty-range captures, all four output
tags, Null refusal, cancellation and cumulative limits.

## Bounded public qualification

The [public qualification tests](../../crates/mfd/tests/scalar_sequence_saved_design203.rs)
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

Only the materialization-only test is ignored by default. To retain evidence,
run the tests with a fresh, absent, normalized absolute
`FERRULE_SAVED_DESIGN203_EVIDENCE_DIR`, using a distinct output directory for
each gate. A complete public qualification requires all
24 native comparisons and all 51 typed refusal controls to pass; a partial or
failed run does not qualify the cohort. The saved originals remain the evidence,
not a summary flag alone.

Public previews and checker boundaries do not expose every unused raw stage
value, internal counter or intermediate sequence. Internal unit observations in
[#253](https://github.com/DeandreT/ferrule/issues/253) qualified 11 internal
controls. The separate controller checkpoint in
[#263](https://github.com/DeandreT/ferrule/issues/263) does not qualify a
filter/map topology. Native engine agreement and typed export
refusal establish neither an external saved-design representation nor native
MFD interoperability.

## Disposition by frozen recipe

Indices are the zero-based indices in the public expectations above. All 24
have Ferrule interpreter comparisons. The seven legacy recipes remain baselines,
with no change to their existing importer/exporter admission. For **every** row
from 7 through 23 the interchange decision is **reject**. “Unverified” below
describes the external evidence, not permission to export that recipe.

| Index | Recipe | What the external controls decide |
| --- | --- | --- |
| 0 | Explicit range and positions | Complete values match the decimal and generic-literal controls; neither preserves the frozen Int declaration. |
| 1 | Default lower bound | The [explicit/omitted probe pair](fixtures/native-feed-controls/README.md) preserved its distinct lower connections but produced identical values before and after reopening. This establishes the decimal control, not an Int descriptor round trip. |
| 2 | Reversed range | Empty rows observed in a transformed capture control; no exact legacy descriptor comparison. |
| 3–6 | Filtered Exists; third ItemAt; computed Sum; computed rows | Legacy source paths exist; no exact external comparison for these four recipes. |
| 7 | Identity rows | Matching values with an always-true filter hide the two positions. Repair import does not reconstruct the descriptor. |
| 8 | Source, parent and dense positions | Variable rows distinguish original and dense ordinals; the parent-position-7 capture and both owners are unverified. |
| 9 | Ordered string captures | No proof of declaration order, once-only evaluation or exact tags. |
| 10 | False suppresses mapper Raise | The current scalar UDF exporter refuses the Raise body even on the unselected path. |
| 11 | Eager before Exists | Late mapper error required; an ItemAt result cannot qualify Exists. |
| 12 | Eager before ItemAt(1) | Unguarded variable returns 42 instead of the late native error. Count guard matches that one error, not its Ferrule cause or invocation count. |
| 13 | Empty range, failing capture | Full-row candidate loses the reachable capture error on an empty range ([#1127](https://github.com/DeandreT/ferrule/pull/1127)). |
| 14 | First bad capture before later Raise | Neither order nor exact first-capture type error is established by the one-capture experiment. |
| 15–17 | Bool; Unicode String; finite Float | No exact saved/reopened typed result comparison for any of the three. |
| 18 | Null mapper result | A missing XML value would not preserve the strict mapper-result error. |
| 19 | Upper expression before lower coercion | Both expression evaluation and the original upper error are unverified. |
| 20–21 | Atomic item reservation; cancellation before reservation | No ordinary saved representation of the shared run-state boundary has been identified. |
| 22 | Nested later argument before adaptation | Pin order alone does not establish which exception reaches the caller. |
| 23 | Sum over all mapped items | The existing legacy reducer is a different descriptor; no qualified reconstruction. |

## Minimum experiment before changing interchange

Use a different topology. Start with case 7: save, reopen and compare both
complete structures under an explicit ID bijection. Keep the two owners,
ordered signatures, capture edges, consumer and parent frame distinct.
Then exercise the empty failing capture and late ItemAt error before
widening the cohort. Matching output values alone cannot prove once-only
evaluation, shared budgets or cancellation.

## Admission, refusals and follow-on ownership

Preflight, extension export and native export must preserve
`MfdError::UnsupportedSequenceComposition { item }` before publication.
Reconstructing a bare Generate, a legacy reducer or a partial design loses
the transform. Legacy importer/exporter shapes and declared clone ownership
are unchanged. A new form needs both descriptor and execution evidence.
