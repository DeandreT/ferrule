# Primary document filename plans

`mapping::DocumentProject` is a distinct, versioned artifact containing a
mapping project and its primary filename graph root. It retains acquisition
metadata without changing the legacy `Project` format or its saved preview
paths.

The serialized wrapper has three required fields:

- `format`: `"ferrule_document_project_v1"`.
- `project`: the embedded mapping project.
- `primary_source`: an object with a `filename` node ID from that project's
  graph and an optional `selection`.

`selection` states how the evaluated filename selects documents:

- Absent or `"direct"`: the filename names one document; `*` and `?` are
  ordinary characters.
- `{"file_name_wildcard": {"empty": "refuse" | "map_no_documents"}}`: `*` and
  `?` in the final path component select every matching file in one directory,
  in Unicode scalar order of their names; directory components never contain
  wildcards. `empty` states whether a wildcard that matches no file is a typed
  selection failure or a run over zero documents.

A direct plan is saved without `selection`, so its bytes are unchanged. A
saved wildcard plan is refused by readers that predate the field rather than
being read as a direct filename.

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

`engine::evaluate_primary_filename` evaluates the classified literal or global
parameter without a source instance. It preserves the exact string, reuses
ordinary parameter coercion, and reports explicit null results separately from
missing parameters. Neither saved previews nor parameter previews supply a
filename, including in a preview execution context. Unrelated mapping nodes are
not evaluated.
Success validates only this filename expression, not the embedded mapping body.

Document loading, source ownership and output-document execution require their
own validated profiles. Filename evaluation does not interpret empty strings,
wildcards, URI schemes or filesystem syntax, and does not enable I/O.

## Generated filename companions

`codegen::lower_document` produces a `DocumentProgram` that retains a separate
`PrimaryFilenamePlan` alongside the ordinary lowered mapping. It applies the
same closed filename classifier and ordinary mapping validation; it does not
silently lower an unsupported filename as if acquisition were absent.

The Rust and C# `emit_with_primary_filename` entry points add
`evaluate_primary_filename` / `EvaluatePrimaryFilename` to the ordinary typed
instance library. Each takes only the execution context, not a source instance.
The method preserves literal text and resolves parameter values without using
previews. Explicit nulls produce a typed `PrimaryFilenameType` error rather than
an empty path or a missing-parameter fallback.

These companions expose filename evaluation, not document execution. Ordinary
instance mapping remains a separate, explicitly pre-acquired-input operation.
Neither the companions nor the ordinary instance entry points perform source
acquisition or claim filesystem, wildcard, parser or publication support.

## Importing connected CSV filename metadata

`mfd::import_csv_document` and `import_csv_document_with_options` return an
`ImportedDocument` containing a `DocumentProject`, the ordinary import warnings
and the mapping path. They retain the connected primary CSV `FileInstance`
input expression before pruning its graph dependencies. The selected source's
data ports identify the owning declaration; a component label or declaration
order alone is insufficient. Ambiguous feeds, missing connections and unsupported
wrapper controls are refused.
An optional header containing an empty namespace list, or the native empty and
MapForce protocol namespace pair, is metadata rather than another file wrapper.
Declared wrapper namespace slots must resolve to the protocol domain. Other
header content, unresolved slots and multiple headers remain refused.

This is a repair-oriented metadata import, not an executable profile. All
ordinary warnings, including unsupported input/output filename behavior, remain.
A retained source-dependent expression still fails the filename classifier and
document lowering. The APIs neither read saved preview instances nor replace
the connected expression with a saved path. Ordinary import, pipeline import
and `ImportProfile::Executable` keep their existing behavior.

## Complete primary CSV documents

Filename evaluation, selection, loading, parsing, mapping and publication are
separate stages:

1. The filename plan is evaluated once, without a source instance or preview.
2. A `format_csv::PrimarySourceHost` receives the filename text and the plan's
   selection mode, and returns logical requests in load order. The host
   chooses where requests load from; it does not choose the selection mode.
