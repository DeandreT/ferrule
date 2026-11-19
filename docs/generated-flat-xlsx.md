# Generated flat workbook output

`codegen_csharp::emit_with_flat_xlsx_output(&project)` emits
`ExecuteJsonToXlsxBytes(byte[], FerruleExecutionContext)`. It reads one strict
JSON row, executes the generated mapping, and writes one new worksheet. It does
not open a file or start a worksheet thread. A worksheet read by the bounded
interpreter has the mapped cell values. The generated ZIP layout is not the
native writer's ZIP layout; do not substitute cell equivalence for a required
exact-byte baseline.

Both schemas must be plain, non-repeating flat groups with 1–26 unique scalar
fields. Field names are 1–64 ASCII letters, digits or underscores. The source
must declare plain JSON; the target must declare XLSX with headers enabled.
The saved sheet name (default `Sheet1`) uses the same character set, has at
most 31 characters, and cannot be `History` (case-insensitive). The first row
contains field names; the second contains a complete mapped row in schema
order. Additional sources/targets, computed output paths, update-existing,
column selections, renamed headers and other boundary options are refused.

JSON input is limited to 64 KiB and the output ZIP to 256 KiB. Admitted cells
are booleans, integers of magnitude at most 999,999,999,999,999, and nonempty
strings of at most 1,024 UTF-16 code units without controls or underscores.
Text is written as inline text, never as a formula. No partial workbook is
returned on failure. This entry point does not accept XLSX input.
