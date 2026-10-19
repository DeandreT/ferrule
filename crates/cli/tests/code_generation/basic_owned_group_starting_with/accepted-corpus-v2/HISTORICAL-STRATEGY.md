# Parent-owned exact merged-base ordinary artifact strategy

No generation or comparison was run here. This strategy intentionally contains no fabricated emitted-file hash and no candidate-derived historical row.

## Frozen literal input

The baseline Project hash is `df23d0c0fbc2d945ea5bf9d111acbe98ac6fb7acd02c9a078fca18c5d969916b`; input `7bed999b17c4ccafbbb86dde5dc92d9b2508b53a3c3822523fdaf27740e89036`; expected JSON `4e154407efb8c87961eb66f538d2bbc59cdb03718579569af97d4b0afd624140`. Canonical hand-authored Project serialization is pretty+LF. Lossless input/expected trees are separate. Future model re-encoding is a comparison with this oracle, never permission to rewrite it automatically.

Parent supplied the completed existing-generator build from exact `f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971` archive, not a new helper:

- `/home/deandre/.cache/ferrule-qualification/starting443-historical-generator-v1/ferrule-historical`
- reported SHA256 `bc68d9621b93455355a1b131c64178c3b6b2128b4aaa868433cef719401922bc`
- build source/private target in that custody root; original archive in `starting443-merged-base-source-v1/`.

The completed build/source-guard/root-receipt seals are parent evidence. Merely finding those files here is not independent attestation. Preserve them and bind the executable's actual build-time source, tool receipts and complete archive identity to future artifact provenance. Do not relabel an old a70/7769 executable or infer correctness solely from today's source files.

## First historical generation (parent only, before production changes)

After the independent baseline source review and an explicit generation grant, use this existing CLI contract, twice into distinct **absent** roots:

```text
<retained-ferrule-historical> generate --project <frozen-legacy-project.json> --language rust --out <absent-rust-root> --rust-runtime-path <retained-base-runtime-path>
<retained-ferrule-historical> generate --project <frozen-legacy-project.json> --language csharp --out <absent-csharp-root>
```

The existing CLI uses ordinary `codegen::lower`, `codegen_rust::emit` / `codegen_csharp::emit`; it does not use the new companion. Retain argv/cwd/environment, executable/tool/source hashes, exact fixture bytes/options, start/finish/exit/stdout/stderr, complete artifact files and a sorted path/length/SHA256 manifest before any candidate generation. Confirm the actual ordinary pathset (expected2 Rust/78 C# from inspected contracts); a mismatch is review-required, never an invented or silently normalized baseline.

Freeze the complete Rust Cargo body and a complete template with exactly one explicit runtime-path JSON operand; no other line is parameterized. Preserve actual historical Cargo bytes separately. Candidate expected Cargo may instantiate only that frozen operand; never normalize actual output or strip runtime sections. `src/lib.rs` stays byte-exact.

For C#, preserve the complete historical 78-file package. All three generated files and all unaffected embedded runtime files remain exact. Before allowing a #443 runtime delta, independently capture source-base and candidate bytes, size/SHA256 and complete-file diff **before emission**, obtain exact-path delta review, then bind each emitted file to that reviewed source and verify sources unchanged after emission. A new path, removed path, fourth changed runtime path or any unreviewed body is a blocker. No Runtime/** exclusion or automatic use of #442's three-file exception is approved.

## Later conservation and execution

Compare ordinary historical profile to ordinary candidate profile with identical options and explicit runtime operand. New-companion ordinary emission success is an additional separate control, not a substitute. Preserve existing #441/#473/#442 fixtures, manifests, archival bodies and whole-package receipts. Their previous helper enum/bridge is not #443 approval.

The parent owns portable comparator integration and real historical manifests. Only after freezing/reviewing originals should separately authorized native and compiled public Rust/C# consumers execute the ordinary input, expecting literal runs `[one,two] [three] [four,five]` and the complete independent trees for All/Primary. Retain original raw products and every mismatch. Any baseline serialization mismatch is investigated against inspected serde rules; no actual output becomes a new expectation.
