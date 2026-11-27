# Native scalar-sequence controls — 2026-10-26

These observations advance the experiment in
[scalar sequence saved-design interchange](../design/scalar-sequence-saved-design.md).
They do **not** establish a faithful `filter_map_v1` representation or change
its typed export refusal.

## Reference and authoring

The reference was MapForce Enterprise 2026r2 x64, executable 28.2.0.0, running
offline in an isolated Wine environment. Its executable and copied resources
were matched to the official installer. This is not native Windows ABI
qualification.

Independent schemas and expected values were frozen before preview. The
reference used its visible controls and matching installer catalogs; schema
validation stayed enabled. Saved candidates were reopened and resaved. The
failed Position candidates below were not saved.

## Legacy Generate controls

Both controls generated 3 through 5. The complete XSD-valid rows were
`(Value=3, Position=1), (4,2), (5,3)` with no extra fields or root attributes.

The native constant controls are significant:

- **Number** saved the bounds as `datatype="decimal"`.
- **All other** saved them as `datatype="anySimpleType"`.

Neither is the exact Int-constant descriptor from the frozen Ferrule recipe.
Their matching values are a finite behavioral observation, not proof that
literal types or bound-error behavior can be erased.

Each original native save was closed, reopened and saved to a fresh file before
execution. The decimal control gained an explicit UTF-8 source encoding on
re-save. The generic-literal control was byte-identical. The added encoding is
retained as a metadata difference, not discarded as formatting.

## Predicate and mapper candidate

Visible function controls created `Keep` and `Map`, each with required,
non-sequence integer inputs named `item` and `source_position`. `Keep` has a
Boolean result fed by an All-other literal `true`; `Map` returns its integer
item. The Inlined-use option was deselected. The saved definitions retain the
unused parameter declarations, but that alone does not establish whether their
arguments are evaluated or adapted eagerly.

A Position on the mapper output failed validation: `The 'position' function
cannot be used here because there is no current sequence.` Position on the
Filter's true output also failed, even when Filter drove the rows. These
are unsaved failures of those topologies only.

A deliberately limited variant removed the extra Position and used the
original Generate position for output. It saved, reopened, re-saved and
executed with the same three complete rows as the legacy controls. Whole saved
structures matched after removing only checked coordinate-only views and empty
descriptions; complete original files and their differences were retained.
Ferrule's best-effort import produced the same complete output, but executable
import refused the unused physical source. This is not a clean executable
round-trip or `filter_map_v1` reconstruction.

## Complex variable: original and dense positions

A separately authored candidate passes the filtered mapper result into an XML
variable with the shape `Bucket/Row*/Value`, then reads that variable's rows into
the final target. The physical input root drives the variable's `compute-when`
input. A new Position reads the variable's Row output; the original Generate
Position remains connected to the predicate and mapper arguments.

For Generate `3..5`, predicate `item > 3`, and mapper
`item * 10 + source_position`, the re-saved design produces the complete rows
`(42,1), (53,2)`, valid against the independent target XSD. The mapped values
retain original source positions `2,3`; the target positions are dense `1,2`.
This establishes that finite distinction, not reconstruction of the complete
scalar `filter_map_v1` descriptor.

The first re-save changes coordinate-only views and adds
`XSLTTargetEncoding="UTF-8"` to the variable. All other non-layout XML matches.
The encoding addition is retained as a real metadata difference, not removed
under formatting normalization. Ferrule's executable import refuses this
generated repeated variable child because it is not one supported source
collection. The repair-oriented import does not create an executable substitute.

## Complex variable: late error versus ItemAt

The next independently frozen control keeps the predicate and uses mapper
`(item * 10 + source_position) / (5 + item * -1)`. Item `4` maps to `42`; the
later retained item `5` reaches a zero denominator. The full-output and ItemAt
designs retain the same mapper body and parameter identities. The ItemAt
variant removes the full-row target and dense Position consumer, and feeds only
the variable's Value sequence through ItemAt at index `1` to one integer field.

| Source range | Consumer | Complete native outcome |
| --- | --- | --- |
| `3..4` | Full rows | One row `(42,1)` |
| `3..5` | Full rows | Execution fails: `Numeric overflow.` |
| `3..4` | ItemAt `1` | `FirstValue=42` |
| `3..5` | ItemAt `1` | `FirstValue=42`; execution succeeds |

The `3..5` ItemAt succeeds where the frozen Ferrule contract requires the
later mapper error. The native `Numeric overflow.` and Ferrule
`division by zero` are distinct diagnostics. Dense positions alone therefore
cannot qualify this topology; the outcome says nothing about caching.

## Count guard before ItemAt

A further native-authored candidate retains the same variable, predicate,
mapper and ItemAt index. Its sole target value comes from an If-Else:
`if count(variable/Row/Value) > -1 then ItemAt(variable/Row/Value, 1) else 0`.
The count has no connected parent-context input. Its value input is the same
variable Value output used by ItemAt; there is no separate full-row target.
The condition and both branch connections remain explicit in the saved graph.

The two saved/reopened outcomes were:

| Source range | Complete count-guarded native outcome |
| --- | --- |
| `3..4` | `FirstValue=42`; execution succeeds |
| `3..5` | Execution fails: `Numeric overflow.` |

Non-layout XML and the mapper body matched the unguarded control. The
guard preserves this late error, not necessarily eager evaluation, once-only
invocation or the complete descriptor. In particular, ItemAt may reevaluate
the sequence; no cache behavior was observed.

## Captures on an empty range

Four further controls distinguish an empty `4..3` range from one item `4..4`.
Both stage functions declare ordered item, source-position and capture inputs.
Keep returns true; Map returns `item + capture`. The capture input and Map result
are declared `double`; the variable and target Value fields use `xs:double`.
The full-row output keeps the variable's dense row position, with no ItemAt
consumer whose empty-input behavior could mask the result.

The successful capture is the native decimal literal `1`. The failing capture
is the same parent's `divide(1, 0)` result, wired to both stage capture inputs.
The bounds and true literal are saved as `anySimpleType`, not the exact typed
constants of the independently frozen Ferrule models. These declaration
differences remain explicit; numeric agreement does not establish strict Float
identity or adaptation equivalence.

| Source range | Capture | Complete native outcome |
| --- | --- | --- |
| `4..4` | `1` | One row `(5,1)` |
| `4..4` | `divide(1,0)` | Execution fails: `Numeric overflow.` |
| `4..3` | `1` | Empty Output; no rows |
| `4..3` | `divide(1,0)` | Empty Output; execution succeeds |

All four designs validated. On resave only checked layout/empty-description
differences were removed for comparison. Successful output validated against
the target XSD. The bad capture is reachable with one item; on an empty
range the candidate succeeds where Ferrule fails in Capture before either
stage. This does not reveal whether the reference skipped or suppressed
evaluation.

## Remaining boundary

No tested topology preserves both private owners, ordered captures, strict
stage adaptation, eager errors and shared limits/cancellation. Keep the
[typed interchange refusal](../design/scalar-sequence-saved-design.md#disposition-by-frozen-recipe);
the positive position and count-guard controls remain finite observations.
