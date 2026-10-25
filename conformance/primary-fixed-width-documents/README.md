# Primary fixed-width filename documents

These authored plans use the shared grouped-row mapping from the CSV cohort.
Every selected fixed-width document is parsed before one mapping run. Widths
count Unicode scalars; BOM input is accepted; LF and CRLF input normalize to LF
output, or records concatenate with no delimiter. Trailing fill is trimmed,
output is right padded, and the layout controls empty fields.

The corpus compares complete bytes, typed failure stages and ordered host calls
across the interpreter and generated Rust/C#. It includes direct and wildcard
selection, empty selections, cross-file grouping, Unicode fill and values,
malformed records, malformed scalar values and per-file byte limits. Additional
host assertions check exact row origins/positions and bounded output/input totals.

Named endpoints and incompatible options are refused before host calls. An
ordinary row position is still the mapping's combined-row position; physical
per-document positions are retained separately in each row's source origin.
