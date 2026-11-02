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

Raw wiring is necessary but not sufficient. A separate import-only ownership
certificate runs once after local catalog promotion, source-role refinement,
primary-source ordering and join resolution, before main graph materialization.
Reader registrations retain the immutable XML document/structure/component
identity beside their normalized records. Promoted local catalogs retain their
own origin; synthetic or unproved records cannot acquire a main-graph identity
through matching names, UIDs, keys, paths or values. Origins and certificates
are never serialized into a Project and do not change the execution model.

The certificate inspects the original producer collections, before keyed
builder maps can overwrite competing records. Every argument dependency,
explicit receiving-scope driver and declared-call output must have exactly one
normalized producer matching its proved raw owner and port. All normalized
source ports participate, including input aliases. Competing claims refuse even
when the expected main source would currently win or both producers have the
same value. The producer collections remain frozen through lowering.

The initial certified profile is ordinary unqualified XML scalar source ports,
regular declared-call boundaries, literal constants and ordinary scalar
functions with exact, complete reader-backed pin correspondence. Element versus
attribute identity and unique schema paths must agree. Repeated receiving
groups require explicit, certified source-group drivers. Implicit or projected
scope construction, qualified/alternative paths, transparent intermediates,
dynamic properties, filenames, local UDF dependencies, joins and database
computed projections remain unproved profiles. External scalar/XSLT recipes,
parsers and serializers remain blocking producer claims, not invisible routes.
Source-node rules affecting a checked argument, driver or output also refuse
until their provenance and consumption profile are proved.

Frame availability is proved separately for every receiving boundary. A frame
in one sibling receiver cannot authorize an argument in another. Root-visible
singleton values and constants remain usable across receivers; repeated source
fields require their exact source frames in that receiver's ancestor chain.
The bounded nested profile supports same-source descendant and identity driver
chains, not a second implementation of the legacy fallback resolver.

Affected raw `ferrule-scope-sources` entries are checked before the legacy map
can collapse duplicate keys. Matching hints retain the proved stored driver
path (relative for a nested descendant). Changed, duplicate, conflicting,
unknown or oversized affected hints refuse; unrelated scopes retain their
ordinary behavior. A final context seal then checks actual constructed roots,
ancestor paths, drivers and direct declared-call bindings after hint restoration
and subsequent transformations, before publication. Unproved wrappers or late
context changes refuse rather than inheriting the earlier producer proof.

XML path identity is checked for every segment. Attribute ancestors are outside
the supported profile; a leaf attribute is allowed only when its normalized
schema role agrees. Numeric prefixes may reference only the proved unqualified
zero namespace slot, including the legacy implicit zero slot. Qualified,
undefined and nonzero slots are not made unqualified by stripping their prefix.
These checks apply to arguments, drivers and receiving paths alike.

Independent local function namespaces and parameter catalogs remain local.
An unused catalog becomes relevant only if its actually promoted ports compete
on the checked closure; unrelated collisions do not become a whole-graph gate.
Ordinary imports without declared-library calls retain their existing behavior.
Certificate failure is a hard `UnsupportedImport` before a project is returned,
not a skipped call followed by source fallback. The certificate performs no
resource reads or evaluation. Earlier declaration/schema processing may already
have acquired metadata, including an unused catalog's schema; its readset is
evidence of delivery, not evidence of successful admission.

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
