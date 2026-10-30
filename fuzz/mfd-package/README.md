# MFD byte-package admission fuzz target

A standalone [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) harness for
the byte-only package boundary in `crates/mfd`: `MfdPackage::new`, which admits
or refuses a design and its resource entries, and `import_package`, which
imports an admitted package without filesystem, catalog or network access. It
is not a member of the root workspace: the empty `[workspace]` table in
`Cargo.toml` makes this directory its own workspace with its own `Cargo.lock`,
so root builds, tests and CI never compile it.

## Input format

Each input is a compact byte program (`src/program.rs`). The first byte selects
a self-authored starting package from `src/templates.rs`:

| Template | Contents |
| --- | --- |
| `empty` | no entries |
| `xml_to_json` | XML source (XSD) to JSON target (JSON Schema) |
| `json_to_xml` | JSON source (JSON Schema) to XML target (XSD) |
| `dtd_source` | XML source described by a DTD |
| `nested_design` | `xml_to_json` with every entry under `maps/` |
| `xsd_include` | a source XSD that includes its definitions from `types.xsd` |
| `json_ref` | a source JSON Schema whose property refers into `types.schema.json` |
| `edi_idoc_source` | an IDoc (`EDIFIXED`) EDI source with a supplied configuration |
| `db_source` | a database source whose connection names a supplied file |

The remaining bytes are operations that add, drop and rename entries, set the
design path, replace, splice, truncate or rewrite entry contents, copy an entry
under a case alias, move a schema behind a chain of up to 64 includes or
`$ref`s (complete, cyclic or dangling), append filler entries at and past the
size limits, and surround an entry with a repeated prefix and suffix. Path
operands come from a pool of valid names, aliases and traversal attempts
(parent and dot segments, absolute, drive, URL, backslash, control-character,
trailing-dot and over-long paths) or from raw bytes, lossily decoded as UTF-8.
Every byte string decodes: reads past the end yield zero and indexes wrap.

## What it checks

For each input:

1. **Admission.** `MfdPackage::new` succeeds exactly when the candidate obeys
   the rules in `docs/mfd-interop.md`: at most 128 entries, 4 MiB per entry and
   16 MiB in total; paths of at most 256 UTF-8 bytes without empty, dot or
   parent segments, `:`, backslashes, control characters or segments ending in
   a space or dot; no two entries equal ignoring ASCII case; and an entry named
   exactly like the design. A refusal names one of the rules the candidate
   breaks. Admission does not depend on entry order.
2. **Admitted contents.** An admitted package returns exactly the supplied
   entries, sorted by path; `get` finds each entry, refuses its ASCII case
   aliases with `MissingEntry` and refuses invalid paths with `InvalidPath`.
3. **Import.** `import_package` returns an imported project or an `MfdError`;
   a panic is a finding. Package refusals (`MfdError::Package`) are reported
   apart from other errors. Importing the package built in either entry order
   gives the same result, warnings and serialized project.
4. **No host I/O.** The import makes no filesystem, process-image, mount or
   network system call. Components whose backends need the filesystem (IDoc
   and SWIFT configurations, database introspection) refuse with
   `PackageError::FilesystemResource` instead.

## No-host-I/O boundary

On Linux x86_64, `src/sandbox.rs` installs a seccomp filter on the thread that
imports. It traps filesystem path, directory, extended-attribute and watch
calls (`open`, `openat`, the `stat` family, `access`, `readlink`, `getdents`,
`getcwd`, `*xattr`, `inotify_add_watch`, `fanotify_mark`, file handles), every
filesystem change, mounts, `chroot`, `io_uring_setup` (whose submissions
seccomp cannot see), `socket` and `connect`, and every x32-ABI call, into a
`SIGSYS` handler that counts each call made during an import. Read-only calls
and opens then run as normal, so a crash, timeout or out-of-memory report
raised during an import still writes its artifact. Filesystem changes, mounts,
`io_uring` and network calls fail with `EPERM` while an import runs, and x32
calls always do. Any counted call is a finding, named by its system call.
Threads an import spawns inherit the filter and are charged to it; monitors are
serialized, so a spawned thread's call cannot be charged to another import
running in parallel. The harness needs no scratch directory: imports have no
reason to touch the host at all. One call is not counted: glibc's allocator
opens `/proc/sys/vm/overcommit_memory` once per process, the first time `free`
shrinks a thread arena's heap, so any import can trigger it.

