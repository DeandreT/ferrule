# Offline bundle producer qualification

This 2026-10-15 record covers the Linux producer at
`7ff83194882f49285b47d14bdc44464d44bf8dbe`.
The [command contract](../offline-bundles.md) defines the producer profile,
included-data policy, resource declarations, budgets and runtime requirements.

## Reached behavior

The authored corpus constructs a real Project with primary/named JSON inputs,
primary/named outputs and an exact Float(1.0) graph literal. It exercises the
public library exporter and the actual CLI. The complete project model is
compared after independent literal path rebases, and the float tag/bits remain
exact through the existing file codec.

- Default export succeeds with both configured instance files physically absent.
  Its complete tree has exactly four files: project, package manifest, inventory
  and the one declared schema. The unrelated synthetic credential sentinel is
  absent. The CLI's explicit input opt-in produces five files when no additional
  resources are requested.
- Two opt-in exports have identical complete file trees/bytes. After deleting
  the source tree and relocating an export, the real CLI reads both retained
  inputs and produces the complete expected primary/named JSON values. These
  exports have six producer files apiece.
- A separate copied MFD design imports with `--require-executable` after deleting
  and relocating its original package. Its two declared JSON Schema catalogs
  retain declaration precedence despite reversed lexical artifact order. The
  resulting typed Int field runs to the expected XML `Output/Code = 42`. This
  bundle has nine producer files.
- Every file/hash/length entry in all five retained successful final-run bundles
  was checked again after execution. Inventories remain versioned and relative;
  no absolute source-root identity is recorded.
- Refusals cover absolute/drive paths, unsafe traversal, noncanonical resource
  paths, reserved filenames including superscript device forms, missing files,
  duplicate declarations, case/Unicode aliases, file/directory collisions,
  contained/outside/dangling symlinks, nonregular resources, configured metadata
  aliases, data exclusion, oversized files/counts and a separate path-metadata
  budget. HTTP/dynamic sources and SQLite input copying refuse this profile.
- Existing destination directories, regular files and dangling links remain
  exact. Separate physical publication controls reject staged validation failure
  without publishing, and protect a sentinel directory created after preflight.
  Both controls verify owned staging removal.

These are finite Linux producer, relocation, existing-import and native execution
checks. They do not qualify another producer platform, every resource grammar,
schema dependency discovery, generated code, GUI workflows, external applications
or power-loss/network-storage durability.

## Build and evidence

These results were observed with Rust `1.97.0-nightly (9838411cb 2026-04-25)`,
Linux x86_64 GNU, locked dependencies, two build jobs, incremental compilation
off and dev/test debug information off. The [producer fixture](../../crates/cli/tests/offline_bundle.rs)
exercises the public exporter and CLI. The retained inventory was checked
against each file's full bytes, length and SHA-256; only CLI lint was qualified.

Measured source identities:

| File | SHA-256 |
| --- | --- |
| `crates/cli/src/bundle.rs` | `7dd685df9c35bb22208a6a533fb3527e40f84711976d2ea7f6fe5ff8c6ed4a84` |
| `crates/cli/tests/offline_bundle.rs` | `22162ebaf87326db44a8b1bc405315c243c6480f88c8be8f8edd0264cbc499b4` |
| `crates/cli/src/code_generation.rs` | `ec84885ca6bd82ad8cb6ec0b5ca0b872dd24cedb4a6ee5e7544ae83fe9639c13` |
| `crates/cli/src/project_paths.rs` | `3afeb9edad7c3aa118cc852f3d70811aff48f4b21d9b3e76f40791dfa419477b` |

To retain the fixture's generated evidence during a new run, set
`FERRULE_BUNDLE_EVIDENCE_ROOT` to a fresh destination. Otherwise the fixture
uses its own temporary roots.
