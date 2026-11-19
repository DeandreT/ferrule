# Empty element-only groups in indented XML

MapForce Enterprise Edition 2024 x64 (26.0.0.0) ran a synthetic physical
XML-to-XML mapping with the built-in language and `AddSchemaLocation="0"`.
Both schemas declared repeated rows containing repeated inner groups with
an optional integer. The input had three rows: one containing 7, one with
no inner group, and one containing 9. All row and inner occurrences were
connected.

The 194-byte UTF-8 output contained `\r\n\t<Row/>\r\n` between the two
populated rows. Its SHA-256 was
`b2e7b5971c343846e09f1742272b350e2cd12499991b93faf20dbcd5a0d71070`.
The original and reopened native save produced identical output bytes, with
zero execution errors or warnings. The two native saves were byte-identical.

Ferrule follows the observed empty-element spelling when indenting an empty
element-only group. Its surrounding indentation remains the Ferrule convention
(spaces and LF). Compact output retains its existing spelling, and mixed or
text content retains its existing whitespace.
