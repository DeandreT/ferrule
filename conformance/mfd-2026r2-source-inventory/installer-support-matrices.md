# Enterprise 2026r2: documented backend coverage

[The catalog](installer-support-matrices.csv) numbers the operation rows on
each physical PDF page. Section 20.1.1 has 30 format and connector rows;
section 20.1.2 has 28 mapping and deployment rows. The digital-signature row
begins on page 1605 and finishes on 1606; its continuation is not a second row.
Each of the eight vendor columns records the **printed mark** (`1` = dot,
`0` = blank). Apply the qualifications below before interpreting a row.

The source is the English manual shipped in the Enterprise 2026r2 x64
archive. The official download and container hashes are pinned under `release`
in [artifact-observations.json](artifact-observations.json).
The release manual is 23,722,503 bytes, SHA-256
`9cf5bfa17ec38140941566dd15b19a9c32185339b04fb7383674f50452f43a09`,
1,750 pages. The [separately pinned public manual](documentation-pages.json)
has 1,751 pages. Its TOC entries 1085 and 1086 refer to the same sections;
their page numbers, layout line anchors and text-only dispositions belong to
that other PDF. The unversioned download URL is not a substitute for either pin.

## Reading the rows

| Qualifier | Source and effect |
| --- | --- |
| `xml-signatures`, `xbrl-signatures` | §20.1.1, p. 1604, notes 1–2: signature processing is limited to vendor built-in execution even where the containing format has a broader set of marks. |
| `strict-xlsx-input` | §20.1.1, p. 1604, note 3: the three Excel rows describe OOXML workbooks from 2007 onward. Strict spreadsheets are input-only; the shared `.xlsx` extension does not grant either target operation. |
| `driver-environment` | §20.1.1, p. 1604, note 4: the database and execution environment still constrain each marked interface. |
| `call-only`, `soap-call-security` | §20.1.1, p. 1604, note 5: these rows cover calls. Generated C# and Java SOAP clients exclude client certificates, preemptive authentication and WS-Security. Service generation has its own row. |
| `collection-engine` | §20.1.2, p. 1606, note 1: wildcard inputs in XSLT 2/3 and XQuery use `fn:collection`; wildcard resolution depends on the engine. |
| `json-string-adapters` | §20.1.2, p. 1606, note 2: the general string-adapter row marks only built-in. The note explicitly adds C# and Java for JSON. |
| `driver-language` | §20.1.2, p. 1607, note 3: database exception handling requires the connected driver to support the selected language. |

For example, reading an ordinary `.xlsx` source, creating a target, and
updating an existing workbook have three distinct sets of marks: XSLT 2/3,
C#, Java and built-in for reading; C#, Java and built-in for creating; and
built-in alone for updating. As another example, the SOAP **call** has a
built-in mark, whereas the **generation** row marks C# and Java only.

`ledger_context` points to a related native-profile capability where there is
a useful broad match. The [operation-level review](adapter-operations.md)
resolves all four originally blank contexts. The [SELECT and workbook review](select-workbook.md)
distinguishes four operations whose original rows already had broad contexts. A broad match does not
establish a one-to-one identity or transfer an assessment. Vendor `built_in`
covers the product's output preview or server execution; `csharp` in this
table concerns vendor output. Ferrule import, interpretation, native export,
self-roundtrip, GUI, debugging, generated Rust and generated C# require
independent evidence on every row. The ledger's existing assessments and
`inventory_complete=false` are unchanged.

## Reproducing the comparison

Run `python verify_installer_matrices.py` from this directory for the
repository-only checks. With PyMuPDF available, pass `--pdf PATH` to verify
the exact manual hash and compare the catalog with the positions of the dots
on all five table pages. The PDF is not needed for ordinary catalog checks.

## Open inventory work

At the matrix review the canonical ledger had 789 records. All 752 records added after the original
37 have a corresponding canonical ID in `source_units` in the
[source companion](../mfd-2026r2.source-contracts.json): 576 API records,
67 existing-ID enrichments and 109 appendix records. Their empty execution
evidence lists are not missing source citations. Likewise, a matrix mark
cannot serve as a runnable example for any of those records.

The [page inventory](documentation-pages.json) identifies 462 pages with
only context or control coverage in its compared registries. The two matrices
resolve one source question; they do not supply the missing operation-level
contracts for those pages. Named function defaults, cardinalities and failure
cases, complete generated-wrapper contracts, and the release binding of the
historical online COM documentation are still open under
[#820](https://github.com/DeandreT/ferrule/issues/820).
