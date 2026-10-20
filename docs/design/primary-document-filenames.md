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
