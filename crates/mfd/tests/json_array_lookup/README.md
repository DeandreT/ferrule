# Retained lookup Release projects

The ignored `export_retained_lookup_release_projects` test imports and emits the
five existing generic profiles without running mappings, compilers or generated
hosts. Its names are `direct`, `conditional`, `root-scalars`, `edi` and
`conditional-edi` (the last retains the `conditional/edi` fixture profile).

Set `FERRULE_LOOKUP_EXPORT_ROOT` to a new absolute native Windows drive path whose
parent already exists. Run the export through the fixed-slot Cargo wrapper and
the separately admitted resource guard. The exporter rejects an existing root,
ambiguous/device paths, repository or worktree ancestry, Git-state overlap,
Cargo target/build/slot-pool overlap, reparse points and offline/recall parents.
It holds ordinary parent directories, creates files exclusively, verifies their
bytes and complete membership, and never deletes or replaces an export. A failed
export remains for inspection. Other operating systems are explicitly refused
because this retained writer uses Windows locked-directory support.

The root contains the exact selected source fixtures under `fixtures/`, cleared
package sources in `NuGet.Config`, and `export-manifest.json`. The manifest lists
source fixture hashes, compiled exporter/test hashes, each output's size and
SHA-256, complete output membership, and profile/project names. The manifest
itself is included in membership but excluded from its own hash list.

Each `profiles/<name>/` contains:

- Unmodified complete emitter artifacts in `generated/rust/` and
  `generated/csharp/`.
- `source-schema.json`, `target-schema.json` and byte-identical `cases.json`.
- `rust-host/Cargo.toml` and the original literal `host.rs` oracle. The binary
  name is `lookup-host`; its generated library package is `json-lookup-mapping`.
- `csharp-host/Host.csproj`, byte-identical `Program.cs` and `LookupOracle.cs`.
  The library is `generated/csharp/Ferrule.Generated.csproj`; the host targets
  `net10.0` and its assembly is `Host.dll`.

These are the original self-authored literal oracles. The exporter never derives
expected output from the interpreter and does not certify execution agreement.

## Separately admitted build and execution

Keep the retained export unchanged. Copy only each host directory to a new owned
build workspace before lockfile generation or restore. The host projects use
absolute library references, so the copied hosts still consume the retained
generated libraries. Cargo targets remain in fixed slots; direct nested Cargo
execution from a test is not this workflow.

Generate the copied Rust host's lockfile offline, then build with `--release
--locked --offline --message-format=json-render-diagnostics`. Select the exact
`lookup-host` executable from Cargo JSON, requiring the optimized non-test Release
profile; retain and hash it before another profile can overwrite a slot artifact.
Do not infer the executable from a target-directory name.

Build C# with the selected SDK, `--configuration Release`, disabled compiler
servers and an external `--artifacts-path`. Keep both final outputs and
intermediates outside the retained export. Set `DOTNET_ADD_GLOBAL_TOOLS_TO_PATH=false`
for every isolated CLI home and child. Query the selected project's `TargetPath`
with the same configuration/artifact properties before invoking the assembly.

Both hosts take exactly three positional arguments: retained `cases.json`,
`source-schema.json` and `target-schema.json`. Admit each build and each invocation
as its own guarded stage, retain complete stdout/stderr and source/artifact
identity, and keep those results separate from external MapForce reference work.
