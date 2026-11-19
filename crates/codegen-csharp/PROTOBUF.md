# Generated Protobuf byte interface

Call `emit_with_protobuf_bytes(&project)` with the saved mapping. Each primary or named endpoint needs its own Protobuf settings. Generation resolves the root against the embedded schema and imports and compares its projected shape to the endpoint schema. Execution has no schema argument and opens no instance or schema paths.

`ExecuteProtobufOutputs` returns the primary output first, followed by named targets in declaration order. `ExecuteSelectedProtobufOutput` evaluates the requested target only. Both take the primary bytes and exactly the declared named inputs (ordinal, case-sensitive names). Input buffers must not be mutated during execution. Empty input and output are valid when the message permits them.

This static profile covers flat messages with implicit or required `string`, `bytes` (Ferrule UTF-8 text), `int64`, and `bool` fields. Typed output scalars must match the field. Other types, collections, explicit optional fields, oneofs, dynamic paths, and mixed boundary options are not admitted. Unknown varint, fixed-width and length-delimited fields can be skipped; group wire markers are refused.

Limits are 4 MiB per document, 16 MiB per input or output bundle, 63 named endpoints per direction, 256 declared fields per message and 65,536 wire fields per input. These bound adapter data, not mapping execution or total process memory.
