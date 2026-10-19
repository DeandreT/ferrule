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

## Loaded occurrences

`codegen_runtime::JsonSourceOccurrence` and `FerruleJsonSourceOccurrence` retain
the owning document, the current value and its exact structural parent. Creating
a root establishes a new occurrence even for equal content or the same label.
Reselecting a member in that root preserves its occurrence identity; duplicate
members and array positions remain distinct. Kind filtering produces zero or one
occurrence without converting values or adding a parent frame. A retained child
keeps its document and parents alive after traversal advances or ends.

`JsonSourceDocuments` and `FerruleJsonSources.Load` consume host requests lazily,
in order, and call the host once for each reached load. The host returns a parsed
document only after its required loading, validation and input cleanup succeed.
The sequence stops on its first request or load error. It neither sorts nor
deduplicates requests or labels. The caller can lower the inclusive 1,000,000-load
limit; zero admits an empty sequence. Checking for an excess document consumes
its request, but does not call the loader. Rust returns the host error in `Host`;
C# does not wrap it. Request-iterator teardown follows the respective language's
normal drop/disposal rules; teardown failures have no shared precedence contract.
C# re-enumeration starts a new run; Rust callers construct a fresh iterator.

These APIs supply occurrence traversal and a host callback boundary. Filename
enumeration, validation annotations, numeric conversion, graph admission and
output publication still require explicit policies. Count and per-document parser
limits are not an aggregate process-memory bound.

`occurrences.json` specifies a self-authored complete compact output document.
Both runtime fixtures load its three inputs, select every string-valued `value`
under each array-valued `rows` member, recover `owner` from that value's exact
parent, and aggregate one output with the ordinary JSON writer. Missing and
wrong-kind selections contribute no rows. The fixtures also check repeated labels,
duplicate members, retained parents, early termination, loader limits and injected
host failures. They exercise runtime composition, not imported/generated mapping
admission or a vendor's loading/validation contract.
