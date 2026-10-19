# Ordered JSON source documents

`format_json::source_document::JsonSourceDocument` and
`Ferrule.Runtime.FerruleJsonSourceDocument` parse one complete UTF-8 JSON value.
An optional leading UTF-8 BOM is accepted. Decoded object names and members retain
source order, including duplicates; named selection returns every exact match.
Numbers retain their original decimal text, including exponent and negative zero.
String, number, Boolean, null, array and object remain distinct kinds. Selectors
return no value/items for another kind and perform no coercion.

Document identity is supplied separately by the host and retained exactly. It is
nonempty, NUL-free Unicode text of at most 4096 UTF-8 bytes. It is an opaque label,
not a canonical path, a globally unique occurrence ID or a JSON property. Parsing
copies input content; later changes to the caller's buffer cannot change a value.

Default limits are 64 MiB of original input bytes (including BOM and whitespace),
64 nested containers and 1,000,000 values. The root and each member/array value
count once; property names do not count. Callers can lower each limit to zero.
These are document acceptance limits, not a process-memory ceiling. Error kinds
identify syntax, encoding, identity and limit failures; when several failures
apply to the same input, their precedence is not a cross-runtime contract.

`cases.json` is a self-authored corpus with literal expectations, independent of
either implementation. The expected tree is `[kind, content]`, or `["null"]`;
objects contain ordered `[name, value]` pairs. Both runtimes compare the complete
tree. Runtime-specific tests also exercise typed selection, immutable ownership,
UTF-8/identity failures and inclusive byte/depth/value bounds.

This API performs syntax parsing and kind-based selection. Transport, schema
validation or annotations, numeric conversion, mapping demand, document iteration
and output publication belong to their explicit callers. Ordinary schema-shaped
JSON reader APIs continue to enforce their existing contracts.
