# Raw binary I/O and structured strings

Four of the [58 installer matrix rows](installer-support-matrices.csv) had no
`ledger_context` at `2bd2311d`. The cross-references in those rows lead to **14
independently assessable operations** in the [ledger](../mfd-2026r2.json).
The [operation table](adapter-operations.tsv) gives each ID a distinguishing
input and observable result. All page numbers here are physical pages of the
[pinned English Enterprise 2026r2 installer manual](installer-support-matrices.md).

| Matrix source (physical page, row on page) | Representation |
| --- | --- |
| §20.1.1 p. 1604, row 11: raw binary files | Format heading covering read and write below. No third binary operation. Protocol Buffers has its own row immediately above it and requires a message schema. |
| §20.1.2 p. 1606, row 11: read data from binary files | Links to §5.8.7.19.1, p. 978. Map bytes to a BLOB independently of their lexical representation in XML. |
| §20.1.2 p. 1606, row 12: write data to binary files | Links to §5.8.7.19.2, pp. 980–981. A side effect and a returned final path; in normal preview the final file need not yet exist. |
| §20.1.2 p. 1605, row 14: string parsing and serialization | Links to §8.3, p. 1224. Six declared formats × two directions. No extra ID for the matrix row, the mode selector or either tutorial. |

**Why split the string row?** In §8.3.1, pp. 1224–1226, a parser accepts one
root string and exposes structured items. A serializer accepts structured
items and exposes one root string. One component cannot do both at once.
Reading a CSV record does not show that an EDI interchange can be produced;
parsing a JSON property does not show that the same structure can be serialized.
Section 8.3 explicitly enumerates CSV, fixed-length text, EDI, FlexText
templates, JSON schema and XML schema. This is not an extrapolation from the
larger list of formats supported as ordinary sources and targets.

Read each case's `page_index`, `pdf_page` and `locator` **together with** §8.3
and §8.3.1. The chapter establishes format eligibility and root direction;
a format chapter alone does not establish string mode. The fixed-length
parser tutorial (§8.3.2, pp. 1227–1231) and XML serializer tutorial (§8.3.3,
pp. 1231–1235) show those particular combinations directly. In the latter,
disabling the declaration controls the fragment text; the connection to the
database table, not serialization alone, creates one record per source element.
The remaining combinations have synthetic checks based on the documented mode
and format; they are not twelve vendor-run fixtures. In particular §8.3.1 says
not all file-mode settings are available in string mode. No case assumes
file-mode encoding, BOM, an exact EDI envelope, or JSON key order.

**Backend and assessment boundary.** All four rows have one printed dot, in
BUILT-IN. For the string row, note 2 on p. 1606 and §8.3 on p. 1224 add
vendor Java and C# for **JSON in both directions**. Thus documented positive
environments are BUILT-IN for every operation, plus Java/C# for the two JSON
operations. The dot and footnote are source evidence for this distinction,
not execution receipts. Join each `matrix_surfaces` key to the matrix for the
seven other printed cells; do not treat a blank as a new `not_applicable`
assessment or vendor C# as Ferrule-generated C#.

The two [named functions](functions.json) already had source identities and
`functions.builtins` context. Their direction IDs give those same operations
assessment slots; they are not two new function discoveries. The five
`formats.*` contexts in the TSV remain format families (CSV and fixed text
share one). `formats.flextext` already covers parsing broadly; the new IDs
isolate the string root from a layout or document file. The three existing
[mapping cases](mapping-contracts.tsv) on 815–817 cover dual-direction refusal,
numeric validation and global root selection. Those contracts still apply;
neither their cases nor the [358 workflow examples](workflow-contracts.md)
are added to the 14.

At `703901ad` the ledger had **811 records**: the existing 797 plus 14 native-profile
operations. All **224** new cells (112 Ferrule surface cells, 112 vendor cells)
were unassessed, with empty evidence. For future qualification,
[#531](https://github.com/DeandreT/ferrule/issues/531) concerns scalar JSON
parser fields from a required runtime string, and
[#1170](https://github.com/DeandreT/ferrule/issues/1170) concerns JSON parser
array records. Neither qualifies serialization or the entire JSON parser.
The exact claims and runnable evidence still need independent review under
[#820](https://github.com/DeandreT/ferrule/issues/820), coordinated through
[#89](https://github.com/DeandreT/ferrule/issues/89). `inventory_complete` remains false.

Run `python verify_adapter_operations.py` to check the reconciliation against
the original matrix, the current ledger and both function source
identities. With `--pdf PATH` the verifier checks the cited passages inside
their own topic bookmarks. `verify_installer_matrices.py --pdf PATH` separately
checks the positions of all printed backend dots.
