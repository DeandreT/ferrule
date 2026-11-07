# Parsed lookup composition checks

`root_scalars.rs` adds the bounded independent root-scalar profile to the existing
`json_array_lookup` integration target. The root and filtered-key fixtures were
self-authored for this GPL-3.0-only repository; their schemas, mappings, literal
catalogs and host oracles contain generic public data, not vendor source or
captured business payloads. Filtered-key cases retain observations to obtain,
not invented expected results.

Run the ordinary `json_array_lookup` and `json_array_parser` targets first. They
check interpreter values/errors, exact graph ownership, refusal controls and
source emission. Emission alone does not qualify a generated consumer.

The existing cross-platform ignored host path runs the same literal catalog
against optimized Rust and C# libraries:

```sh
cargo nextest run --locked --offline -p mfd --test json_array_lookup \
  --build-jobs 1 --target-dir /absolute/private/outer-target \
  --config profile.dev.debug=0 --config profile.dev.incremental=false \
  --config 'profile.dev.strip="symbols"' \
  --run-ignored only \
  -E 'test(=root_scalars::generated_root_scalar_lookup_release_hosts_match_complete_outputs_and_typed_failures)'
```

Coordinate resources before this opt-in check. Set `TMPDIR` to a short owned
disk-backed cache directory on Linux, and use the repository's pinned Rust and
.NET toolchains. The Rust child runs `cargo run --release --offline -j 1` with
incremental compilation disabled, symbols stripped and warnings denied. The C#
child runs `dotnet run --configuration Release --disable-build-servers` with
shared compilation and parallel builds disabled. Each isolated .NET CLI home
explicitly sets `DOTNET_ADD_GLOBAL_TOOLS_TO_PATH=false`.

The host templates are `fixtures/json-array-lookup/root-scalars/rust_host.rs`
and `csharp_host.cs`, with the shared typed C# assertion helper. Every host takes
exactly three positional arguments: `cases.json`, `source-schema.json` and
`target-schema.json`. They compare complete typed output against literal expected
JSON parsed with the target schema; schema failures require `InvalidArgument`,
the exact detail and a function from the case's bounded allowlist. Empty-primary
errors require `json_parse_field`; a missing `LookupText` requires
`MissingRuntimeParameter`.

This disposable cross-platform path is separate from the retained branch's
native-Windows locked-directory exporter. That writer is not a portable workflow
and must not be launched or adapted on Linux. Neither host path records external
reference behavior or admits filtered keys.
