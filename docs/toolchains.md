# Toolchain baselines

Ferrule pins the toolchains used to build, lint and test the workspace, the
generated Rust and C# consumers, and the browser demo.

| Toolchain | Baseline | Pinned in |
| --- | --- | --- |
| Rust | 1.98.1 stable, with rustfmt, clippy and `wasm32-unknown-unknown` | `rust-toolchain.toml`; the `dtolnay/rust-toolchain` steps in `ci.yml`, `codegen.yml` and `site.yml` |
| .NET SDK | 10.0.112, rolling forward to the latest 10.0.1xx patch; projects target `net10.0` | `global.json`; `actions/setup-dotnet` reads it in `codegen.yml` |
| trunk | 0.16.0, with the wasm-bindgen version locked in `Cargo.lock` | `site.yml` |

`crates/cli/tests/toolchain_pins.rs` fails when these files disagree.

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
through `RUSTUP_TOOLCHAIN`.

`dotnet` reads `global.json` from the current directory or a parent, so the
runtime and smoke-test projects under `runtime/csharp` use the pinned band.
Generated C# hosts built outside the checkout use the SDK selected on the
machine; CI installs only the pinned band, and local runs should do the same.

The site build has no `--locked` flag in trunk 0.16, so `site.yml` fails when
the build rewrites `Cargo.lock`. Every cargo step in `ci.yml` and `codegen.yml`
passes `--locked`.

## Updating a baseline

Bump a baseline in an ordinary pull request that changes every pinned file
together. Before merging, run on the new toolchain from a fresh target
directory:

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `cargo clippy -p cli --test code_generation --features codegen-tests --locked -- -D warnings` and the generated-host suites in [code-generation.md](code-generation.md);
- the C# smoke tests in `runtime/csharp/Ferrule.Runtime.SmokeTests`;
- `cargo build -p web-demo --target wasm32-unknown-unknown --release --locked` and a trunk build of `crates/web-demo`.

Fix new lints and incompatibilities in the same pull request. There is no
automated update bot or scheduled pipeline.

## What the pins establish

- **Deterministic generation**: the generators emit the same source for the
  same project and options, independent of the toolchain. The generated-artifact
  tests check that source.
- **Reproducible builds**: compiling that source, and the workspace itself,
  with the pinned toolchains and `Cargo.lock` gives the same results on each
  run. The pins establish this; deterministic source alone does not.
- **Runtime compatibility**: generated C# targets `net10.0` and generated Rust
  builds on the pinned Rust release. Other compiler, SDK or runtime versions
  are not qualified.
