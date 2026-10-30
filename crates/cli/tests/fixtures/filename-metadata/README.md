# Filename metadata fixtures

These small HL7-style configuration, JSON schema and mapping files are authored
for this suite. They do not come from another application's examples or a
customer mapping.

The baseline maps two String fields into a closed JSON object. The tests add
one saved metadata form at a time and compose the supported forms. Preview
filenames are deliberately different from the runtime parameters.

The generated hosts use non-ASCII text, preserve both filename parameters and
check missing parameters, wildcard filenames and direct-selection cardinality.
Reader encoding/BOM, EDI settings/messages and target cast markers have separate
refusal controls. A saved reader or cast setting must not disappear merely
because the baseline String fields happen to produce an expected document.
