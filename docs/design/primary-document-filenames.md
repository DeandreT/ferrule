# Primary document filename plans

`mapping::DocumentProject` is a distinct, versioned artifact containing a
mapping project and its primary filename graph root. It retains acquisition
metadata without changing the legacy `Project` format or its saved preview
paths.

The serialized wrapper has three required fields:

- `format`: `"ferrule_document_project_v1"`.
- `project`: the embedded mapping project.
- `primary_source`: an object with a `filename` node ID from that project's graph.

Unknown wrapper and primary-plan fields are rejected. A serialized wrapper
cannot be read as an ordinary project, because the project fields are nested.
The embedded project is exposed by shared reference, not by mutable reference.

For file persistence, use `mapping::document_project_file::{encode_pretty,
decode_str, decode_bytes}`. This reuses the bounded mapping-file codec with the
distinct `document_project` kind. When required, its version-2 outer envelope
retains every finite float's type and exact bits throughout the embedded project.
Ordinary model serde is not the lossless file contract. Non-finite model floats,
invalid UTF-8 and mismatched versioned artifact kinds are refused.

## Source-independent classification

`DocumentProject::global_filename` classifies the initial filename forms:

- A string constant retains its exact text, without path normalization.
- A string runtime parameter retains its exact name. Its name must meet the
  existing nonempty, NUL-free, 256-byte parameter-name limit.

The classifier does not use saved preview paths or parameter preview values.
It performs no filesystem access and does not decide wildcard, URI, decoding,
parsing or publication policy. Missing nodes, non-string values, invalid
parameter names and unsupported expressions have distinct typed errors.
Unsupported expressions remain in the artifact rather than being replaced.

`codegen_runtime::ExecutionContext::resolve_runtime_parameter` and C#
`FerruleExecutionContext.ResolveRuntimeParameter` resolve host scalars before
source acquisition. They share the existing scope-context coercion rules and
typed missing/type errors. Explicit null stays null, and source fields cannot
supply missing host parameters. Resolving a scalar does not itself validate a
filename, select a document or perform I/O.

## Graph retention

`DocumentProject::prune_unreachable_nodes` retains the primary filename node
and its dependencies in addition to the ordinary project's target, control,
failure and dynamic-source roots. This applies even when the filename
expression is unsupported by the classifier. Ordinary project pruning keeps
its existing roots and behavior.

This model has no interpreter or generated-host acquisition entry point.
Filename evaluation, document loading, source ownership and output-document
execution require their own validated profiles; retaining a plan does not
enable those operations.
