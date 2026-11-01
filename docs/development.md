# Development and Verification

Use [Architecture](architecture.md) to find the responsible layer and
[ROADMAP](../ROADMAP.md) to choose a bounded next change. Detailed format and
compatibility contracts live in [Supported formats](formats.md),
[`.mfd` interoperability](mfd-interop.md), and [Code generation](code-generation.md).

## Coordinate issue work

[Contributing](../CONTRIBUTING.md) covers finding, claiming and delivering an
issue. [Parallel work](parallel-work.md) covers shared-file handoffs and shared
build/display resources. GitHub issues are the only live claim and status
source: get assigned before editing, open a scoped issue for a new gap before
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

Native and generated-backend CI can use the dedicated OVHcloud build runner.
See [OVHcloud CI operations](ovh-ci.md) for provisioning, isolation, resource
limits, and the GitHub-hosted fallback switch.

The checkout pins Rust, the .NET SDK band and trunk; see
[Toolchain baselines](toolchains.md). rustup installs the pinned Rust
toolchain, with rustfmt, clippy and the wasm32 target, on the first `cargo`
command in the checkout, so do not pass a `+toolchain` override.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Start with the affected package or integration target. For example:

```sh
cargo nextest run --locked -p format-csv
cargo nextest run --locked -p cli --test csv_row_release
```

Use cargo-nextest 0.9.150 or newer for ordinary tests. The repository profile
runs one test process at a time so generated consumers do not start competing
builds. Doctests still use `cargo test --doc`; nextest does not replace them.
The x86_64 Linux GNU target uses `cc` with mold. Install mold before building.
Windows/MSVC uses Rust's bundled LLVM lld; WebAssembly keeps its existing linker. See
[Toolchain baselines](toolchains.md) for setup and verification.

For a local alternate-display run, use the CI workspace test subset with both
ambient Wayland handles removed:

```sh
xvfb-run -a env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET LIBGL_ALWAYS_SOFTWARE=1 \
  cargo nextest run --workspace --locked \
  --exclude codegen --exclude codegen-runtime \
  --exclude codegen-rust --exclude codegen-csharp

cargo test --workspace --doc --locked \
  --exclude codegen --exclude codegen-runtime \
  --exclude codegen-rust --exclude codegen-csharp
```

Removing `WAYLAND_DISPLAY` and `WAYLAND_SOCKET` keeps the run off an ambient
Wayland session, as the [CI workflow](../.github/workflows/ci.yml) does.

Pixel-rendering tests also need a graphics adapter; Xvfb alone does not provide
one. On Ubuntu, install `libvulkan1`, `mesa-vulkan-drivers` and `vulkan-tools`
alongside `xvfb`. `xvfb-run -a vulkaninfo --summary` should list a CPU device
(Mesa's software Vulkan renderer). Hosted CI checks this prerequisite before
building; a missing adapter is a setup failure, not a reason to skip rendering.

Those exclusions are not generated-backend qualification. Changes to lowering,
runtime primitives, emitters, or public adapters need their focused Rust and
C# checks and actual compiled-host calls as applicable. See
[Code generation verification](code-generation.md) for prerequisites and
opt-in suites; source generation alone does not test an emitted library.
The CLI generated integration target is explicitly enabled with
`cargo nextest run -p cli --features codegen-tests --test code_generation --locked`;
read that suite's backend prerequisites before running it.

Format capability tables and workspace component lists in the guides and site
are generated from [shared technical references](reference/README.md). Edit
their TOML source and run `cargo run -p reference-docs`;
`cargo test -p reference-docs` fails while a generated region is stale.

### Before requesting review

Run the checks that match your change locally, on the head you will push. They
mirror the [CI](../.github/workflows/ci.yml) and
[codegen CI](../.github/workflows/codegen.yml) workflows; a passing subset of
focused tests does not replace them.

- [ ] Every Rust change: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`, and the
  alternate-display `cargo nextest run --workspace` subset and
  `cargo test --doc --workspace` commands above.
- [ ] `crates/web-demo` or a crate it depends on
  (`cargo tree -p web-demo --target wasm32-unknown-unknown` lists them):
  `cargo build -p web-demo --target wasm32-unknown-unknown --release --locked`.
- [ ] `codegen`, `codegen-runtime`, `codegen-rust`, `codegen-csharp`,
  `runtime/`, generated contracts, or a crate they depend on
  (`cargo tree -p codegen` and `cargo tree -p cli --features codegen-tests`
  list them):
  `cargo clippy -p cli --test code_generation --features codegen-tests --locked -- -D warnings`,
  `cargo nextest run --locked -p codegen -p codegen-runtime -p codegen-rust -p codegen-csharp`,
  their `cargo test --doc` run, the C# runtime smoke tests and the CLI generated
  integration target, as the codegen workflow runs them. Its mappings job also
  runs the compiled boundary regressions and cohort consumers, which the default
  run skips, with `--run-ignored only`; use the commands from those workflow
  steps. The pinned .NET SDK is
  in [Toolchain baselines](toolchains.md).
- [ ] `scripts/ci/`: `bash -n scripts/ci/*.sh` and the `scripts/ci/test-*.py`
  suites the workflows run.
- [ ] Generated technical references: `cargo test -p reference-docs`.

### Run the PDF worker from a build

`ferrule run --pdf-worker` and the GUI's PDF worker option admit only a
single-link `ferrule-pdf-worker` beside the running executable, so they refuse
Cargo's hard-linked build outputs. Stage a private package and run from it:

```sh
scripts/dev/stage-pdf-worker-package.sh            # dev profile; add --release
target/debug/pdf-worker-package/ferrule-gui
target/debug/pdf-worker-package/ferrule run --project project.json --pdf-worker
```

The script builds `ferrule`, `ferrule-pdf-worker` and `ferrule-gui` with
`cargo build --locked`, honouring `CARGO_TARGET_DIR`, and copies them into
`<build directory>/pdf-worker-package` (or `--dest DIR`) with mode `0700`.
`--cli-only` skips the GUI, and `--build-dir DIR` stages existing binaries
without building. The destination and its ancestors must be owned by you or root
and not writable by group or others. Run the script again after each rebuild;
the staged copies do not follow the build directory. See the
[PDF worker contract](pdf-worker-contract.md#eligible-host-and-compatibility)
for the admission rules.

### Reuse generated CLI host builds

The `extra_targets`, `failure_rules` and `static_sources` regressions honor
`FERRULE_CODEGEN_HOST_TARGET_DIR` as a dedicated host cache. Use a compatible
cache outside the outer Cargo target and temporary test directories, one build
job, and nextest's serial repository profile; coordinate other processes using
the same cache. Do not override `--test-threads` for these shared-cache runs.
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
