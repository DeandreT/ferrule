# Complete CSV document adapters

These self-authored cases use positional CSV input with a skipped header and
preserved present empty text. Output is pipe-delimited CSV with a header, LF
record terminators and a UTF-8 BOM. Multiline field content retains its CRLF.

The ordinary mapping keeps every row, adds one to Count and writes Status `ok`.
The grouped mapping combines equal Key values in first-seen order, joins Text
with `+`, and sums Count. The A/B/A case therefore produces two complete rows,
with A's members kept in source order. Expected documents are literal authored
values, not output copied from either backend.

The generated-host tests compare complete text and bytes, exercise context
overloads and repeated calls, and check late parse failures, reached missing
parameters, invalid UTF-8, bounds, and output that cannot be written without
quoting. These fixtures establish a single-document CSV boundary; filenames,
multiple-document loading and publication belong to their own host contracts.

The explicit raw-delimited mode keeps the same input parser, increments Count,
and writes Status `ok|`. It emits no header or BOM, preserves field text (including
delimiters, quotes and embedded CRLF) verbatim, inserts one pipe between the four
modeled fields and ends each record with CRLF. The final pipe is part of Status;
it is not an additional modeled column. Header-only input yields zero bytes.
Ordinary unquoted CSV still refuses field text that requires escaping.
