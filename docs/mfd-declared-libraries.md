# Declarative MFD function libraries

The importer reads document-local declarations of this form:

```xml
<settings><libraries>
  <import path="../libraries/strings.mfd" saverelative="1"/>
</libraries></settings>
```

There is **no import alias**. Functions retain the exact `library` and `name`
attributes on their direct declaration components. Saved call `componentid`
values must match that definition's input/output component IDs, not similar
parameter names. IDs and ports are local to each document and definition.

## Authority and ownership

- A natural reference is relative to its declaring document and confined to
  the explicitly approved package root (the main document's directory by
  default). No basename search, sibling-project discovery, global application
  libraries, or case-insensitive fallback selects another file.
- A relocated root library requires `ResourceBindingKindV1::MappingLibrary`
  through `ImportOptions::with_resource_bindings_v1`. It uses the existing
  exact original-origin/reference selector, expected SHA-256, optional
  `with_origin_sha256`, dependency pins, and `prepare` snapshots. Never label
  library metadata as an XML schema or EDI configuration.
- Transitive imports are relative to the owning imported document and confined
  to the **same authorizing root**, not the main mapping's resources or an
  unrelated catalog. Pins on the root binding can seal these dependencies.
  Separate relocation bindings for transitive owners are not supported.
- Native rooted reads refuse links/reparse points and special files. Parsing
  consumes the immutable delivered buffer whose hash is recorded; it never
  hashes a pathname and asks an XML parser to reopen it.
- Each document resolves its own declarations against only its own direct
  imports. A main-mapping definition cannot satisfy an imported nested call.
  Local declarations take precedence over imports of the exact same symbol.
  Conflicting declarations and ambiguous same-symbol imports are refused;
  no import-order precedence is guessed. Ambiguity still produces a diagnostic
  even if a local declaration subsequently shadows that symbol.
- Transitive symbol **re-export** is deliberately unsupported: import the
  defining document directly when calling its function from the main mapping.
  The available documentation establishes dependency trees, but not sufficient
  ownership rules for guessing re-export visibility or alias remapping.

The library's main mapping is never imported or evaluated. Its payload paths,
HTTP/database components and adjacent code modules are not opened. A missing
library remains an unresolved resource/required-producer diagnostic. Kind-19
calls in a mapping declaring MFD libraries do not fall through to custom-module
discovery or captured-output/name substitution.

## Initial supported contract

Imported definitions use the **existing scalar UDF reader and materializer**,
not a separate evaluator. They require named, explicitly typed scalar inputs
and outputs, complete function-owned port wiring, and supported scalar bodies.
Typed input defaults remain owner-local and are used only for an omitted
connection; a connected null is not an omitted argument. Required inputs without
defaults must have a connected producer. Multiple scalar outputs require an
explicit inline declaration.

All body components are checked, including unused ones. Unknown/custom,
structured, resource-reading, recursive, runtime-clock/random/delay and
origin-sensitive filepath bodies are refused rather than stubbed, pruned into
partial support, or passed to schema/module recognizers. This also refuses
unproved requiredness/cardinality/context metadata, XML namespaces on function
metadata, reserved built-in/node-function library ownership, and unknown library
formats (`.mff`, DLL, XSLT, etc.) in this declared-MFD ingestion path. Ordinary
embedded UDF lowering remains in place.

Supported scalar operations may still fail at runtime. Their existing lazy
branches, dependency and failure handling are reused; no purity-to-totality
assumption is made. The main root-function ABI is unchanged. Portable projects
contain owned existing recipes/helper graphs, not live external callbacks.
Existing export support applies to those graphs; unsupported graph shapes keep
the existing precise export refusals rather than emitting an opaque import.

## Budgets and private evidence

One closure is limited to 32 files, 8 MiB per file, 32 MiB aggregate delivered
bytes (repeated deliveries count), 16 document levels, 128 import edges, 512
declarations, 131,072 XML nodes, and 65,536 aggregate expanded expression nodes.
The existing scalar depth/expansion and native rooted-path budgets also apply.
Qualified symbols are bounded to 256 UTF-8 bytes each and scalar literals to
1,024 bytes before expansion; scalar pin positions are below 256 and unique
per direction. Legacy connection vocabularies, value-map body
metadata and extra graph/context attributes are not in this initial subset.
DTD/entities, file import cycles, definition cycles and digest mismatches are
refused. Imported closure acquisition is staged before registration.

An optional `ResourceReadSetV1` records actual consumed raw byte hashes for both
natural and bound MFD libraries. `mapping_library_definitions()` additionally
retains the exact declaring document/hash/reference, resolved document/hash,
qualified function identity, full saved declaration XML (including signature,
defaults and ports), transitive function identity closure and any refusal.
This evidence is host-private, bounded, and never serialized into a Project.
Unpinned transitive bytes are recorded, **not claimed pre-authorized by hash**.

## Format rationale

Official documentation distinguishes MFD UDF libraries from custom executable
libraries, states that local library paths are relative to the declaring MFD,
describes declared library names, local-before-import precedence, and nested
library-import trees:

- [Manage Function Libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_manage.html)
- [Local and Global Libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_local_vs_global.html)
- [Relative Library Paths](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_relative_path.html)
- [UDF Basics](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_udf_basics.html)
- [mfd-filepath](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_lib_core_path_mfd-filepath.html)

These rules do not supply any absent owner's definitions or establish the
identity of a historical/similarly named library.
