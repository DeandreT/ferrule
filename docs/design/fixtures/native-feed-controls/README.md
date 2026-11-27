# Saved range and singular-feed controls

`python controls.py NEW_DIRECTORY` writes seven self-authored MFDs with
case-local XSDs, input XML and expected output or diagnostic text. The
expectations are independent of the preview. For a round trip, execute the
design, save it to a new file, close and reopen that file, then execute and save
again. Compare both saves and complete output bytes, including partial XML on
conversion failure.

| Cases | Only distinction under test | Frozen comparison |
| --- | --- | --- |
| `explicit`, `omitted` | Lower Generate pin connected to decimal 1 or disconnected; upper is decimal 3 in both | Complete ordered `Result/Mark` values `1,2,3`. A validation refusal or different sequence is a material result. |
| `two-values` | Two direct String feeds to separate cloned entries of one singular double field | Complete `Result/Reading` values `7,31`, **invalid** against the singular target. A single value contradicts this prediction. |
| `east-bad`, `west-bad` | Only the first or only the second string fails conversion | The failing token is respectively `ochre` or `indigo`; retain the complete diagnostic and any output. |
| `both-bad`, `both-bad-reversed` | Both conversions fail; reverse entry order while keeping source tokens, port identities and edge order fixed | First failure names `ochre`, then `indigo`, if declared order selects it. |

Compare complete saved connections, declared types and clone order with
the frozen outputs, XSD validity and first diagnostic. The range probes test
scalar values, not Position or a filter/map descriptor.

In `west-bad`, check the partial XML for the first `Reading` (`7`) as well
as the diagnostic `Conversion to double failed for 'indigo'`. See the
[target binding rules](../../../conformance/singular-target-feeds.md).
