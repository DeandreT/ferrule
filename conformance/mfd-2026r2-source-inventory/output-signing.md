# Digital signatures on XML and XBRL mapping outputs

`formats.sign-xml-xbrl-output` tracks one operation: create a digital signature
for an XML or XBRL mapping output in the MapForce desktop BUILT-IN preview.
The XML and XBRL target settings point to the same signing chapter. Settings
belong to the individual output component and are stored with the saved MFD.

| Output and placement | What distinguishes the case | Installer manual (physical pages) |
| --- | --- | --- |
| XML, detached | Two outputs: the XML document and a separate signature that refers to it. The manual shows a temporary preview pair; **Save all generated outputs** writes both to the output directory. | §4.3.8.1 p. 172; §4.3.8.2 pp. 172–173, including both preview images |
| XML, enveloped | The signature is the last child of the document element. The XML schema must allow that element for the result to remain schema-valid; that check is separate from signature verification. | §4.3.8.1 p. 171; §4.3.8.2 p. 173 |
| XBRL, detached | The XBRL target setting enables output signing, but the placement rule permits only an external signature file. Do not generalize XML's enveloped mode to XBRL. | §4.8.6.2 p. 542; §4.3.8.1 p. 172 |

The certificate path requires an accessible certificate with a **private key**
for signing. The verifier uses its **public key** (which can be included in
the signature via certificate-only **Append KeyInfo**). The manual separately
describes a password-based option. Its note to make a detached signature file
active when verifying does not identify a MapForce signed-input verification
operation or an acceptance/rejection result in the mapping. This slice adopts
output signing only; it does not repurpose the earlier verification example
in [workflow-contracts.tsv](workflow-contracts.tsv). The manual's XSD and WSDL
placement notes are not used to add mapping targets or capability IDs here.

**Backend boundary.** §20.1.2 starts its single XML/XBRL signatures row at
p. 1605 row 16 and finishes the linked label on p. 1606. Only BUILT-IN is
marked. The two broader format rows (§20.1.1 p. 1602 rows 1 and 5) have their
own marks; their signature footnotes on p. 1604 limit signature processing
to BUILT-IN. §4.3.8 p. 168 narrows signing to preview and expressly excludes
MapForce Server. The invalid-settings discussion on p. 172 mentions command-line
handling (error or unsigned output); that passage is not positive evidence
for successful command-line signing.

The [source manifest](oracles/output-signing-source.json) records the bounded
cases, citations and two detached-preview image fingerprints. They are
documentation, not execution evidence. All 16 new assessment cells are
unassessed, including the matrix mark and blanks. The previous 824 entries,
versions and evidence are unchanged; `inventory_complete=false`.

Run `python -B verify_output_signing.py --check-introduction` to compare
against exact base `b3e3186e`. Add `--pdf PATH` to verify the pinned installer
PDF, in-topic passage anchors, preview fingerprints, graphical marks and
cross-page row links. Run `python -B -m unittest test_live_inventory` for the
offline controls.

**First runtime gate:** With a synthetic XML target and a disposable signing
identity, retain the saved design, generate a detached pair from BUILT-IN
preview, and save both outputs. An independent verifier must accept the saved
pair and reject it after a covered data value changes. That would qualify only
the tested desktop case, not MapForce verification, enveloped or XBRL signing,
other engines, or any Ferrule surface. This source-only review did not run it.
