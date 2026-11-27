# Scalar filter/map and MFD export

Ferrule can [execute `filter_map_v1`](../design/scalar-sequence-composition.md),
but MFD preflight, extension export and strict native export all refuse it
with `MfdError::UnsupportedSequenceComposition { item }`, even if the
descriptor is unselected. The legacy Generate and reduction forms have their
own admission rules.

A saved representation must preserve more than the output values:

- A stage sees the original producer position; a consumer sees a dense
  post-filter position. Matching these two ordinals on an identity mapping
  cannot establish which owner supplied them.
- Captures run once in declaration order, even on an empty input. A row
  variable that produces an empty output without evaluating a failing capture
  changes the result.
- The sequence is built eagerly. An unguarded ItemAt can return an early
  value instead of raising a later mapper error. Adding a count guard can
  expose that error without establishing once-only evaluation.
- Stage types, both private owners, work and item budgets, and cancellation
  are part of the contract. Decimal or generic range bounds are not
  interchangeable with a declared Int just because they yield the same rows.

The [24 descriptor recipes](../design/fixtures/scalar-sequence-saved-design203.json)
cover seven legacy controls and 17 filter/map cases. The
[public tests](../../crates/mfd/tests/scalar_sequence_saved_design203.rs)
materialize projects, compare complete interpreter outcomes to
[independent expectations](../../crates/mfd/tests/fixtures/scalar_sequence_saved_design203_expected.json),
and check all 51 export refusals before publication.
Set `FERRULE_SAVED_DESIGN203_EVIDENCE_DIR` to a fresh absolute directory per
test to retain the outputs. The [range fixtures](../design/fixtures/native-feed-controls/README.md)
distinguish an omitted lower bound from an explicitly connected decimal 1.