3. The host loads each request's bytes, which are parsed as one CSV document
   with its own origin and local row positions.
4. Only after every selected document is parsed does the ordinary mapping run,
   once, over the rows of all documents in selection order. Grouping, sorting
   and aggregation therefore span documents, and every row shares one
   execution context. The mapping never runs per discovered file.
5. The caller publishes the result.

The first failure ends the run before any later request is loaded and before
the mapping runs. `engine::run_primary_csv_document` refuses an unsupported
filename expression, then validates the embedded mapping, before selecting
anything, and returns the target instance; failures
keep their stage (`Invalid`, `Acquisition` with the filename, selection,
request, document-limit, load or input cause, or `Mapping`).

Generated libraries add the same entry point to the CSV document adapters:
`codegen_rust::emit_with_primary_csv_documents` adds
`execute_primary_csv_document_bytes` and `PRIMARY_FILE_SELECTION`, and
`codegen_csharp::emit_with_primary_csv_documents` adds
`ExecutePrimaryCsvDocumentBytes` and `PrimaryFileSelection`. They take the
execution context, a host, a byte-decoding policy and a document limit, and
return one complete CSV document. They accept no filename or document content
from the caller; the embedded plan alone names the documents. Rust failures
are `PrimaryCsvDocumentError::{Acquisition, Output}`; C# raises the
acquisition exception, with the host's exception as its cause, or the mapping
and CSV output exceptions.

## Local filesystem host

`format_csv::LocalPrimaryFiles` (C# `FerruleLocalPrimaryFiles`) is the explicit
local filesystem host. Nothing else performs filesystem access, and no host
reaches a network: a host that wants another transport must be supplied
explicitly.

- Filenames are relative paths beneath one configured root. `/` and `\` both
  separate components; empty and `.` components are ignored, and requests
  use `/` between the remaining components.
- URI schemes, rooted, drive and UNC paths, parent components and any other
  colon are refused before the filesystem is touched.
- Every component beneath the root must be a directory or regular file, not a
  symbolic link. A wildcard selects regular files only, skips directories and
  refuses a matching link. These checks are not atomic against concurrent
  changes to the tree.
- Matching is case-sensitive over Unicode scalars; names that are not valid
  Unicode never match.
- Each document is bounded by a byte limit, 8 MiB by default.

Typed refusals are `Root`, `Uri`, `Rooted`, `Parent`, `InvalidName`,
`WildcardDirectory`, `NoMatches`, `Missing`, `Symlink`, `NotRegular`,
`TooLarge` and `Io`. A missing direct file is a load failure; a missing
wildcard directory or a refused empty match is a selection failure.

The portable cohort in `conformance/primary-filename-documents` compares
complete documents, typed failures and the ordered host calls of the
interpreter, generated Rust and generated C#.

## Command-line, editor and generation routes

A saved document project is the `document_project` file artifact; it is never
read as an ordinary project, and an ordinary project is never read as a
document project.

- `ferrule run-document --project DOCUMENT --input-root DIR` runs it through
  the local host. `--max-document-bytes` and `--max-documents` bound the
  selection, `--param NAME=VALUE` supplies parameter filenames and mapping
  values, and `--output` defaults to the saved target path. Both endpoints need
  an explicit CSV identity. The output is published by staged replacement only
  after the whole run succeeds, and never replaces the project or a document
  the run read. `cli::execute_primary_csv_document` is the same run without
  publication.
- `ferrule generate --primary-csv-documents` generates the document library;
  `--raw-delimited-crlf` selects raw output. The generated identity and
  compatibility manifest options apply, and the manifest records the
  `primary_csv_document` or `primary_raw_delimited_document` profile. The
  backends' `Generator::emit_with_primary_csv_documents` methods return the
  free functions' artifacts under default options.
- The editor opens a document project with its filename plan, saves the plan
  back unchanged and shows an input root (initially the project's folder) and
  limits in place of the input file. Run publishes through the command-line
  run; Preview performs the same run in memory. Debug Run, Debug Preview and
  library generation are not offered for document projects.
