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

MapForce Enterprise 2026r2 x64 (28.2.0.0) ran offline under Wine. This is
finite black-box evidence, not native Windows ABI qualification. The
self-authored unqualified `Envelope` contained `xs:long` Amount and String
Label. Switching **Add schema/DTD reference** off added only
`AddSchemaLocation="0"`; switching it on restored the original bytes.
The saved settings were stable after reopen.

Eight independently frozen runtime cases covered signed 64-bit minimum, zero,
maximum and malformed XML with the checkbox on and off. All six successful
outputs matched the full ordered element/value oracle and validated against the
target XSD. The on cases carried exactly the expected schema-instance
`noNamespaceSchemaLocation` URI; off cases had no root attributes. Both malformed
cases failed with `An end tag is missing for start tag name 'Envelope'.`
Original output sentinels remained unchanged during preview.

Comparisons used all expanded names, text and attributes. XML declarations
and element-only indentation were not raw-byte requirements.

## Explicit and namespaced settings

The five controls below distinguish absent, explicit on/off and namespaced
roots. Each actual saved design was validated, reopened, resaved and executed.

| Target root | Setting | Output root hint | Saved setting |
| --- | --- | --- | --- |
| Unqualified | absent | `xsi:noNamespaceSchemaLocation` with the schema's absolute `file:///` URI | absent |
| Unqualified | `"1"` | the same attribute and URI form as absent | absent |
| Unqualified | `"0"` | none | `"0"` |
| Namespaced | absent | `xsi:schemaLocation` of the root namespace and the schema's absolute `file:///` URI | absent |
| Namespaced | `"1"` | the same pair as absent | absent |

All five outputs passed their XSDs with identical data values; resaves
matched byte for byte. Saving explicit `"1"` removed the property. A
namespaced root used its default namespace and had no `noNamespaceSchemaLocation`.

Ferrule imports both enabled forms with the schema reference the design stores,
converted to a URI reference, instead of the machine-specific absolute URI; the
namespaced form keeps the observed namespace/location pair. A relative
reference stays relative to the design and is not rebased to the output, so it
names the schema only for output written beside the design.

## Actual Ferrule exports

Before the export correction, the reference added an implicit root hint to
an unhinted Ferrule output. The corrected physical target set the off
property. The exported design and its native resave both produced the full
XSD-valid no-hint oracle, the two native saves matched byte for byte, and
executable re-import reproduced the previous Ferrule output bytes.

The fixture's `xs:long` became `xs:integer` on export: the evidence is for
those signed values, **not** preservation of the bounded-long declaration.
Only the final comparison verified the actual files referenced by both saves
and the guarded sentinels. An earlier path check and handwritten entry-cache
warning supply no file-integrity or successful execution evidence.

`crates/mfd/tests/xml_schema_reference.rs` covers primary and named physical
targets, strict re-import, unchanged mapping values and the no-hint default.
`qualified_root_view_roundtrip.rs` covers the closed profile's exact property
admission and continued refusal of other spellings.
`crates/mfd/tests/xml_schema_location_hint.rs` and
`crates/cli/tests/import_mfd_schema_location.rs` cover the imported hint for
each setting and root form, its target-only role, DTD targets, a mapped `xsi`
attribute, non-XML sources, and the native export/re-import of the default.

This finite export observation does not qualify general XML export, the
historical signed-integer cohort, partial publication or the separate
[two-phase native reopen protocol](native-saved-design-reopen.md).
