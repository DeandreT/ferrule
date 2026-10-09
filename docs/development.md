# Development and Verification

Use [Architecture](architecture.md) to find the responsible layer and
[ROADMAP](../ROADMAP.md) to choose a bounded next change. Detailed format and
compatibility contracts live in [Supported formats](formats.md),
[`.mfd` interoperability](mfd-interop.md), and [Code generation](code-generation.md).

## Coordinate issue work

Use [Parallel work](parallel-work.md) to choose an available scoped issue and
check shared source/build/display ownership. Assign the issue to yourself before
starting; maintainer work uses `DeandreT`. If assignment is unavailable to you,
ask the maintainer to assign it first. Open a scoped issue for a new gap before
implementation, agree on overlapping files, and link the implementing PR.

Keep issue comments concise: result, evidence link, blocker, and next step.
Report source review, local tests, generated calls, desktop interaction, external
execution and memory trials separately. Close only the acceptance gates that
have evidence; leave the remaining checks explicit.

## Keep a change reviewable

- [ ] Describe one concrete input or user action and the expected result.
- [ ] Identify the owning schema, graph, scope, adapter, or interface layer.
- [ ] Keep format-specific behavior at its boundary; preserve typed engine
  errors and existing public defaults.
- [ ] Add a self-authored fixture with an independent full-value or full-byte
  oracle. Include the error or cancellation path that motivates the change.
- [ ] Preserve existing project identities, paths, ordering, and optional-input
  behavior unless the change explicitly revises that contract.
- [ ] Update the focused contract document and roadmap gate, then run the
  relevant checks. A new test that only repeats the implementation is weak evidence.

For GUI changes, exercise the real controls in addition to state methods:

- [ ] Create or edit, undo/redo, save, and reopen.
- [ ] Verify pins, wires, hover identities, readable compact summaries, and
  positions in every affected canvas.
- [ ] Check disabled controls, dirty prompts, cancellation, stale drafts, and
  checked ID allocation before mutation.
- [ ] Use a realistic viewport and actual wheel/keyboard/pointer events.
- [ ] Verify Preview separately from Run and output publication.

## Run the appropriate checks

Install nightly Rust and its formatting/lint components. The repository CI
uses nightly; dependencies and generated API contracts should be checked
against the actual toolchain used for the run.

```sh
rustup toolchain install nightly --component rustfmt --component clippy
cargo +nightly fmt --all --check
cargo +nightly clippy --workspace --all-targets -- -D warnings
```

Start with the affected package or integration target. For example:

```sh
cargo +nightly test -p format-csv
cargo +nightly test -p cli --test csv_row_release
```

For a local alternate-display run, use the CI workspace test subset with both
ambient Wayland handles removed:

```sh
xvfb-run -a env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET LIBGL_ALWAYS_SOFTWARE=1 \
  cargo +nightly test --workspace \
  --exclude codegen --exclude codegen-runtime \
  --exclude codegen-rust --exclude codegen-csharp
```

Removing `WAYLAND_DISPLAY` and `WAYLAND_SOCKET` avoids reusing these ambient
Wayland handles during a local alternate-display run. The
[current CI workflow](../.github/workflows/ci.yml) clears `WAYLAND_DISPLAY`
only; this local example adds the socket guard.

Those exclusions are not generated-backend qualification. Changes to lowering,
runtime primitives, emitters, or public adapters need their focused Rust and
C# checks and actual compiled-host calls as applicable. See
[Code generation verification](code-generation.md) for prerequisites and
opt-in suites; source generation alone does not test an emitted library.
The CLI generated integration target is explicitly enabled with
`cargo +nightly test -p cli --features codegen-tests --test code_generation`;
read that suite's backend prerequisites before running it.

### Reuse generated CLI host builds

The `extra_targets`, `failure_rules` and `static_sources` regressions honor
`FERRULE_CODEGEN_HOST_TARGET_DIR` as a dedicated host cache. Use a compatible
cache outside the outer Cargo target and temporary test directories, one build
job, and `--test-threads=1`; coordinate other processes using the same cache.
An empty explicit path, overlap with `CARGO_TARGET_DIR`, or active-test-executable
ancestor is refused before temporary test files or a host build are created.
Without the setting, these regressions keep their isolated `cargo-target` path.

Set `FERRULE_CODEGEN_KEEP_ARTIFACTS=1` to retain generated CLI test directories
after success or failure. These three regressions record complete source and
command originals, followed by status/output or startup-error originals before
assertions; source copies exclude build caches. Other values keep ordinary
temporary cleanup. Retention can use substantial space, so bind a suitable
temporary directory and inspect space before the run.

### Build space and desktop isolation

- [ ] Check free disk space and relevant Cargo target sizes before a large
  workspace build or generated-host cohort.
- [ ] Reuse a compatible target directory; avoid duplicate full caches and
  simultaneous heavy builds. Coordinate one build lane when sharing artifacts.
- [ ] Use bounded jobs. CI currently sets `CARGO_BUILD_JOBS=2`,
  `CARGO_INCREMENTAL=0`, dev/test debug information to zero, and
  `RUST_TEST_THREADS=4`.
- [ ] Check space again after substantial builds. Preserve required evidence,
  active artifacts, and user files when reclaiming owned generated output.
- [ ] Run graphical tests on an alternate display. A separate `DISPLAY` alone
  does not isolate a desktop portal: interactive file-dialog checks also need
  an isolated session bus and a verified local chooser route.
- [ ] Bind the executable to the tested source, especially after changing
  runtime files embedded by an emitter. Verify the emitted file body; a cached
  build reporting “fresh” is not independent proof of embedded-source freshness.

## Record the kind of evidence

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Source review | Reached contracts and a plausible implementation | Compilation or execution |
| Import/validation | A representable, admitted project | Correct values or external execution |
| Local interpreter run | That fixture's values, errors, and artifacts | Generated or external behavior |
| Export/reimport | A checked round trip under its selected profile | Acceptance by another application |
| Compiled public host | That API, backend, input, and outcome | Every backend or transport |
| Desktop interaction | The observed controls and saved workflow | Unvisited UI paths or platforms |
| Memory trial | The recorded process/profile/input and measured phase | A universal RAM or performance bound |

For interop changes, retain raw outcomes before semantic assertions: complete
status and errors, produced artifacts including partial failures, and the
original input. Compare against independent typed values, literal bytes, or
schema-valid trees. Preserve a failed run when a later correction succeeds.
Keep source review, local testing, and external qualification distinct.

The [conformance inventory](../conformance/README.md) records capability and
backend cells separately. Its overall inventory is incomplete. Do not turn
unknown cells into supported ones or infer a parity percentage from a focused
cohort. An evidence update must follow the ledger's documented contract;
ordinary feature or documentation work must not rewrite the protected survey.
