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
`mapping::pdf_layout_file` exposes the same three file APIs for standalone PDF
layout payloads. An enclosing format can impose a smaller byte limit; `.pxt`
templates exported by ferrule are limited to 1 MiB after XML escaping.

New saves and versioned reads are limited to 64 MiB, including the envelope
and bit table. JSON syntax depth remains bounded, so the versioned envelope
also counts toward that limit. Unknown versions, invalid metadata, and
non-finite values produce typed errors. NaN and infinity cannot be saved as
JSON values without losing their identity; saving rejects them instead of
turning them into null. Failed encoding occurs before file publication.


## Primary XML root reads

`SourceRootXmlTypeEquals` compares the input root's actual `xsi:type` annotation
against one canonical expanded type name. An inferred schema alternative or a
writer's type marker does not establish that annotation. `SourceRootField` reads
one scalar path from the immutable primary input root, without falling back to
a repeated item, ancestor frame, or named input.

`SourceRootField.required` defaults to `false` and is omitted when false, so
existing projects retain their nullable reads. With `required: true`, a missing
or null field produces a typed `MissingRequiredField` error containing the node
and field path. Empty strings and XML nil remain present values. This policy is
explicit and independent of the schema's required-attribute metadata. A field
inside an unselected conditional branch is not read and cannot raise this error.
Generated Rust and C# mappings retain the same lazy behavior and error details.
The editors identify required primary fields and keep these nodes read-only.

The source format option `xml_allow_inactive_root_type_members` is also false
by default. With `xml_document: true`, it permits declared scalar attributes
outside a known explicitly selected root type's member set to remain available.
It supports only a singular closed local XML document with explicit namespaces,
1–32 flat attributes, a concrete default type, and 2–32 type alternatives. It
rejects targets, remote inputs, and other format-option combinations. Unknown or
malformed type annotations and ordinary scalar validation remain errors; absent
annotations retain their absence. Required-value failures are controlled by the
mapping's reads rather than this input policy. File and payload entry points
apply the same policy. These root expression nodes remain outside public `.mfd`
import and export admission.
