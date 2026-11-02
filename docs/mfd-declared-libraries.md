# Declarative MFD function libraries

Filesystem import recognizes document-local declarations:

```xml
<settings><libraries>
  <import path="../libraries/helpers.mfd" saverelative="1"/>
</libraries></settings>
```

This is a bounded metadata profile, not general library compatibility or proof
of execution in a reference application. Immutable byte-package import explicitly
refuses declared libraries, even when matching entries are supplied; it never
falls back to the filesystem. Ordinary packages without these declarations keep
their existing behavior.

## Ownership and acquisition

Natural references are relative to their declaring document and confined to
the approved package root (the main document's directory by default). There is
no basename search, global library lookup, alias remapping, or case fallback.
Native rooted reads refuse links and special files. Parsing uses the same
immutable buffer whose digest is recorded.

Relocated root libraries require `ResourceBindingKindV1::MappingLibrary` through
`ImportOptions::with_resource_bindings_v1`. Existing exact origin/reference
selectors, origin hashes, root hashes, dependency pins and `prepare` snapshots
apply. Transitive file imports stay relative to their owning library under the
same authorizing root. Separate relocation of transitive owners is unsupported.
Hash failures are authoritative and do not select natural-file fallback.

Each document exports only its own exact `library`/`name` declarations; imports
are not re-exported. Local definitions precede imports of the same symbol.
Duplicate local declarations and ambiguous imported symbols are refused, not
resolved by import order. Ambiguity remains diagnostic when a local definition
shadows it. Acquisition of the complete file closure precedes registration.
The library's main mapping, payload paths and adjacent executable modules are
never imported or evaluated. Missing or unsupported declared libraries do not
authorize module discovery or function-name substitution.

## Admitted function and call shape

Functions use the existing scalar UDF reader. They have one explicitly typed
scalar output, named mandatory scalar inputs, regular call boundaries, and
complete function-owned port wiring. Saved call `componentid` values bind to
definition input/output component IDs, not display names. IDs and ports are
document-local. Calls remain `UserFunctionCall` nodes, preserving existing input
and output type adapters and eager argument consumption, including unused typed
arguments. Supported conditional branches inside the function remain lazy.
Owned helper graphs do not reopen libraries during execution or export.

Every connected argument feed must have exactly one raw output declaration and
no input-direction claims in its active graph before call admission. Source
readers can retain input keys in normalized source-port maps, so output-only
counts and normalized maps do not prove ownership. This includes unused typed
arguments and connected inputs of intermediate producers. Duplicate or opposite-
direction claims are refused regardless of owner/order or whether the colliding
input is connected. Nested function graph namespaces stay independent. Graph
edges do not add pin declarations: one producer may feed multiple consumers.

Only direct ordinary XML scalar target consumers are admitted. Their selected
content root, explicit unqualified `document/@instanceroot`, nonempty schema
reference, and raw port declarations must agree with the XML reader. The
exporter's namespace-verified `FileInstance/document` wrapper is recognized.
Filename sockets, normalized `FileInstance` aliases, duplicate raw input keys,
pass-through variables, output-bearing consumer owners, unconsumed outputs,
failure predicates, forwarded expressions, and root-function ABI call roles are
explicit refusals. A transitive consumer walk cannot erase a variable boundary.

Nested calls, optional/defaulted or inline imported signatures, multi-output
functions, structured ports, unproved namespaces/context attributes and unknown
metadata are refused before recipe expansion. Every body component is checked,
including unused components. Unknown, resource-reading, recursive, random,
clock, delay and origin-sensitive bodies are not pruned or stubbed into support.
Built-in, schema, node-function and web-service owners cannot be claimed by a
declared kind-19 call. Unsupported `.mff`, DLL or XSLT imports are not interpreted
as MFD declarations. Existing embedded UDF behavior remains separate.
In declared-library mode, numeric aliases of kind 19 such as `019` and `+19`
are refused before builtin, schema, HTTP or module dispatch. Other kinds and
ordinary imports without declared libraries keep their existing admission.

## Limits and private evidence

One file closure admits at most 32 files, 8 MiB per file, 32 MiB of delivered
bytes (repeated deliveries count), 16 document levels, 128 import edges, 512
declarations and 131,072 XML nodes. DTDs, entities, file cycles and digest
mismatches refuse. Symbols are at most 256 UTF-8 bytes and literals 1,024 bytes.
Scalar pin positions are unique and below 64; parameter/output slots use
position zero. Graph edges must stay within the definition's owned direction.

A shared sticky quota reserves at most 65,536 construction work units and
32 MiB of decoded/copied text before metadata, expression and helper-graph
allocation. The 256-level expression ceiling applies during construction;
synthetic adapters receive conservative work and depth charges. Work units
include metadata and argument slots, so this is not a promised final graph
size. Expanded definitions move between owners instead of being cloned before
a post-hoc size check. Budget exhaustion refuses the import.

An optional `ResourceReadSetV1` records consumed library-buffer hashes.
`mapping_library_definitions()` also retains the exact declaring document/hash,
reference, resolved document/hash, qualified symbol, raw declaration XML and
definition refusal. This bounded host-private evidence is never serialized in
the project. Unpinned transitive reads are recorded, not claimed authorized by
hash. Delivery or a supported leaf declaration does not establish whole-import
admission, reference parity, or compiled Rust/C# equivalence.

The ownership model follows the public descriptions of
[function libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_manage.html),
[local and global libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_local_vs_global.html)
and [relative library paths](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_relative_path.html).
Those descriptions do not supply missing function bodies or establish the
identity of similarly named or historical libraries.
