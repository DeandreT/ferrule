# Ordinary Starting artifact baseline

`manifest.json` fixes the complete ordered path, byte length and SHA256 of the
ordinary Rust and C# packages emitted from merged source commit
`f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971`. Its provenance binds the source archive,
source tree, generator executable, literal Project/input/expected products, and
ordinary lowering/emission APIs. The generator uses the same merged source as
this baseline; no source-equivalence bridge is needed.

The Rust package name is `ferrule-generated-mapping`, with an explicit Path
runtime dependency and no adapter opt-ins. Ordinary C# emission has no options.
Historical artifacts must be compared with ordinary emission using the same
Project and options. New-profile output is a separate comparison.

## Cargo operand and library

`Cargo.toml.historical` and `lib.rs.historical` retain complete original bytes.
`Cargo.toml.template` fixes every byte except one `@RUNTIME_PATH_JSON@` operand.
Instantiating it with `historical_runtime_path` must reproduce the entire
historical Cargo body and its manifest row before a candidate runtime path may
be used. Only printable ASCII paths are admitted, for which the emitter's TOML
quoting and JSON quoting agree. Emitted actual bytes are never normalized.
The library always remains byte-exact against its historical row and body.

The historical path is an inert provenance operand, not a build dependency or a
path that the comparison opens. The complete local comparison inputs are this
package and the accompanying literal corpus files; the archived generator and
original source checkout are not required in a fresh clone.

## C# conservation

All 78 paths are fixed: three generated files and 75 runtime files. Exact
historical equality is the default. A Starting-specific reviewed policy may
admit alternative source-bound bytes only for:

- `Runtime/FerruleBasicGroupBy.cs`
- `Runtime/FerruleInstance.cs`
- `Runtime/ScopeContext.cs`

`historical-runtime/` retains their exact merged-base emitted bodies. Each must
match its historical manifest row and original runtime source. These files are
comparison data, not active runtime sources.

The three generated files and 72 other runtime files stay historical-byte exact.
Any permitted runtime change requires a complete pre-emission source snapshot,
full old/new source diff, independent Starting-specific review and exact binding
of emitted bytes to that snapshot, with unchanged-source verification afterward.
The earlier Adjacent policy grants no Starting exception. Added or removed
artifacts, another modified runtime body, or altered retained layouts require
separate review.

Artifact conservation establishes source and packaging identity. Literal output
trees, Project roundtrip, ordinary execution, old-profile behavior and component
resource checks retain their independent assertions.
