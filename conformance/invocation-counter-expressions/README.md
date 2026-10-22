# Invocation counter expression controls

Self-authored mappings exercise explicit, bounded invocation counters. They do
not represent a captured MapForce mapping or authorize MFD importer admission.

- The parameter fixture checks signed wrapping, current steps, repeated calls
  and primary/named output state.
- The source fixture checks strict operand kinds and their evaluation order.
  Direct instance entry calls avoid attributing an earlier JSON parser refusal
  to the counter expression.
- The row fixture distinguishes changed start from changed restart, missing
  restart from present empty text, and invocation state from row-local state.
- Admission fixtures keep every owned companion and user-function body closed,
  including unused counter expressions.

`expected.rs` contains literal complete JSON documents. Hosts encode document
bytes as hexadecimal only for a lossless, line-oriented test transcript; the
test compares all bytes, including indentation and the final newline.
