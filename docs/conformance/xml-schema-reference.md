# XML schema-reference export — 2026-10-26

## Contract

Ordinary Ferrule XML output has no schema-reference attribute. Exported physical
XML targets therefore set native `properties/@AddSchemaLocation="0"`, including
named targets that are not the default output. The reference application's
implicit default is different: a checked **Add schema/DTD reference** control
with an empty reference field writes the schema's absolute URI on the root.

Imported designs keep the reference default: an absent or `"1"` setting imports
as a schema-location hint naming the stored schema reference, relative to the
design rather than to the output, and `"0"` imports as no hint. Export writes
the absent setting for a hint still derived that way, after which re-import
names the exported schema sibling, and `"0"` for unhinted targets; explicit `xml_schema_hints`
remain unsupported by MFD export. WSDL boundaries and
intermediate XML string serializers are outside this observation. Closed XML
root-view admission accepts the observed `"0"` and `"1"` spellings when this
property is present; other values retain their refusal.

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

## Explicit and namespaced settings

A second set of independently derived controls reused the same unqualified
design and schemas in separate case directories. An explicit
`AddSchemaLocation="1"` was written into the saved target properties, and two
namespaced variants used a target schema with its own `targetNamespace` and
qualified elements. Each design was opened, validated, saved, reopened and
saved again, then executed; the complete output was retained and validated
against its target XSD.

| Target root | Setting | Output root hint | Saved setting |
| --- | --- | --- | --- |
| Unqualified | absent | `xsi:noNamespaceSchemaLocation` with the schema's absolute `file:///` URI | absent |
| Unqualified | `"1"` | the same attribute and URI form as absent | absent |
| Unqualified | `"0"` | none | `"0"` |
| Namespaced | absent | `xsi:schemaLocation` of the root namespace and the schema's absolute `file:///` URI | absent |
| Namespaced | `"1"` | the same pair as absent | absent |

All five designs executed successfully with schema-valid outputs and identical
element values; save and re-save were byte-identical in every case. Saving an
explicit `"1"` removes the attribute, so `"1"` and absent are the same setting.
The namespaced output declares the root namespace as its default namespace and
adds no `noNamespaceSchemaLocation`. Original inputs, schemas, designs and
output sentinels stayed unchanged.

Ferrule imports both enabled forms with the schema reference the design stores,
converted to a URI reference, instead of the machine-specific absolute URI; the
namespaced form keeps the observed namespace/location pair. A relative
reference stays relative to the design and is not rebased to the output, so it
names the schema only for output written beside the design.

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
`crates/mfd/tests/xml_schema_location_hint.rs` and
`crates/cli/tests/import_mfd_schema_location.rs` cover the imported hint for
each setting and root form, its target-only role, DTD targets, a mapped `xsi`
attribute, non-XML sources, and the native export/re-import of the default.

The first handwritten entry-cache attempt and its load-preview schema warning
were retained rather than treated as a successful execution. The qualified
runtime inputs explicitly name their independently authored source schema.
This finite observation does not requalify the historical signed-integer cohort,
general XML export, partial-artifact publication, or the separate native
save/reopen and owned-dialog protocols.
