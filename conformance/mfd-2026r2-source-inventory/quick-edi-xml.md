# Quick EDI-to-XML conversion

`formats.quick-edi-to-xml` adds a separate assessment slot for the conversion
documented in §4.7.5 of the pinned English Enterprise 2026r2 x64 installer
manual. The section describes one conversion in two component roles, not two
separate formats. Both produce XML without requiring or providing an XML
schema. The XML structure depends on the EDI data, omits fields with no data,
and cannot be customized in this mode (physical p. 505).

| EDI component role | Distinguishing behavior | Installer evidence |
| --- | --- | --- |
| Source | With an EDIFACT `ORDERS` source, **Create Mapping to XML** adds a target for XML output and wires it with copy-all. | Page 506 shows the wired graph and an XML preview with `Envelope` → `Interchange` → `UNB`. The target in the diagram still uses the ORDERS component tree. |
| Target | On an existing EDI target, **Write content in XML format** switches its output from EDI to XML. This is a target-only setting, not the source action that creates a new component. | Section 4.7.2 p. 473 confirms the setting is meaningful only for an EDI target. Section 4.7.5 pp. 506–507 shows an X12 example with `Envelope` → `Interchange` → `ISA` in the XML preview. |

Those partial previews show a concrete difference from writing ordinary EDI
and from wiring an XML target to a user-supplied schema. Their example element
names and encoding declarations do not specify a universal output grammar or
defaults. The earlier `quick-edi-shape` case in [mapping-contracts.tsv](mapping-contracts.tsv)
already records omission and shape under the broad `formats.edi` context;
neither that context nor EDI string parsing/serialization is replaced here.

**Matrix boundary.** Section 20.1.2, physical p. 1606 row 7, links directly to
§4.7.5 and marks only BUILT-IN. The text also explicitly requires BUILT-IN for
this conversion. The mark and blanks are documentation, not a vendor run or
Ferrule applicability decisions. All 16 new cells are unassessed with empty
evidence; the prior 823 records and their evidence are unchanged. The ledger
has **824** records and `inventory_complete=false`.

The [source receipt](oracles/quick-edi-xml-source.json) pins the role-specific
text and the three illustrations. Only anchors and hashes are retained, not
the vendor artifacts. Run `python -B verify_quick_edi_xml.py --check-introduction`
to compare the introduction to exact Git parent `b9a847ab`. With the retained
installer PDF and PyMuPDF, add `--pdf PATH` to recheck the text, image fingerprints,
matrix marks and matrix-to-topic link. Run `python -B -m unittest test_live_inventory`
for the offline negative controls.

Further qualification needs independent saved-design and execution evidence
for both roles, starting with a source-mode specimen with one populated and one
empty EDI field, its actual XML output, and the saved graph showing the copy-all
target. The previews do not establish omission edge cases, naming and ordering
across EDI families, or whether another implementation understands the saved mode.
