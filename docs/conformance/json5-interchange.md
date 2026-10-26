# JSON5 interchange observation — 2026-10-26

## Decision

MapForce's observed JSON5 setting is not equivalent to Ferrule's native JSON5
codec. Best-effort MFD import retains the setting and instance filenames for
repair, but document-host execution, generated mappings and faithful MFD export refuse that
boundary. No executable MFD JSON5 profile is admitted by this observation.

The setting is stored as `FormatOptions.json5_interchange_dependency`, separately
from the executable `json5` Boolean. A component attribute preserves its exact
text, including unknown spellings. A `.json5` instance path without that
attribute retains an extension dependency. Both survive project save/reload.
Absent dependencies add nothing to legacy serialized options.

## Reference and scope

The reference GUI identified itself as MapForce Enterprise 2026 release 2, x64;
the executable version was 28.2.0.0. It ran offline in an isolated Wine prefix
and virtual display. These are black-box observations, not native Windows ABI
qualification or proof of the installed files' original acquisition provenance.
Original reference files and samples were not edited. Fixtures were independently
authored; no vendor implementation, license data or bundled sample is reproduced.

Five designs were saved, closed, reopened and saved again. Complete bytes matched
after reopening; declaration reordering caused by target edits was retained in
the comparison. The actual controls were **Use JSON5** and the input/output JSON
filename. Selecting the checkbox saved `json5="1"` on that component's `json`
element. Changing the filename to `.json5` did not enable the checkbox or add the
attribute. Modern filenames were nested `file` elements with `inputinstance` or
`outputinstance` roles.

Runtime comparisons covered 40 scenarios per engine: 32 source flag/extension
cases, four target controls, three repeats, and one corrected retry. The object
contained a string, integer, Boolean, number and ordered integer array. Independent
expected types, values, field order and formatting policy were frozen before
execution. Full output files and errors were retained, not sampled prefixes.

## Results and limits

- Explicit JSON5 with strict values plus comments/trailing commas matched the
  frozen complete-value oracle in the tested cases. This is finite-cohort
  agreement, not proof for all inputs in a proposed syntax profile.
- With the checkbox disabled, a `.json5` filename did not enable vendor JSON5.
  Ferrule's native file dispatcher does select JSON5 from that extension.
- A compound JSON5 input was refused because `0x2A` was not valid for its integer
  field; Ferrule accepted it. The other syntax in that compound case was not
  independently isolated.
- For the tested number field, vendor JSON5 mapped `NaN`, `Infinity` and
  `-Infinity` to zero. Ferrule refused those tokens. Vendor output retained
  `1e999` outside Ferrule's finite binary64 domain. No implicit coercion or
  replacement is authorized by these observations.
- A truncated object failed in both engines. The vendor reported
  `Syntax error in JSON input: ',' or '}' expected. (Line 2, Column 1)`.
  Strict-mode comments failed with `Character / is grammatically unexpected.`
- The target checkbox, not its filename extension, enabled unquoted property
  names. Strict-target raw bytes differed between encoders: vendor CRLF/tab
  formatting and no final newline versus Ferrule LF/spaces and a final newline.
  Successful comparison used complete canonical bytes, preserving all values,
  field order and array order while ignoring only JSON whitespace/line endings
  and an optional final newline. It does not establish raw encoder equivalence.
- Failed runs preserved existing output. Successful vendor repeats were
  byte-identical, a repeated refusal still produced no output, and a corrected
  retry succeeded without replacing the retained original failure evidence.

The driver had a reporting-guard failure after a repeated refusal; its original
result was retained. A separate non-executing finalizer corrected the expected
absence of output. The revised accounting explicitly includes the corrected
retry instead of counting target controls twice.

## Regression ownership

`crates/mfd/tests/json5_interchange.rs` uses a synthetic four-boundary fixture to
check settings, extensions, legacy/nested paths, exact attribute text, strict
import, save/reload and export refusal. Legacy path attributes take precedence
over nested paths; unknown setting values are preserved, not guessed as Boolean.
`crates/cli/tests/json5_interchange.rs` covers runtime, preview, stdin, primary
filename and generation refusal before input acquisition/output publication.
Native JSON5 codecs and explicit generated JSON5 companions keep their separate
existing contracts; neither gains vendor interchange compatibility.
The graph interpreter's typed-instance API is not a document-format admission
API; callers that bypass hosts must still validate the project themselves.
