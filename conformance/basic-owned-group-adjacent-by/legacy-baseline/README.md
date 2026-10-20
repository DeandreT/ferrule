# Ordinary adjacent artifact baseline

`manifest.json` contains the complete ordered inventory of two Rust and 78 C#
files emitted by the existing CLI built from commit
`a70e111282638db2f63bc6cf208c934bf1663a2f`. Each row fixes the relative artifact
path, byte length and SHA256. The harness pins the complete manifest's own hash.
No current emission provides an expected row or archival body.

## Provenance and comparison profile

The manifest binds the a70 source archive and source-blob receipt, actual
generator executable, exact `../legacy-project.json`, independently authored
input/expected-output files, ordinary APIs and generation options. Rust uses
package `ferrule-generated-mapping` and an explicit Path runtime dependency;
adapter opt-ins are absent. C# uses the ordinary emitter without options.

The retained `provenance/GENERATOR-BRIDGE.md` records the accepted independent
L01-only source-equivalence review to base
`7769fce8009bc17be091980cb7de12f11f7722c4`. The manifest binds that document,
base archive/tree and complete/emission-closure source diffs. This does not
relabel the a70 executable as a base-7769 build. The bridge covers only the
exact fixture, ordinary profile and matching options; it establishes no
execution equivalence for other Projects, Basic profiles or generated results.

Historical archive/executable hashes are provenance facts, not filesystem
dependencies. All inputs needed by the comparison are tracked here and in the
adjacent corpus; a fresh clone does not need the original custody directory,
archived executable or source tarballs and never rebuilds the old generator.

## Complete Cargo contract

`Cargo.toml.historical` is the original 292-byte emitted manifest, unchanged.
`Cargo.toml.template` fixes every byte except one `@RUNTIME_PATH_JSON@` operand.
Instantiating that operand from `historical_runtime_path` in the pinned manifest
must reproduce the complete archival bytes and frozen row before the current
runtime path is used. The historical path is only a literal operand; it is not
opened or required to exist. Expected current Cargo bytes instantiate the same
template. Actual emitted bytes are never rewritten or normalized.

The operand admits printable ASCII paths, where the historical TOML quoting
and JSON quoting agree. Other paths fail closed. The Rust library's historical
row remains exact, independent of the runtime path.

## Reviewed C# runtime boundary

All 78 historical artifact paths are retained, and the 74 stable files remain
subject to exact historical comparison. Only these four artifacts can use reviewed
candidate source bytes instead:

- `Runtime/FerruleBasicGroupBy.cs`
- `Runtime/FerruleFunctions.cs`, which dispatches `upper`/`lower` to the pinned
  portable Unicode casing
- `Runtime/FerruleInstance.cs`
- `Runtime/ScopeContext.cs`

`historical-runtime/` retains their byte-exact a70 emitted bodies so preparation
can produce complete old/new source diffs without retaining all historical
packages. Each archival body must match its pinned historical row. It is test
data, not an active runtime source.

The current package also includes exactly four source-bound additions:
`Runtime/FerruleUnicodeCasing.cs`, `Runtime/FerruleUnicodeCasing.Tables.cs`,
`Runtime/Json/FerruleJsonSourceDocument.cs` and
`Runtime/Json/FerruleJsonSourceOccurrence.cs`. They remain absent from the frozen
historical inventory. Preparation records their complete source and addition diffs
with a null historical row; it does not manufacture historical files for them.

Before any emission, preparation snapshots the corresponding candidate repository
sources, retains old/new sizes and hashes plus full source diffs, and binds the
candidate snapshots. The parent-owned `legacy_artifact_policy` requires exact
historical paths plus those four additions, stable rows and exact candidate-source rows; those sources
must remain unchanged. Historical equality is reported separately from the
reviewed replacement/addition policy. No `Runtime/**` exemption exists, and an additional
changed file requires a separately reviewed policy change.

The source fence includes this entire baseline package and the comparison helper.
Baseline/source packaging checks do not replace independent mapping-result
oracles, runtime component checks, old-profile refusals or ordinary execution.
