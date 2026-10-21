# Ordered text selection

`last_present_text` and `last_distinct_text` are explicit scalar reductions.
They accept strings and the missing-value sentinel (`Null`). Missing values are
ignored; an empty or entirely missing sequence returns missing. Empty strings,
whitespace, case and Unicode spelling are preserved exactly. JSON null, XML nil,
numeric values, booleans and decimals are rejected rather than converted. Neither
operation accepts an extra argument.

`last_present_text` returns the final present string in source order.
`last_distinct_text` remembers strings in first-seen order and returns the final
new string. For `A, B, A`, these return `A` and `B`, respectively. Distinctness is
ordinal string equality, not sorting, trimming or Unicode normalization.

The interpreter and generated Rust/C# use the same contract. These operations do
not change ordinary reductions or expand owned-aggregate admission. MFD export
refuses them, including unused graph nodes and declared function bodies, until
an explicit export representation is supported. MFD import does not infer these
operations from a general grouping or overwrite pattern.
