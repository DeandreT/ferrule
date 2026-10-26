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

## Remaining boundary

The successful variant is not a substitute for dense output positions: its
predicate keeps every item, so original and filtered positions happen to agree.
It also does not establish two private sequence owners, ordered captures,
strict stage adaptation, late-error precedence, eager completion before a
consumer, or shared work/cancellation limits. No importer or exporter admission
follows from these observations. The distinguishing frozen controls and the
existing typed refusal remain required.
