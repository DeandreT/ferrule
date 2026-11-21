# Toolchain baselines

Ferrule pins the toolchains used to build, lint and test the workspace, the
generated Rust and C# consumers, and the browser demo.

| Toolchain | Baseline | Pinned in |
| --- | --- | --- |
| Rust | `nightly-2026-11-16`, with rustfmt, clippy and `wasm32-unknown-unknown` | `rust-toolchain.toml`; the `dtolnay/rust-toolchain` steps in `ci.yml`, `codegen.yml` and `site.yml` |
| .NET SDK | 10.0.112, rolling forward to the latest 10.0.1xx patch; projects target `net10.0` | `global.json`; `actions/setup-dotnet` reads it in `codegen.yml` |
| trunk | 0.16.0, with the wasm-bindgen version locked in `Cargo.lock` | `site.yml` |
| cargo-nextest | 0.9.150 or newer | `.config/nextest.toml`; test workflows |
| mold | 2.40.4 in CI; Linux GNU linker | `.cargo/config.toml`; checksum-pinned `scripts/ci/setup-mold.py` |
| LLVM lld | Bundled with the pinned Rust toolchain | `.cargo/config.toml` for x86_64 Windows/MSVC |

`crates/cli/tests/toolchain_pins.rs` checks the Rust, .NET, trunk and nextest pins,
parallel-frontend defaults and target-specific linker settings.

## Supported platforms

CI builds and tests on x86_64 Linux, on the self-hosted OVH runner or the
GitHub-hosted `ubuntu-latest` fallback, and builds the browser demo for
`wasm32-unknown-unknown`. Windows development uses the same pins; it is not a
CI-qualified platform.

## Using the pins

rustup reads `rust-toolchain.toml` and installs the pinned toolchain on the
first `cargo` command in the checkout. Do not pass a `+toolchain` override.
Processes started by that `cargo`, including the generated Rust hosts that the
`code_generation` tests build in temporary directories, inherit the selection
through `RUSTUP_TOOLCHAIN`. The pin applies to this checkout; it does not change
rustup's global default or other projects. Package `rust-version` declarations
and the independent fuzzing toolchain are separate from this build pin.

`dotnet` reads `global.json` from the current directory or a parent, so the
runtime and smoke-test projects under `runtime/csharp` use the pinned band.
Generated C# hosts built outside the checkout use the SDK selected on the
machine; CI installs only the pinned band, and local runs should do the same.

CI installs mold into each job's temporary directory without sudo or a global
linker replacement. On x86_64 Linux locally, install mold and keep the
target-specific linker configuration in `.cargo/config.toml`.
Windows/MSVC selects `rust-lld.exe` with
`linker-flavor=lld-link`, using Rust's bundled LLVM lld without an external
LLVM installation. WebAssembly keeps its existing linker.
When running Cargo directly in a generated Rust project outside this checkout
on x86_64 Linux, use
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc` and
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"`
for the same linker. Cargo processes started from the checkout already pass
these linker defaults, and the Windows linker flavor, to inheriting children.

Install the pinned nextest release with `cargo binstall cargo-nextest --version
0.9.150 --secure`, or use the official prebuilt release. Run `cargo nextest run
--locked` for unit/integration tests and `cargo test --doc --locked` separately
for doctests. The serial nextest profile preserves the shared generated-host
build lane and does not retry failures automatically.

The site build has no `--locked` flag in trunk 0.16, so `site.yml` fails when
the build rewrites `Cargo.lock`. Every cargo step in `ci.yml` and `codegen.yml`
passes `--locked`.

## Parallel compiler frontend

The pinned nightly supports `-Zunstable-options --jobs-frontend=0`. Zero selects
the logical CPU parallelism reported by `std::thread::available_parallelism()`,
capped at 255 threads. This reflects process CPU constraints, including affinity
and CI quotas, not spare capacity under current system load. It does not promise
that every thread stays busy or that builds speed up in proportion to the thread
count.

`.cargo/config.toml` sets these flags in `[build]` for targets such as
`wasm32-unknown-unknown`. The explicit Linux GNU and Windows/MSVC target
`rustflags` include them too, since target flags take precedence over build
flags; their existing linker settings remain in place. The `[env]` defaults
pass the linker settings but not the frontend flags. Cargo appends inherited
target flags to the checkout's target flags, and `rustc` accepts
`--jobs-frontend` only once. Cargo started outside the checkout does not load
this frontend configuration. To opt in there, use a replacement flag list
as described below.

Cargo's normal flag precedence still applies: caller-supplied
`CARGO_ENCODED_RUSTFLAGS` or `RUSTFLAGS` replaces the configured flags rather than
appending to them. Include the frontend options and the appropriate linker flags
when supplying a replacement, for example
`RUSTFLAGS="-Zunstable-options --jobs-frontend=0 -C link-arg=-fuse-ld=mold"` on
x86_64 Linux. Do not export a second `--jobs-frontend` in `CARGO_TARGET_*_RUSTFLAGS`
for a Cargo process that reads this checkout's configuration.

Frontend threads work within each compiler process. Cargo's `-j` or
`CARGO_BUILD_JOBS` controls concurrent build jobs; nextest and test-thread limits
control test concurrency. Keep existing bounded build jobs, scheduler limits and
serial generated-consumer gates. Automatic frontend parallelism does not raise
those limits or set a new Cargo job default, and concurrent compilers can still
compete for CPU and memory.

## Updating a baseline

Bump a baseline in an ordinary pull request that changes every pinned file
together. For Rust, choose an exact `nightly-YYYY-MM-DD`, not the moving
`nightly` channel. Update `rust-toolchain.toml`, all five toolchain selectors in
`ci.yml`, `codegen.yml` and `site.yml`, and the documented date together. Confirm
that rustfmt, clippy and `wasm32-unknown-unknown` are available for that date and
that the frontend flags and inherited linker defaults remain supported.
Before merging, run on the new toolchain from a fresh target directory:

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo nextest run --workspace --locked` and `cargo test --workspace --doc --locked`;
- `cargo clippy -p cli --test code_generation --features codegen-tests --locked -- -D warnings` and the generated-host suites in [code-generation.md](code-generation.md);
- the C# smoke tests in `runtime/csharp/Ferrule.Runtime.SmokeTests`;
- `cargo build -p web-demo --target wasm32-unknown-unknown --release --locked` and a trunk build of `crates/web-demo`.

Fix new lints and incompatibilities in the same pull request. There is no
automated update bot or scheduled pipeline.

## What the pins establish

- **Deterministic generation**: the generators emit the same source for the
  same project and options, independent of the toolchain. The generated-artifact
  tests check that source.
- **Reproducible inputs**: the toolchain pins and `Cargo.lock` constrain build
  inputs. The parallel frontend can vary diagnostic order and, in some cases,
  binary output; a pin alone does not establish byte-for-byte reproducibility.
- **Runtime compatibility**: generated C# targets `net10.0` and generated Rust
  uses the pinned Rust nightly. Relevant qualification checks, not the pin alone,
  establish compatibility. Other compiler, SDK or runtime versions are not
  qualified.
