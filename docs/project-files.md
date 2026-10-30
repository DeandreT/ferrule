# Project and pipeline files

The CLI, native editor, and browser editor share the mapping crate's project
file codec. Pipeline files use the same codec for all embedded projects.
Existing unmarked files retain their ordinary JSON wire format.

Saving a project must preserve its values when it is reopened. The shared
JSON parser now correctly rounds finite floating-point tokens. Earlier parsing
could shift a shortest-decimal value by one representable step. The file codec
verifies the complete model and preserves floating-point bits, including
negative zero.

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

### File capabilities

Some options or schema metadata must never be silently ignored by a reader
that predates them. A project or pipeline that uses one is always saved in a
version-3 envelope whose header also lists the sorted `capabilities` the
document requires. Readers without the envelope miss the project's required
members, and version-2 readers refuse the version, so neither falls back to
older behavior. Current readers require the declared set to match the features
the document uses exactly: such an option or schema metadata in unmarked or
version-2 JSON is refused, as are unknown or stale capabilities. The
capabilities are:

- `join-computed-keys-v1`, carried by a join plan with a
  [computed key source](design/join-computed-keys.md) in any target scope,
  graph or function body;
- `join-computed-sequence-keys-v1`, also carried wherever that computed
  source is sequence-valued;
- `sqlite_read_v1`, carried by a
  [declared SQLite query](formats.md#declared-sqlite-queries) anywhere in the
  project's format options;
- `xml-target-scalar-output-v1`, carried by an
  [XML scalar output policy](design/xml-cast-suppression.md) in any format
  options;
- `xml-wildcard-exclusions-v1`, carried by a negated namespace set or excluded
  names on an XML wildcard in any stored schema
  ([XSD 1.1 wildcard exclusions](design/xml-wildcard-exclusions.md));
- `json_tuple_arrays_v1`, carried by a
  [JSON tuple frame](design/json-tuple-arrays.md) in any stored schema.


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
apply the same policy. A bounded flat String-attribute root-view pattern
supports `.mfd` import and export; other root expression graphs remain
unsupported. See [XML root-view interoperability](mfd-interop.md#xml-root-view-interoperability).


## Target XML type selection

In the native editor, select a target group and use **XML type** in its scope
inspector to choose one of that group’s declared XML types. The chooser is
available for supported schema type alternatives on primary and named targets,
including nested groups. The canvas shows the type binding as an **XML type**
input beside the group’s ordinary fields.

Choosing a declared type creates a separate constant for that binding. Other
outputs that share the previous expression keep their values. Choose **Schema
inference** to remove the explicit type binding and let the writer select from
the populated fields and the schema default. Ambiguous inferred types fail when
writing XML.

The editor retains invalid imported type bindings and displays them for repair
or removal. Opening the inspector does not rewrite them. Whole source group
copies cannot gain a new explicit type binding; an existing invalid binding can
still be removed. Type edits support undo, redo, and save/reopen, and retain
canonical namespace-qualified type identities.