Process creation and program execution are refused by the filter itself,
with a seccomp errno rather than a trap: `execve`/`execveat`, `fork`, `vfork`
and a `clone` without `CLONE_THREAD` fail with `EPERM`, and `clone3` with
`ENOSYS` so glibc retries through the refused legacy `clone`. The errno is
applied by the kernel without a signal, so it holds even while the caller has
SIGSYS blocked (`posix_spawn` blocks it around its clone) and never depends on
the handler, which a created child would reset to the default; a `clone` with
`CLONE_THREAD` is allowed so threads keep working. These refusals are not
counted, because a reliable count would need the handler.

The fuzz target runs checks on one 8 MiB worker thread, so libFuzzer's own
thread stays unfiltered: a process started from a filtered thread inherits the
filter without its handler, and sanitizers start `llvm-symbolizer` from
libFuzzer's thread. `cargo test` covers the detector itself. On other platforms
the harness runs without detection, and the fuzz target and tests print a
one-time warning saying so.

## Known limits

These are current behavior that the harness tolerates rather than reports. The
first three overflow the 8 MiB worker stack or exceed the 10 s timeout; the
harness skips the import of a package that reaches one (`known_limit_exemption`
in `src/lib.rs`).

- **Deep entry trees** ([#1167](https://github.com/DeandreT/ferrule/issues/1167)).
  A design component's `<entry>` tree is walked recursively, one call per
  level, by `mfd::import::schema::typed_xml_entry_tree_schema` (entries with
  `ferrule-kind` metadata), `xml_ports::reconcile_children` (XML components),
  `collect_json_ports_with_declarations` (JSON components) and
  `mfd::import::target_node_default::collect`. The fuzz build overflows at
  1,219 nested typed entries. The harness skips a package whose design, read
  as strict UTF-8 like the importer reads it, nests more than 512 `<entry>`
  elements inside a `<component>` (`MAX_EXEMPT_ENTRY_DEPTH`).
- **Quadratic structure walk** ([#1185](https://github.com/DeandreT/ferrule/issues/1185)).
  `mfd::import::xml_root_view::Inventory::read` searches the ancestors of
  every element below the design's `<structure>` for the nearest `structure`
  and `component`, so each element costs about its depth below that structure
  and the pass costs their sum, quadratic along a deep chain. It runs once per
  imported stage over the top structure; a nested `<structure>` is not a
  separate pass and only shortens its own descendants' walks. Only deep chains
  are slow, so the harness ignores the first 64 levels of each element's depth
  (`ANCESTOR_FLOOR`) and a wide, shallow design, however many elements, is
  never skipped. On the fuzz build a chain of 4,000 plain elements inside a
  component takes 1.6 s and one of 16,000 takes 24.7 s, both quadratic. The
  harness skips a design whose deep-tail depths sum past 2,000,000
  (`MAX_EXEMPT_ANCESTOR_STEPS`), a chain of about 2,060 elements that still
  imports well under the 10 s timeout.
- **Large XML Schemas** ([#1168](https://github.com/DeandreT/ferrule/issues/1168)).
  `format_xml::xsd` imports by recursive descent with no depth limit: once per
  nested particle (`parse_element`, `parse_complex_type`,
  `collect_sequence_contents`), per named type reference or extension base
  (`resolve_complex_type`, `parse_complex_type_declaration`) and per group
  reference (`groups::parse_model_group`). Reference chains recurse as deeply
  as nesting, so the bound is the number of schema components, not their
  nesting. Below the `xs:schema` root the importer matches local names in any
  namespace, so the harness counts elements by local name (`XSD_COMPONENTS`),
  unprefixed or not, outside `annotation` subtrees. The fuzz build overflows at
  523 nested anonymous types (1,569 counted components) and at a 448-type
  named chain (1,345, a 45 KB input); each recursion level takes at least three
  counted components, since a complex type descends only through a
  compositor, group or complex content. The harness
  skips a package whose XSD documents hold more than 512 counted components
  in total (`MAX_EXEMPT_XSD_ELEMENTS`). DTD and JSON Schema imports stop at
  their own 64-level depth limits instead.
- **Silent JSON Schema substitution**
  ([#1169](https://github.com/DeandreT/ferrule/issues/1169)). A source JSON
  Schema that is not an object (`[1,2,3]`) or a content-free `$ref` cycle
  imports without a warning as a single `root` string field. The seeds
  `json_schema_not_object` and `json_chain_cycle` record this as `imported`;
  their expectations change when the importer warns or refuses.

## Per-input budget

- **Input size:** the target ignores inputs larger than 64 KiB
  (`MAX_INPUT_BYTES`).
- **Package size:** a program builds at most 136 entries, 4 MiB + 1 byte per
  entry and 20 MiB in total, enough to cross every admission limit by one.
  Every operation stops building at its entry's cap, and all `Substitute`
  operations of one program do at most 64 MiB of byte comparisons in total
  (`MAX_SUBSTITUTE_SCAN`, entry length times needle length per op), so a short
  program cannot spend the timeout decoding.
- **Time and memory:** smoke and long runs pass `-timeout=10` (seconds per
  input) and `-rss_limit_mb=2048`. libFuzzer reports a slow input or excess
  memory as a crash artifact.

## Requirements

- cargo-fuzz 0.13 (`cargo install cargo-fuzz`).
- The nightly toolchain pinned in `rust-toolchain.toml`
  (`nightly-2026-09-07`). Commands run from this directory pick it up
  automatically; from the repository root pass `+nightly-2026-09-07`.

Every command below runs from this directory. `--fuzz-dir .` tells cargo-fuzz
that this directory is the fuzz project.

## Build and smoke run

```sh
cargo fuzz build --fuzz-dir . admit
mkdir -p corpus
cargo fuzz run --fuzz-dir . admit corpus seeds -- \
  -max_total_time=120 -max_len=65536 -timeout=10 -rss_limit_mb=2048
```

`corpus/` is the writable working corpus; `seeds/` is read-only input. Both
`corpus/` and `artifacts/` are ignored by Git. `-max_len=65536` keeps the
largest seeds whole. Raise `-max_total_time` for a longer campaign; keep the
other bounds unchanged.

## Reproduce and minimize a finding

libFuzzer writes a failing input to `artifacts/admit/crash-*` (or `timeout-*`,
`oom-*`).

```sh
# Replay one input; the panic message names the invariant.
cargo fuzz run --fuzz-dir . admit artifacts/admit/crash-<hash>

# Shrink it to a minimal reproduction.
cargo fuzz tmin --fuzz-dir . admit artifacts/admit/crash-<hash>
```

Copy the minimized input to `regressions/<short-name>.bin`; `cargo test`
replays every file there. Report the defect as a separate issue with that
reproduction rather than fixing the package boundary in the harness change.

To shrink a grown working corpus: `cargo fuzz cmin --fuzz-dir . admit corpus`.

## Seeds and tests

`seeds/` is generated by `src/seeds.rs` with the program encoder. Seeds cover
every template, each path rule and case alias, entry-count and byte limits at
and one past each bound, missing direct and transitive schemas, missing JSON
Pointer targets, design, include and `$ref` references that try to leave the
package, include and `$ref` chains, cycles and dangling links, unreferenced
invalid entries, and truncated, non-UTF-8, wrong-kind and deeply nested
designs, entry trees, schemas, schema annotations and referenced instances,
a named-type chain below the XSD exemption, IDoc, SWIFT and database sources
that must refuse with `FilesystemResource`, and a substitution that would
expand a 4 MiB entry to 1 GiB without the program's per-entry cap. `regressions/deep_design_xml.bin`
nests 20,000 unclosed elements and must fail with an XML error. Regenerate after changing the generator:

```sh
cargo run --example write_seeds
```

`cargo test` checks that `seeds/` matches the generator, that each seed reaches
its intended admission and import outcome, that prefixes of every seed satisfy
the invariants, that program decoding, including expanding substitutions, stays within its
budget, that the path
rules and known-limit exemptions behave as documented, that the sandbox detects
read-only, changing, `io_uring`, x32, network and spawned-thread calls,
refuses the changes, refuses process creation and execution without counting
them while keeping thread creation working, ignores the allocator probe and
charges a spawned thread's call only to its own monitor, and that each saved regression
still passes. These tests do not need sanitizer instrumentation.
