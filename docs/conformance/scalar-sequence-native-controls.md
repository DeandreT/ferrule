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

The designs were created through visible controls: independently authored XML
schemas, two constants, Generate, Position, and then separate scalar predicate
and mapper functions. Schemas and complete expected output values were fixed
before preview execution. Missing standard XML Schema catalogs initially
prevented schema validation; matching installer catalog/schema resources were
added only to the isolated environment. Validation was not disabled.

## Legacy Generate controls

Both controls generated 3 through 5 and produced the complete ordered rows
`(Value=3, Position=1)`, `(4,2)`, `(5,3)`. Outputs passed the independently
authored target XSD, with no extra fields or root attributes. Every input,
schema and original design remained unchanged. Output files were compared
after the reference processes had closed, not immediately after a Save click.

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

The straightforward candidate used Generate, the predicate call, a Filter,
the mapper call and a row consumer. A separate output Position connected to
the mapper result failed native validation:
`The 'position' function cannot be used here because there is no current sequence.`
Connecting that Position to the Filter's true output also failed, including
when the Filter drove the target rows. Invalid Save attempts were declined;
the full messages and authoring records were retained. These unsaved failures
are not claimed as complete saved-descriptor comparisons, nor as proof that
every possible native representation fails.

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

Each design was saved, reopened, re-saved and executed separately. Complete
messages, outputs and original-resource checks were retained before comparison.
The reference's `Numeric overflow.` and the frozen Ferrule model's
`division by zero` are distinct reported errors; no exception-type equivalence
is asserted.

The `3..5` ItemAt result contradicts the frozen expectation that the late native
failure would survive the change of consumer. **This complex-variable candidate
does not preserve the eager-error contract of `filter_map_v1`.** Its successful
position control is not sufficient for general interchange admission. The
observation concerns the visible result/error boundary, not proof of the
reference's internal evaluation or caching strategy.

## Count guard before ItemAt

A further native-authored candidate retains the same variable, predicate,
mapper and ItemAt index. Its sole target value comes from an If-Else:
`if count(variable/Row/Value) > -1 then ItemAt(variable/Row/Value, 1) else 0`.
The count has no connected parent-context input. Its value input is the same
variable Value output used by ItemAt; there is no separate full-row target.
The condition and both branch connections remain explicit in the saved graph.

The two outcomes were fixed before authoring, then each complete design was
saved, reopened, re-saved and executed:

| Source range | Complete count-guarded native outcome |
| --- | --- |
| `3..4` | `FirstValue=42`; execution succeeds |
| `3..5` | Execution fails: `Numeric overflow.` |

All non-layout XML matched between each original save and its re-save after
removing only checked coordinate-only views and empty descriptions. The mapper
body matched the unguarded ItemAt control. Original designs and resources stayed
unchanged; complete messages, the successful output and process closure were
retained before comparison.

The guard therefore preserves the late native error in this finite control,
unlike the unguarded complex-variable candidate. It does not establish a faithful
`filter_map_v1` representation: descriptor reconstruction, strict scalar
adaptation, captures on empty ranges, stage invocation count, other error
precedence, multiple consumers, shared limits and cancellation remain
unqualified. In particular, the observation does not prove whether the value
sequence is cached or evaluated again by ItemAt. The native and Ferrule error
categories remain distinct, and every existing export refusal remains in place.

## Remaining boundary

The original identity variant is not a substitute for dense output positions:
its predicate keeps every item, so original and filtered positions happen to
agree. The unguarded complex-variable variant distinguishes the positions but
fails the late-error/ItemAt control; the count-guarded variant matches that one
late-error control. None establishes the full two-private-owner descriptor,
ordered captures,
strict stage adaptation, late-error precedence, eager completion before a
consumer, or shared work/cancellation limits. No importer or exporter admission
follows from these observations. The distinguishing frozen controls and the
existing typed refusal remain required.
