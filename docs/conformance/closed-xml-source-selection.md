# Closed, unqualified XML source selection

An independently authored physical XML mapping connects two repeated groups
and an optional integer to equivalent target occurrences. Both XSDs have no
target namespace. The design uses UTF-8 and the target's
`AddSchemaLocation` was `0`. The input root selection was explicitly `{}Input`.

| Input | Complete selection result | Runtime warnings |
| --- | --- | --- |
| Unqualified rows with 7, no `Inner`, and 9 | Three rows: 7, empty, 9 | 0 |
| `{urn:other}Input` with otherwise unqualified descendants | Empty `Results` | 1: root differs, no data selected |
| `{urn:other}Count` as the only integer | One row with one empty `Inner` | 0 |
| `{urn:other}Count` = 7 before unqualified `Count` = 9 | One row with one `Inner` containing 9 | 0 |

The original and saved designs and their inputs are in
`crates/mfd/tests/fixtures/closed-source-selection/`.

The Ferrule native-reader policy is attached only to a physical MFD input
with an explicit unqualified root, an authoritative no-namespace XSD and
the observed source shape. In this profile a well-formed look-alike root emits
a warning and an empty row sequence; a different local root or malformed
XML is still an error.

When reading directly with an XSD, a look-alike root instead reports
`UnexpectedRoot`. Closed groups select only children with the declared
expanded name; a qualified look-alike cannot displace a later unqualified
child. DTDs, entry trees and other shapes without namespace information
continue to match by local name.
