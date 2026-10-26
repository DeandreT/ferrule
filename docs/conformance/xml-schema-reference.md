# XML schema-reference export — 2026-10-26

## Contract

Ordinary Ferrule XML output has no schema-reference attribute. Exported physical
XML targets therefore set native `properties/@AddSchemaLocation="0"`, including
named targets that are not the default output. The reference application's
implicit default is different: a checked **Add schema/DTD reference** control
with an empty reference field writes the schema's absolute URI on the root.

This change does not alter XML codecs, imported-project defaults, values, error
ordering or file publication. Explicit `xml_schema_hints` remain unsupported by
MFD export. WSDL boundaries and intermediate XML string serializers are outside
this observation. Closed XML root-view admission accepts only the observed `0`
spelling when this property is present; other values retain their refusal.

## Reference observation

MapForce Enterprise 2026r2 x64, executable version 28.2.0.0, ran offline in an
isolated Wine prefix and virtual display. No original reference files or bundled
samples were changed. These are black-box observations, not native Windows ABI
qualification or proof of the installation's original acquisition provenance.

The fixture was independently authored: one unqualified `Envelope` containing
an `xs:long` Amount and a string Label. The reference application populated its
entry tree from the source and target schemas; the two field connections and
instance filenames were set through its real controls.

- Initial checkbox on, empty reference field: the property is absent in the
  saved MFD.
- Checkbox off: the only complete-file change is `AddSchemaLocation="0"` on the
  target properties.
- Save, close, reopen and re-save preserve the complete off-state bytes.
- Turning the checkbox back on restores the original complete bytes; its
  save/reopen/re-save bytes also match.

Eight independently frozen runtime cases covered signed 64-bit minimum, zero,
maximum and malformed XML with the checkbox on and off. All six successful
outputs matched the full ordered element/value oracle and validated against the
target XSD. The on cases carried exactly the expected schema-instance
`noNamespaceSchemaLocation` URI; off cases had no root attributes. Both malformed
cases failed with `An end tag is missing for start tag name 'Envelope'.`
Original output sentinels remained unchanged during preview.

Complete raw output and messages were retained. Comparisons use expanded XML
names and exact text/attributes; declaration and element-only indentation are
not byte-identity requirements. No output prefixes or selected fields substitute
for full comparison.

## Actual Ferrule exports

The pre-change Ferrule export opened and executed successfully in the reference
application, but gained its implicit root schema hint. Ferrule's local output
had no hint. This directly reproduced the annotation mismatch.

The control's `xs:long` is projected by Ferrule to an integer domain and exported
as `xs:integer`. These exported-file checks cover the selected signed-integer
values, not preservation of the original bounded-long schema declaration.

The changed export differs only in generated sibling filenames and the explicit
off property. Its reference execution matched the complete no-hint oracle. The
actual exported design was saved, closed, reopened and executed again, then
saved a second time. Both native saves were byte-identical, and both outputs
matched the complete oracle and target XSD. Executable Ferrule re-import of the
native re-save produced the same local output bytes as before the export change.

The final comparison saved a separate project inside each case directory and
verified the MFD's actual input and output references against the guarded files,
including both native re-saves. Those original files and target sentinels stayed
unchanged. An earlier comparison had guarded similarly named case-local files
while its imported paths referred outside the case directory; that preliminary
record is retained but supplies no file-integrity or publication proof.

`crates/mfd/tests/xml_schema_reference.rs` covers primary and named physical
targets, strict re-import, unchanged mapping values and the no-hint default.
`qualified_root_view_roundtrip.rs` covers the closed profile's exact property
admission and continued refusal of other spellings.

The first handwritten entry-cache attempt and its load-preview schema warning
were retained rather than treated as a successful execution. The qualified
runtime inputs explicitly name their independently authored source schema.
This finite observation does not requalify the historical signed-integer cohort,
general XML export, partial-artifact publication, or the separate native
save/reopen and owned-dialog protocols.
