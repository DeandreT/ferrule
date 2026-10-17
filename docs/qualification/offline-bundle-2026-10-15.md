# Offline bundle producer qualification

Local evidence for [#105](https://github.com/DeandreT/ferrule/issues/105),
2026-10-15, against main `7ff83194882f49285b47d14bdc44464d44bf8dbe`.
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

## Checks and originals

Toolchain: Rust `1.97.0-nightly (9838411cb 2026-04-25)`, Linux x86_64 GNU,
locked dependency versions, two build jobs, incremental disabled and dev/test
debug information disabled. The one lockfile change adds the CLI's dependency
edge to an already locked Unicode normalization package; no version changes.

| Check | Result |
| --- | --- |
| `cargo +nightly test --locked -p cli --test offline_bundle -- --test-threads=1 --nocapture` | 12 passed |
| `cargo +nightly test --locked -p cli --lib` | 96 passed, two ignored; includes both staged validation/publication controls |
| `cargo +nightly test --locked -p cli --test import_mfd_json_schema_root --test export_mfd_paths` | All ten existing tests passed |
| `cargo +nightly clippy --locked -p cli --all-targets --no-deps -- -D warnings` | Passed for every CLI target |
| `cargo +nightly fmt --all --check`, diff whitespace checks and documentation links | Passed |
| Independent retained bundle inventory verification | Every recorded file's full bytes match its SHA-256 and length |

The wider Clippy command without `--no-deps` stops in unchanged XML source at
`parser_guards.rs:202-207` with `clippy::collapsible_match`.
[#760](https://github.com/DeandreT/ferrule/issues/760) tracks that separate local
toolchain repair. The passing CLI-only lint does not qualify dependency lint or
claim hosted CI failure/success.

The first integration run passed eight controls but failed a collision fixture's
setup: it attempted to create a child beneath an existing file. The corrected
fixture uses a source directory whose destination conflicts with generated
`project.json`; all original refusal criteria remain. That failed run is retained
separately from the later positive runs.

Final production/test source identities:

| File | SHA-256 |
| --- | --- |
| `crates/cli/src/bundle.rs` | `7dd685df9c35bb22208a6a533fb3527e40f84711976d2ea7f6fe5ff8c6ed4a84` |
| `crates/cli/tests/offline_bundle.rs` | `22162ebaf87326db44a8b1bc405315c243c6480f88c8be8f8edd0264cbc499b4` |
| `crates/cli/src/code_generation.rs` | `ec84885ca6bd82ad8cb6ec0b5ca0b872dd24cedb4a6ee5e7544ae83fe9639c13` |
| `crates/cli/src/project_paths.rs` | `3afeb9edad7c3aa118cc852f3d70811aff48f4b21d9b3e76f40791dfa419477b` |

Complete local logs and test roots are retained under the isolated worktree's
`target/qualification/105-*`. Source originals and all output bytes are available
there; retained test roots use `FERRULE_BUNDLE_EVIDENCE_ROOT`. Ordinary runs use
exclusively created temporary roots and clean up their own files. Build-cache
cleanup must preserve the qualification directory.
