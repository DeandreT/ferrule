# Explicit immutable resource bindings

The `mfd` host API can opt into typed, digest-pinned bindings for XML Schema, JSON Schema and
the ordinary EDI configuration reader. It does not modify a mapping or recreate
an obsolete installation directory. Existing imports and version-1 package
manifests keep their unbound lookup and fallback behavior.

## Host authorization

Construct `ResourceBindingV1` with:

- `ResourceBindingKindV1::XmlSchema`, `JsonSchema` or `EdiConfig`;
- the existing original MFD pathname (canonicalized for exact origin identity);
- the **exact original reference string**, including its separators/prefix;
- an explicitly trusted existing resource root and root-relative target;
- the host-approved SHA-256 of the raw root file bytes.

Register it with the fallible `ImportOptions::with_resource_bindings_v1`. No
binding is inferred from a basename, a similar schema, a neighboring repository,
or an automatically discovered manifest. Duplicate origin/reference selectors,
including selectors with different kinds, are errors rather than precedence
choices. A binding registered for another MFD or another reference is not used.

```rust,no_run
# fn configure(mapping: &std::path::Path, package: &std::path::Path,
#              catalog: &std::path::Path, approved_root_sha256: [u8; 32],
#              approved_types_sha256: [u8; 32]) -> Result<mfd::ImportOptions, mfd::MfdError> {
let binding = mfd::ResourceBindingV1::new(
    mfd::ResourceBindingKindV1::XmlSchema,
    mapping,
    "RelocatedInput.xsd",
    catalog,
    "input.xsd",
    approved_root_sha256,
)?
.with_dependency_pin("types/value.xsd", approved_types_sha256)?;

let options = mfd::ImportOptions::default()
    .with_package_root(package)
    .with_resource_bindings_v1([binding])?;
# Ok(options)
# }
```

The digest is an authorization input. Computing a hash of whatever untrusted
file happens to exist and calling that approval does not establish provenance.
`with_origin_sha256` optionally pins the exact MFD input bytes too; that check
uses the same retained original buffer that is parsed during a bound import.

## Actual consumed bytes

The host binding reader opens a resource beneath its directory capability,
captures bounded raw bytes, checks the provided digest, and returns an immutable
`Arc<[u8]>` to the actual format parser. Metadata, named-type and repeated schema
queries share the binding's cache. They do not hash a pathname and then reopen
it for parsing. UTF-8/UTF-16 schema decoding occurs after the raw-byte check.

Every transitive dependency remains under the binding's trusted root. A caller
may provide additional root-relative hashes with `with_dependency_pin`. A
**root-only hash does not seal a transitive closure**: an unpinned contained
dependency may be read, and its actual digest is recorded without an expected
digest. A mismatch in any supplied pin is authoritative and prevents an import
result from being published, including best-effort/profile/pipeline imports.
It cannot select a package alternative or silently fall back to an entry tree.

`ResourceBindingV1::prepare` explicitly captures the root and provided dependency
pins before import, without parsing or executing a mapping. Those bytes remain
the import's snapshot even if a live file later changes. It does **not** capture
unlisted dependencies. Snapshot receipts must not be interpreted as a claim
that the current live pathname still has the snapshot hash.

## Boundaries and limits

Bound reads reject symlinks/reparse points, dangling links, traversal, special
files, noncanonical resource-name aliases, device/UNC/alternate-stream targets,
and oversized inputs. Target and dependency paths are portable root-relative
paths. Contained `..` components in schema include references are normalized
within the existing authority, not used to escape it. Pin paths are validated
as exact contained identities, including before prepared consumption.

Unix reads use descriptor-relative `openat` with `NOFOLLOW`; Windows holds
ancestor directory handles against write/delete replacement, opens reparse
points themselves, and checks the opened object before any body read. Handles
are owned and close when the import/preparation scope ends. The new bound mode
is intentionally stricter than unbound contained-symlink handling. Unsupported
platforms or sharing/permission failures refuse rather than weakening checks.

Windows resource identities use handle-derived volume-GUID paths rather than
mutable DOS-drive or short-name aliases; those locations remain private receipts.

The binding set is limited to 64 entries and 4096-byte path/reference inputs.
Root and resource path depth is limited to 64 components; exact-name directory
validation is bounded to 4096 entries on Unix. XML binding caches allow at most
64 unique files and 64 MiB of raw bytes; JSON allows 256 files and 64 MiB;
EDI allows 32 files and 8 MiB. The whole
import's binding captures have a 128 MiB aggregate budget. Repeated cached
reads do not consume a second byte budget.

Byte-read requests, including cache hits, are capped at 4096 per binding import
session and per supplied XSD reader operation.

The ordinary EDI backend uses exact selected-message sibling identities such as
`850.Config`; bound mode does not scan other configuration contents, discover
an installation, or extract an archive. Dialect-specific IDoc/SWIFT backends
without supplied-byte support refuse an explicit binding rather than using a
path-only fallback.

## Private read evidence

An optional `ResourceReadSetV1` records complete captures that were supplied to
a reader, and captures rejected by a digest mismatch:

```rust,no_run
# fn configure(options: mfd::ImportOptions) {
let readset = mfd::ResourceReadSetV1::default();
let options = options.with_resource_readset_v1(readset.clone());
// Run a public import with `options`, then inspect private host evidence.
let records = readset.records();
# let _ = (options, records);
# }
```

Each record separates original kind/origin/reference and original MFD digest
from resolved resource identity, observed raw digest, optional expected digest,
delivery status and explicit prepared-snapshot status. Delivery is not proof
that parsing/import succeeded. A permission/path/size refusal has no fabricated
whole-file digest. The append-only sink is host-selected and may collect more
than one import; it is not serialized into a portable Project or used to
overwrite the original request. Resolved paths are private provenance.

The sink is capped at 8192 records. `take_records` drains it between host calls;
exhausting an explicitly requested sink refuses delivery instead of silently
omitting evidence. No sink is retained or populated unless the host requests it.

## Format-reader APIs

`format_xml::xsd` exposes additive root/type/base entry points accepting an
`XmlSchemaByteReader`, plus raw-byte decoding for annotations.
`format_edi_config::import_config_with_byte_reader` accepts an
`EdiConfigByteReader`. `format_json::json_schema::import_with_byte_reader`
accepts a `JsonSchemaByteReader` for the root and relative local-file `$ref`
dependencies, retaining the ordinary schema semantics and import limits.
JSON components, string parsers and HTTP request schemas use the same typed
binding route. These readers receive approved absolute identities and
supply immutable raw buffers; the host reader owns physical authorization and
any digest policy. Their parsing paths do not reopen supplied identities.
The mfd binding implementation supplies the rooted, digest-checking host reader.

These APIs establish resource selection and consumed-byte integrity. They do
not certify the semantics of a schema, infer missing original authority,
execute original mappings, or replace document-output qualification.
