# Project and pipeline files

The CLI, native editor, and browser editor share the mapping crate's project
file codec. Pipeline files use the same codec for all embedded projects.
Existing unmarked JSON files keep their original parsing behavior.

Saving a project must preserve its values when it is reopened. Some finite
floating-point values change by one representable step when their shortest
JSON decimal is decoded by the default parser. That can change a numeric
schema bound, a constant, a value-map entry, a user function, or a PDF position.
The file codec preserves the original binary64 bits, including negative zero.

Files that round-trip exactly retain the ordinary JSON form. Other files use
a versioned JSON envelope containing the document and its floating-point bit
table. Older readers that do not support this form reject it instead of
silently changing the mapping. The document remains readable and editable JSON;
its bit table is authoritative for its floating-point values.
Manual changes to those decimal values require matching bit-table updates;
stale metadata rejects instead of silently overriding an edited value.

For host applications, use these APIs for actual file contents:

```rust,ignore
let text = mapping::project_file::encode_pretty(&project)?;
let reopened = mapping::project_file::decode_str(&text)?;
let pipeline_text = mapping::pipeline_file::encode_pretty(&pipeline)?;
let reopened_pipeline = mapping::pipeline_file::decode_str(&pipeline_text)?;
```

Both modules also expose `decode_bytes` for UTF-8 input. Ordinary `Project`
and `Pipeline` Serde serialization remains available as a separate legacy
wire contract; it does not provide the file codec's exact-value guarantee.

New saves and versioned reads are limited to 64 MiB, including the envelope
and bit table. JSON syntax depth remains bounded, so the versioned envelope
also counts toward that limit. Unknown versions, invalid metadata, and
non-finite values produce typed errors. NaN and infinity cannot be saved as
JSON values without losing their identity; saving rejects them instead of
turning them into null. Failed encoding occurs before file publication.
