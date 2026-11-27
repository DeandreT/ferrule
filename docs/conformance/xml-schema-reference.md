# XML schema-location hints in MFDs

On a physical XML target, absent `properties/@AddSchemaLocation` or `"1"`
enables the schema hint; `"0"` disables it. This applies to named targets
too. Ferrule imports the stored schema reference relative to the design,
not to the output. It exports unhinted targets with an explicit `"0"`, and
omits the property for a hint still derived from the schema reference.
Explicit `xml_schema_hints` are unsupported by MFD export. See
[MFD interoperability](../mfd-interop.md#xml-output-schema-location-hint).

## Conformance evidence

The [versioned inventory](../../conformance/mfd-2026r2.json) cites this
finite XML export check. In Enterprise 2026r2 x64 (28.2.0.0) under Wine:

| Physical target | On or absent | Off |
| --- | --- | --- |
| Unqualified root | `xsi:noNamespaceSchemaLocation` with the schema's absolute file URI | No hint |
| Namespaced root | `xsi:schemaLocation` with the namespace and absolute file URI | Not exercised in the namespaced control |

Saving an explicit `"1"` produced the absent form. A Ferrule export of an
unqualified `Envelope` with a signed integer and a string produced the
expected XSD-valid, no-hint output before and after save/reopen. The two
saves matched byte for byte; executable re-import reproduced Ferrule's XML.
The exported declaration was `xs:integer`, not the original `xs:long`.
This establishes the tested values and root hint, not exact preservation of
the integer schema or general external XML execution.

The [export tests](../../crates/mfd/tests/xml_schema_reference.rs) and
[import tests](../../crates/mfd/tests/xml_schema_location_hint.rs) exercise
Ferrule's side of the contract.
