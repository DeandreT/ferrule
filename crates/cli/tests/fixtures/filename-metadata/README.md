# Filename metadata fixtures

These small HL7-style configuration, JSON schema and mapping files are authored
for this suite. They do not come from another application's examples or a
customer mapping.

The baseline maps two String fields into a closed JSON object. The tests add
one saved metadata form at a time and compose all 32 subsets of the five
supported forms. Preview filenames deliberately differ from runtime parameters.

The generated hosts use non-ASCII text, preserve both filename parameters and
check exact missing-parameter, invalid-filename and direct-selection errors.
Reader encoding/BOM, EDI settings/messages and target cast markers have separate
refusal controls. A saved reader or cast setting must not disappear merely
because the baseline String fields happen to produce an expected document.

The opt-in Release host test compiles Rust and C# consumers without invoking a
vendor application. It is not external-reference qualification of these forms.
The C# sandbox copies the repository's exact `global.json` into its working
directory. SDK selection is recorded with `dotnet --version` before building,
using the same executable, working directory and isolated environment as the
host. The host disables shared build servers and automatic framework/reference
package downloads; installed SDKs are never changed by the fixture.
