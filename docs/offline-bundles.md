# Offline project bundles

`bundle-project` packages one Ferrule project and an explicitly selected set of
local resources. It writes a new complete directory in one atomic publication.
The command keeps the existing package-import manifest and resource resolver;
it does not create another application's proprietary package format.

```sh
ferrule bundle-project \
  --root package \
  --project projects/orders.json \
  --resource maps/orders.mfd \
  --resource resources/json/shared/orders.schema.json \
  --json-schema-catalog resources/json \
  --out orders-bundle
```

`--project`, `--resource`, `--edi-catalog` and `--json-schema-catalog` name paths
relative to `--root`. Repeat resource/catalog flags as needed. Resource flags
select individual files; catalog flags declare directories for the existing MFD
import search. Catalog declarations copy no additional files. Declare every file
you want retained, including transitive schema/configuration dependencies. Each
catalog must contain at least one declared resource file.

The command writes the rebased mapping as `project.json` at the bundle root.
Declared resources keep their source-root-relative locations, so relative
references between them keep working. Existing static input/output paths are
rebased using Ferrule's project path codec. For example, a project stored at
`projects/orders.json` with `source_path: "../maps/input.json"` receives
`source_path: "maps/input.json"` in the bundle. The graph, schemas, embedded
format configurations, optional values and exact finite float tags/bits are
preserved through the project file codec.

## Instance data and execution

Configured input files are omitted by default, without opening them. A declared
resource that is also a configured input refuses unless `--include-inputs` is
present. Add that flag to include the primary and static named input files:

```sh
ferrule bundle-project \
  --root package --project projects/orders.json \
  --include-inputs --out orders-with-inputs
```

No directory scan, credential discovery, network fetch or implicit resource
discovery occurs. Explicitly selected project/resource contents are retained;
the exporter does not classify arbitrary file contents or redact values from
your selected project. It does not automatically copy output artifacts, GUI
layout sidecars, compiled programs or system dependencies. An update-existing
workbook template can be an explicitly declared resource. A configured SQLite
instance cannot be copied through `--include-inputs`: it requires a separate
database snapshot/backup contract, including live journal state.

This profile accepts static local sources. HTTP transport and dynamic source
loading refuse before publication; they cannot establish a finite offline input
set through this command. Pipelines have their own document/path model and are
outside this single-project profile.

Move the completed directory to its new location, then run its `project.json`.
Input paths keep identifying the intended formats even when their data was
omitted. Supply omitted primary data through `run --input`, place named data at
the inventory's logical paths, or explicitly include static inputs when creating
the bundle. A Ferrule CLI or compatible Rust host is required at the destination.
Runtime parameters and any opted-in packaged PDF worker remain host
requirements. The ordinary format/engine admission rules still apply.

```sh
ferrule run --project /new/location/orders-bundle/project.json

ferrule import-mfd \
  --mfd /new/location/orders-bundle/maps/orders.mfd \
  --package-manifest /new/location/orders-bundle/ferrule-package.json \
  --out /new/location/orders-bundle/imported.json
```

The copied resources are the complete declared set, not a claim that an arbitrary
MFD/schema/configuration is executable or that all of its dependencies were
declared. Ordinary strict import (`--require-executable`) can verify a retained
design under its existing capability profile. Import never gains access to the
exporter's original machine paths through the bundle manifest.

## Manifest and inventory

`ferrule-package.json` uses the existing strict import format:

```json
{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [
    {"kind": "json-schema", "root": "resources/json"}
  ]
}
```

`ferrule-bundle.json` is a version-1 producer inventory with kind
`ferrule.project-bundle`. It records the source project's relative name and hash,
the bundled project name, data-inclusion policy, every retained file's relative
source/destination, sorted roles, SHA-256 and byte length, and every configured
path rebase by owning field. Files are listed in deterministic path order.
Shared static input paths copy once, with their declared roles retained.
The inventory excludes itself from its own file/hash list. It contains no
absolute source root, timestamp, inode or build-machine identity.

For identical source bytes and the same ordered request, every output byte is
deterministic. Catalog search precedence remains declaration order within each
existing catalog kind. `bundle-project` groups EDI catalog flags before JSON
Schema catalog flags; the two kinds have independent importer searches.

## Confinement and publication

Selected resource/project/catalog names use normalized relative paths with
forward slashes. Absolute paths, drive/URI spellings, parent/current/empty
components, nonportable reserved names and filename characters refuse.
Configured project input/output references may use safe parent components to
reach another location inside the chosen source root; traversal outside it
refuses. Case and Unicode-normalization aliases, file/directory collisions,
duplicate resource declarations, missing files and nonregular files refuse.
Bundle metadata names are reserved. Existing intentional selected-target output
path sharing remains part of the project.

Every descendant source component is opened relative to a held directory with
no-follow flags; even a contained symlink refuses this producer profile.
Complete reads check size and held/named metadata before accepting their bytes.
The source root itself is explicitly chosen by the host. This is acquisition
confinement, not a sandbox for the mapping's future execution.

The initial producer uses descriptor-relative Unix file APIs and the existing
atomic directory publisher. Unsupported producer hosts or filesystems refuse
with their original cause; the resulting logical bundle is portable. Linux
execution is the qualified producer profile. Other producer platforms need
their own verification before an execution claim.

The destination must not exist, including as a dangling symlink. Files are
written under a fresh sibling staging directory. The existing package-manifest
loader validates the staged catalogs before publication. Atomic no-replacement
publication protects a destination that appears after preflight; failures keep
existing destination content exact. Staging cleanup is best-effort on failure.
There is no partially published destination tree, and no crash/power-loss
durability claim.

The producer caps individual files at 64 MiB, retained content at 256 MiB,
artifact count at 4,096, catalog count at 64 and logical paths at 4,096 UTF-8
bytes and 64 components. Its collision index has a separate 8 MiB retained
string-byte budget; tree nodes and allocator overhead are additional. These are
application payload/metadata budgets, not a process RSS bound.
Acquisition and publication materialize the declared files; this is not a
streaming export. The existing package manifest also retains its 64 KiB limit.

The filename policy includes the ordinary reserved device spellings and their
superscript-digit forms documented in
[Microsoft's filename conventions](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).
