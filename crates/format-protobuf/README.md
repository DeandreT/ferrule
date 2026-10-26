# Protocol Buffers codec

`Layout` resolves and validates the existing proto2/proto3 schema model.
`to_vec` / `to_vec_message` and `from_slice` / `from_slice_message` retain their
existing behavior and `ProtobufError` type. `to_vec_bounded` and
`to_vec_message_bounded` opt into explicit
in-memory encoding budgets through `EncodeLimits`; they do not load resources.

## Bounded encoding

The default budgets are 64 MiB of encoded output, 64 MiB of supplied UTF-8 text,
1,000,000 typed values, 1,000,000 supplied group fields, and instance depth 128.
Callers can lower or raise byte/count budgets up to `isize::MAX`; depth cannot
exceed 128. All budgets allow zero. An empty root group needs one value and
depth zero, but no fields, input text, or output bytes. Absent/implicit defaults
can produce valid zero-byte protobuf output; explicit presence still emits its
tag and payload, even for an empty scalar/message.

- Values includes the root, each group member's value, every repeated or mapped
  sequence container, and every sequence item. Fields counts all supplied group
  entries. Input bytes counts their UTF-8 names and all supplied string values,
  including bytes values and lexical numeric inputs. Counts include data that
  the wire codec would omit. Depth is zero at the root and increases for each
  group-member value or sequence item. Document sets are unsupported and are
  rejected by the native codec without loading their documents.
- Root resolution comes first. Invalid limit configuration and input admission
  failures then stop before native validation and proportional codec allocation.
  This deliberate admission precedence bounds work on malformed/untrusted input;
  a format error beyond an admission limit is not promised.
- Admitted input runs through the **same** native codec with a checked counting
  sink. It validates the complete message before reporting output overflow or an
  output-byte limit, so an earlier large field cannot hide a later native shape,
  range, enum, required-field, map, or oneof error. The counting pass allocates no
  output/nested/packed byte buffers. Implicit-default probes also count rather
  than materializing bytes, and string map keys borrow the input.
- Only after validation and the final byte check does the shared codec emit.
  Every final, nested-message, and packed-field buffer checks length arithmetic
  and caps capacity requests at the measured final length **before** reserving
  or copying. The new `BoundedEncodeError` distinguishes limit, length-overflow,
  and allocation failures; its `Protobuf` variant owns the unchanged native
  `ProtobufError` (also exposed as the error source). Successful
  bytes use the unchanged schema, order, scalar coercion, packing, defaults,
  presence, map, oneof, and import semantics.

These are finite **data budgets, not an exact process-memory ceiling**. The
caller already owns its input and layout. Validation uses bounded field/key
tables, paths, and small scalar-rendering temporaries. Emission can hold one
buffer per active nested message/packed field plus its ancestors; their sizes
are individually bounded by the final length, not collectively by the output
budget. Allocator overhead, retained capacities, schema storage, stack, and other
application work are additional. Use process isolation when a hard memory/CPU
ceiling is required. Calls hold no persistent state, so correcting a failed
value or changing a budget permits a fresh retry.
