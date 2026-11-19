# Generated flat workbook output

`codegen_csharp::emit_with_flat_xlsx_output(&project)` emits
`ExecuteJsonToXlsxBytes(byte[], FerruleExecutionContext)`. It reads one strict
JSON row, executes the generated mapping, and writes one new worksheet. It does
not open a file or start a worksheet thread. With XLSX input and an explicit
root iteration over source rows, the emitter instead supplies
`ExecuteXlsxToXlsxBytes`. It executes the mapping once on the ordered row set.

For portable workbook-to-workbook runs, opt into
`PayloadRunOptions::with_portable_workbook()`. Both implementations write the
same five stored ZIP entries in a fixed order. Both readers re-encode and check
the complete package: a workbook resaved by Excel, a compressed package, extra
parts, formulas or different headers do not qualify. The general bounded XLSX
payload profile is unchanged and does not promise this byte layout.

Both schemas must be plain, non-repeating flat groups with 1–26 unique scalar
fields. Field names are 1–64 ASCII letters, digits or underscores. The source
must declare plain JSON or the portable workbook settings; the target must declare XLSX with headers enabled.
The saved sheet name (default `Sheet1`) uses the same character set, has at
most 31 characters, and cannot be `History` (case-insensitive). The first row
contains field names, followed by 0–256 complete rows in schema order. Additional sources/targets, computed output paths, update-existing,
column selections, renamed headers and other boundary options are refused.

JSON input is limited to 64 KiB and the output ZIP to 256 KiB. Admitted cells
are booleans, integers of magnitude at most 999,999,999,999,999, and nonempty
strings of at most 1,024 UTF-16 code units without controls or underscores.
Text is written as inline text, never as a formula. No partial workbook is
returned on failure. Workbook input and output must each fit within 256 KiB.
