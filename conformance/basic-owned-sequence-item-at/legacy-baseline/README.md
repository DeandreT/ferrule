# Ordinary ItemAt artifact baseline

`manifest.json` freezes the complete ordinary artifact paths, byte lengths and
SHA256 values produced by the existing CLI from commit
`a70e111282638db2f63bc6cf208c934bf1663a2f`. Its provenance binds the source archive,
generator executable and exact `../legacy-project.json` fixture. Generation
uses ordinary `codegen::lower`, `codegen_rust::emit` and `codegen_csharp::emit`,
without adapter opt-ins. It does not execute a mapping or supply mapping-result
expectations. The legacy input and expected output remain independently authored.

Rust uses package `ferrule-generated-mapping`, matching that existing CLI's fixed
option. The consumer keeps its `legacy0` dependency alias. All ordinary Rust and
C# artifact paths and contents participate in the full manifest comparison,
including the C# runtime files. New Basic emission has separate success/type
controls and is not compared byte-for-byte to a different ordinary public API.

`Cargo.toml.historical` retains the unmodified generated manifest at the runtime
path recorded in the provenance. `Cargo.toml.template` independently fixes every
manifest byte except its single `@RUNTIME_PATH_JSON@` operand. Instantiating that
operand with the JSON-quoted historical runtime path must reproduce the complete
original manifest and its frozen length/hash. The current explicit runtime option
then instantiates the same template and must match the complete current manifest.
No actual file is rewritten or normalized. With equal paths, the frozen historical
Cargo length/hash also matches directly.

The JSON-operand template deliberately admits printable ASCII runtime paths only:
that is the domain where the historical emitter's TOML quoting agrees with JSON
quoting. Other paths fail closed rather than silently changing the byte contract.
The other artifact rows never vary with the runtime path. The older Basic By
inventory still refuses the ordinary ItemAt expression; that typed refusal is a
separate static control, not a replacement historical artifact baseline.
