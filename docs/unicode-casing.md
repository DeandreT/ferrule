# Portable Unicode casing

The `upper` and `lower` builtins use one casing contract in the interpreter,
generated Rust and generated C#. Results depend only on the input string, never
on the Rust toolchain's Unicode version, .NET's Unicode tables, the host culture
or `InvariantGlobalization`.

## Contract

- **Unicode version:** 17.0.0. Mappings come from pinned tables generated from the
  Unicode Character Database, not from `str::to_uppercase`, `ToUpperInvariant` or
  similar host APIs.
- **Unit:** each Unicode scalar value is mapped independently. C# decodes UTF-16
  surrogate pairs, so supplementary scalars map as one scalar.
- **Full, language-neutral mappings:** a scalar's mapping is its unconditional
  `SpecialCasing.txt` entry when one exists; otherwise its simple `UnicodeData.txt`
  mapping; otherwise the scalar itself. Mappings can change length, for example
  `upper("ß")` is `SS`, `upper("ŉ")` is `ʼN`, `upper("ﬁ")` is `FI`,
  `upper("ΐ")` is three scalars, and `lower("İ")` is `i` followed by U+0307.
- **Final sigma:** `lower` maps U+03A3 to final ς (U+03C2) when the Unicode
  `Final_Sigma` condition holds in the original input, and to σ (U+03C3)
  otherwise. Scanning outward from the sigma skips `Case_Ignorable` scalars. The
  sigma is final when the first other scalar before it is `Cased` and the first
  other scalar after it is not. A scalar that is both `Cased` and
  `Case_Ignorable`, such as U+02B0, is skipped. Thus `lower("ΟΔΟΣ")` is `οδος`,
  `lower("A'Σ")` ends in `ς`, and `lower("AΣ'B")` keeps `σ`.
- **No language-specific rules:** the Lithuanian, Turkish and Azeri entries in
  `SpecialCasing.txt` are not applied. `upper("ı")` and `upper("i")` are both `I`,
  `lower("I")` is `i`, and `upper("İ")` is unchanged.
- **Titlecase digraphs** follow their simple mappings: `upper("ǅ")` is `Ǆ` and
  `lower("ǅ")` is `ǆ`. Titlecase is not a separate builtin.
- **No normalization:** input is neither normalized nor recomposed, so a mapping
  can produce a decomposed sequence. Scalars without a mapping, including scalars
  unassigned in Unicode 17.0.0, are unchanged.

`length` and other functions apply to the mapped text, so a length-changing
mapping changes their results. Case-insensitive regular-expression matching uses
the separate regex folding profile described in
[code generation](code-generation.md#portable-subset).

## Implementation

`crates/functions/src/casing/` owns the Rust implementation. Generated Rust
reaches it through `codegen-runtime`. The C# runtime implements the same lookup in
`runtime/csharp/Ferrule.Runtime/FerruleUnicodeCasing.cs`. Both languages read
generated leaves with the same rows:

- single-scalar mappings as `(first, last, stride, delta)` runs, where stride 2
  covers alternating upper/lower pairs;
- multi-scalar expansions as `(scalar, mapped...)` padded with zero;
- inclusive `Cased` and `Case_Ignorable` ranges for the final-sigma context.

`casing_tables.txt` is the canonical ASCII representation. `tables.rs` and
`FerruleUnicodeCasing.Tables.cs` contain the same records and the complete Unicode
license notice, which is also stored as `LICENSE-UNICODE.txt`. Unit tests check
that both leaves and the canonical file agree. A toolchain whose
`char::UNICODE_VERSION` equals the pin also checks every scalar and the
final-sigma context classes against Rust's `str` casing.

## Regenerating the tables

`scripts/generate_unicode_casing.py` uses the
[Unicode 17.0.0 UCD](https://www.unicode.org/Public/17.0.0/ucd/) and its
license. It uses no network access or extra Python packages. It requires the
following exact inputs:

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| `UnicodeData.txt` | 2,198,209 | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` |
| `SpecialCasing.txt` | 17,049 | `efc25faf19de21b92c1194c111c932e03d2a5eaf18194e33f1156e96de4c9588` |
| `DerivedCoreProperties.txt` | 1,134,783 | `24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08` |
| `ReadMe.txt` | 740 | `9fe1a90bd32659d7953616283dc2bffaa165518aae9ace026040c42c559ba606` |
| `LICENSE` ([license.txt](https://www.unicode.org/license.txt)) | 1,995 | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` |

Run these commands from the repository root. The first output directory must not
already exist:

```bash
UCD17='/absolute/path/to/pinned-unicode-17'
CASING_OUTPUT='/absolute/path/to/fresh-casing-output'
CASING_INPUTS=(
  --unicode-data "$UCD17/UnicodeData.txt"
  --special-casing "$UCD17/SpecialCasing.txt"
  --derived-core-properties "$UCD17/DerivedCoreProperties.txt"
  --readme "$UCD17/ReadMe.txt"
  --license "$UCD17/LICENSE"
)
python3 scripts/generate_unicode_casing.py \
  "${CASING_INPUTS[@]}" --output-root "$CASING_OUTPUT"
python3 scripts/generate_unicode_casing.py \
  "${CASING_INPUTS[@]}" --output-root "$CASING_OUTPUT" --check
```

The generator rejects these conditions:

- an unexpected language-neutral condition in `SpecialCasing.txt`;
- an expansion longer than three scalars;
- a `Cased` set that differs from `Lowercase`, `Uppercase` and general category
  `Lt` combined.

The generator decodes every code point through the emitted lookup structure. It
then compares the result with the full mapping and property data. Copy
`tables.rs`, `casing_tables.txt` and `LICENSE-UNICODE.txt` to
`crates/functions/src/casing/`. Copy `FerruleUnicodeCasing.Tables.cs` to
`runtime/csharp/Ferrule.Runtime/`. `generation.json` records input and output
hashes and is not committed.

A Unicode version change updates the version pins, the generated leaves, the
version constant in the Rust casing module and this contract. The change is
intentional and reviewed.
