# Reference checks still open

`python controls.py NEW_DIRECTORY` writes seven self-authored MFDs with
case-local XSDs, input XML and expected output or diagnostic text. The
expectations were fixed before any preview. These are probes, not saved
application output. **No result from these seven designs is qualified.**

| Cases | Only distinction under test | Frozen comparison |
| --- | --- | --- |
| `explicit`, `omitted` | Lower Generate pin connected to decimal 1 or disconnected; upper is decimal 3 in both | Complete ordered `Result/Mark` values `1,2,3`. A validation refusal or different sequence is a material result. |
| `two-values` | Two direct String feeds to separate cloned entries of one singular double field | Complete `Result/Reading` values `7,31`, **invalid** against the singular target. A single value contradicts this prediction. |
| `east-bad`, `west-bad` | Only the first or only the second string fails conversion | The failing token is respectively `ochre` or `indigo`; retain the complete diagnostic and any output. |
| `both-bad`, `both-bad-reversed` | Both conversions fail; reverse entry order while keeping source tokens, port identities and edge order fixed | First failure names `ochre`, then `indigo`, if declared order selects it. |

Run the explicit-bound control first. Compare both saves and the reopened
execution; check the actual lower/upper connections and saved datatypes. The
probe tests scalar range values only, not Position or a filter/map descriptor.
For the singular cohort, check the saved field, clone markers, entry order and
endpoints before comparing complete outputs, XSD validity and diagnostics.
Record unexpected success, failure or partial output without changing the
predictions. Match source-specific native causes; do not equate diagnostic
wording across products or versions.

The legacy pair is tracked in [#1463](https://github.com/DeandreT/ferrule/issues/1463);
the singular-entry comparisons are tracked in [#1464](https://github.com/DeandreT/ferrule/issues/1464).
