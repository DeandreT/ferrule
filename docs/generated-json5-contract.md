# Generated JSON5 companion contract

JSON5 companions accept one complete JSON5 object and return complete strict JSON. They are an explicit, additive generation profile: ordinary JSON APIs, descriptor codecs and default generated artifacts retain their existing behavior.

## Explicit generation and public APIs

Select `codegen_rust::emit_with_json5(program, options)` or `codegen_csharp::emit_with_json5(program)` to add the companions. Ordinary `emit` does not add them. C# includes the optional package-free runtime sources; Rust uses the existing local `codegen-runtime` dependency. Neither backend infers selection from a file suffix or stored `json5` settings.

Use `generate --json5-adapters` or `cli::generate_project_with_json5_adapters`.
It conflicts with `--csv-output`. Ineligible projects refuse before publication;
the [writer](code-generation.md#output-filesystem) requires a new destination.

| Route | Rust generated method | C# generated method | Input and owned result |
| --- | --- | --- | --- |
| Text | `execute_json5(source)` | `ExecuteJson5(source)` | UTF-8 `&str` / UTF-16 `string` → strict JSON `String` / `string` |
| Text with context | `execute_json5_with_context(source, execution)` | `ExecuteJson5(source, executionContext)` | Same text boundary with the caller's execution context |
| Bytes | `execute_json5_bytes(source)` | `ExecuteJson5Bytes(source)` | UTF-8 `&[u8]` / `byte[]` → owned UTF-8 `Vec<u8>` / `byte[]` |
| Bytes with context | `execute_json5_bytes_with_context(source, execution)` | `ExecuteJson5Bytes(source, executionContext)` | Same byte boundary with the caller's execution context |

Rust uses `ExecutionContext<'_>`; C# uses `FerruleExecutionContext`. Context methods call the same ordinary mapping with that context. Successful output follows the target schema's property order, uses two-space indentation and ends with one LF, without a BOM. Comments are syntax trivia and are not returned as data. These are singular document APIs; named, plural and loader companions are outside this profile.

## Shared admission APIs

`codegen_schema::json5_profile` provides `validate_schema`, `encode_schema` and `decode_schema`. The first performs a borrowed schema census and validates the closed-object profile. Encoding additionally proves the actual complete descriptor round trip through the unchanged codec. Decoding retains the complete strict parse and its byte/depth errors, then checks decoded key identities in the original payload before accepting its collapsed value. Duplicate descriptor fields, including escaped aliases, are refused. Raw keys and defaults are checked before the ordinary decoder can discard unknown metadata.

`codegen` provides `validate_json5_format_options`, `prepare_json5_boundary` and `validate_json5_boundary`. Preparation returns `Json5BoundaryProfile` with the complete source and target descriptors. Validation delegates to preparation and discards that profile. Refusals use `Json5ProfileError` or `Json5BoundaryPolicyError`; schema-side ownership and original descriptor/program-validation causes remain available through `Error::source`.

Project format options are not retained by the lowered program. The opt-in CLI writer checks source then target options and performs both borrowed schema censuses before ordinary lowering. It then selects the opt-in emitter, which prepares the lowered program before ordinary emission. A direct caller with an existing Program uses the same preparation through `emit_with_json5`; preparation alone does not add methods. Stored `json_document` and `json5` Boolean identities may vary without selecting this profile.

## Eligible schemas and programs

Both source and target must have a closed, nonrepeating Group root. Nested Groups are allowed; each scalar leaf has exactly one String, Int, Float or Bool type. A scalar leaf may be nullable. Required names must be nonempty, unique and declared among that Group's unique children. Required-name presence is enforced by the later typed reader; policy admission does not execute an input.

All other schema metadata must have its ordinary constructor default. The policy refuses open objects, scalar unions, alternatives, repeated or recursive nodes, nullable Groups, XML metadata, constraints, fixed/default/generated values and database metadata. Raw descriptors with unknown node/kind keys or nondefault unsupported metadata are refused. Existing ordinary descriptor callers retain their current handling of ignored fields.

The program must have one primary input and one primary target, with no XML boundary, extra named/dynamic inputs or extra targets. Every target scope must use static Group construction, without repetition or iteration. Its bindings and nested scopes must refer to distinct declared scalar or Group children, with exact scalar target domains. Existing expressions, user functions, ordered global failure rules, lazy branches, graph exceptions and context values remain subject to complete ordinary program validation.

Format options admit their ordinary defaults plus the two JSON identity Booleans. Physical formats, repair dependencies, tabular/workbook controls, XML policies, local file sets, JSON Lines, unresolved-schema references and other nondefault options are refused. Every current SchemaNode, FormatOptions, Program, TargetScope, Binding and construction variant has an explicit decision; adding a typed field or variant requires a new policy decision.

## Policy limits

These limits apply independently to each schema:

| Resource | Maximum | Unit |
| --- | ---: | --- |
| Schema nodes | 4,096 | Root and all declared descendants |
| Logical levels | 64 | Root is level 1 |
| Individual name | 4,096 | UTF-8 bytes |
| Combined names | 1,048,576 | Node names plus every required-name occurrence, in UTF-8 bytes |
| Actual descriptor | 1,048,576 | Complete encoded bytes, including any supported version prefix |

The borrowed census precedes schema copying and descriptor encoding. Required-name and child counts are checked before bounded set allocation. Raw descriptor length is checked before strict JSON parsing; that bounded parse allocates a raw value tree before the descriptor census and typed decode. After it succeeds, an original-text walk tracks decoded keys separately for each object, skipping complete strings so punctuation in values is not mistaken for structure. Those sets are bounded by the complete descriptor length and unchanged strict-reader depth. Each decoded key is limited to 4,096 UTF-8 bytes before set insertion or error retention. This descriptor rule does not change last-value-wins duplicates in mapping input.

The logical-level cap is an upper bound, not a promise that all trees at that depth can be encoded or decoded. A nested Group introduces several JSON containers. The unchanged descriptor codec and strict JSON reader can reject a tree before the logical cap is reached. Preparation requires an actual codec round trip and preserves that typed refusal before emission and publication. The schema census does not introduce an expression-work or process-memory guarantee; ordinary program validation remains separate.

## Generation and runtime stages

The Project route checks options and borrowed schemas before lowering can copy them. An existing Program enters at preparation. Both routes require static-program admission, complete ordinary validation and actual descriptor round trips before ordinary emission produces the optional artifact set.

```mermaid
flowchart TD
    select[Explicit Project companion selection] --> options[Source then target option checks]
    options --> census[Borrowed source then target schema admission]
    census --> lower[Ordinary lowering]
    lower --> policy[Bounded schema and static-program admission]
    program[Explicit emitter with an existing Program] --> policy
    policy --> validate[Complete ordinary program validation]
    validate --> codec[Actual source then target descriptor round trips]
    codec --> emit[Ordinary emission plus optional companions]
    emit --> publish[Complete staged publication by the CLI writer]
    emit -. compiled generated API call .-> original[Original size and strict encoding]
    original --> descriptors[Admit source then target embedded descriptors]
    descriptors --> syntax[Complete JSON5 syntax and finite-token checks]
    syntax --> input[Object shape and typed schema projection]
    input --> mapping[One complete ordinary mapping evaluation]
    mapping --> output[Strict schema-shaped output and complete byte limit]
    output --> returned[Return one complete owned result]
```

At runtime, original size and encoding are checked before either descriptor. Rust text already has valid UTF-8; byte methods retain the original `Utf8Error`. C# text rejects unpaired UTF-16 surrogates and counts its UTF-8 bytes; byte methods use strict UTF-8 decoding. A BOM is accepted as trivia and still counts toward the original limit. C# null source or context arguments refuse before boundary work.

Both complete descriptors are admitted before syntax. Their strict parse errors retain their actual cause; duplicate decoded descriptor keys, including escaped aliases, refuse before typed decoding can hide them. The complete input is then normalized before root-object and typed-schema projection. Mapping is called once only after those checks succeed. Mapping or output failure returns no partial result. The output byte check includes the final LF and occurs after complete output validation and serialization.

## Syntax and numeric scope

The shared normalizers accept comments, trailing commas inside objects or arrays, single- or double-quoted Unicode strings, supported escapes and the fixed JSON5 whitespace set. Unquoted property names use the pinned Unicode 17.0.0 General_Category rule defined below. Quoted Unicode property names and string parsing retain their existing behavior; this change does not select companions or broaden schema admission.

### Unicode identifiers

Identifier membership follows the JSON5 category rule, rather than the newer Unicode `ID_Start`, `ID_Continue` or `XID` properties:

| Position | Accepted scalars |
| --- | --- |
| First | `$`, `_`, or categories `Lu`, `Ll`, `Lt`, `Lm`, `Lo`, `Nl` |
| Later | Every first-position scalar, plus `Mn`, `Mc`, `Nd`, `Pc`, U+200C ZERO WIDTH NON-JOINER and U+200D ZERO WIDTH JOINER |

ASCII letters, `$`, `_` and continuation digits retain their original rules. Combining marks and join controls are continuation-only. Other categories, surrogate code points, unassigned characters and derived-property exceptions such as U+2118 or U+00B7 are not admitted by this identifier rule. Quoting a name remains available without this membership restriction. Names are not normalized to NFC or NFKC; raw and escaped spellings that decode to the same name retain ordinary duplicate-property behavior.

Raw BMP and supplementary scalars are accepted when eligible in their position. An identifier escape is exactly `\u` followed by four hexadecimal digits. A high-surrogate escape must be immediately followed by a low-surrogate `\u` escape; the pair becomes one supplementary scalar before membership checking. Hex digits are case-insensitive; the `u` marker is lowercase. Identifier escapes do not accept `\x`, `\U` or `\u{...}`. For example, `{雪:0}` and `{\u96EA:0}` both normalize to `{"雪":0}`; `{𐐀:0}` and `{\uD801\uDC00:0}` both use the same U+10400 name.

Malformed identifier escapes and lone or mismatched escaped surrogates retain `InvalidEscape`. A well-formed escaped scalar outside its positional membership retains `UnsupportedIdentifier`; excluded raw non-ASCII scalars also retain that kind. Existing ASCII token/error distinctions remain unchanged. Syntax offsets are original UTF-8 byte positions, not character counts or normalized-output offsets. An invalid escaped pair is reported at its first backslash. For instance, `{"é":0,a😀:1}` reports `UnsupportedIdentifier` at byte 9. C# still validates the complete UTF-16 input before syntax and retains the absolute UTF-16 index, valid-prefix UTF-8 offset and original encoder cause for an unpaired source surrogate.

Both languages use static leaves derived from one canonical ASCII range representation; they do not query the host runtime's Unicode category version. The pinned data has 1,079 coalesced non-ASCII intervals and at most **11 binary interval probes** per lookup. ASCII membership bypasses that table. Generation caps the representation at 4,096 intervals and refuses a larger table; this is a lookup bound, not a parser-work or memory limit.

### Offline identifier data regeneration

The generator uses the [Unicode 17.0.0 UCD](https://www.unicode.org/Public/17.0.0/ucd/) originals and their complete license notice. It checks exact file length, SHA-256 and version, with a 4 MiB bound per input. Inputs must match these pins:

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| `extracted/DerivedGeneralCategory.txt` | 277,514 | `d62e5bab70ca74f099343f71224fa051cb1fdd61a1ab45c0488c44cfc0b6102e` |
| `UnicodeData.txt` | 2,198,209 | `2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c` |
| `ReadMe.txt` | 740 | `9fe1a90bd32659d7953616283dc2bffaa165518aae9ace026040c42c559ba606` |
| `LICENSE` | 1,995 | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` |

With those original files already available locally, run from the repository root in Bash. The output parent must exist and the first output directory must be fresh; no network access or extra Python packages are used:

```bash
JSON5_UCD17='/absolute/path/to/pinned-unicode-17'
JSON5_IDENTIFIER_OUTPUT='/absolute/path/to/fresh-identifier-output'
JSON5_IDENTIFIER_INPUTS=(
  --derived-general-category "$JSON5_UCD17/extracted/DerivedGeneralCategory.txt"
  --unicode-data "$JSON5_UCD17/UnicodeData.txt"
  --readme "$JSON5_UCD17/ReadMe.txt"
  --license "$JSON5_UCD17/LICENSE"
)
python3 scripts/generate_json5_identifiers.py \
  "${JSON5_IDENTIFIER_INPUTS[@]}" --output-root "$JSON5_IDENTIFIER_OUTPUT"
python3 scripts/generate_json5_identifiers.py \
  "${JSON5_IDENTIFIER_INPUTS[@]}" --output-root "$JSON5_IDENTIFIER_OUTPUT" --check
```

`identifier_ranges.txt` is the canonical representation. `identifiers.rs` and `Json5Identifiers.cs` use its same ordered intervals; both retain the complete notice also emitted as `LICENSE-UNICODE.txt`. `generation.json` records the input/output hashes, range count and probe bound. Each generation checks the complete category data against independently decoded `UnicodeData.txt` and the canonical lookup against its projection. `--check` regenerates all five output bodies in memory and requires complete byte equality with an existing output set. Compiled Rust/C# lookup and parser verification remain separate checks.

Duplicate input properties survive normalization and retain last-value-wins typed reading. Every lexical number is checked before projection, including an overwritten or undeclared member. `NaN`, signed infinities and decimal overflow to nonfinite values refuse at Syntax. Exact decimal and hexadecimal integer tokens use checked signed/unsigned 64-bit ranges. Decimal floating tokens keep their spelling, with a leading plus removed and missing decimal-side zeros supplied; they are not rounded and rewritten by the normalizer. Typed projection still applies the declared Int or Float domain, so a syntactically valid unsigned integer or an integer not exactly representable as Float can refuse at Input. Integer `-0` normalizes to `0`; a floating negative-zero lexical retains its sign.

## Runtime limits and typed causes

MiB means 1,048,576 bytes. The limits below apply to one API call; the policy limits above apply separately to each embedded schema.

| Resource | Maximum | Accounting |
| --- | ---: | --- |
| Original input | 64 MiB | Complete UTF-8 bytes, including BOM and trivia |
| Normalized strict JSON | 64 MiB | Emitted UTF-8 bytes before typed projection |
| Syntax container depth | 127 | Nested object/array containers |
| Syntax work | 536,870,912 | Charged grammar operations, repeated scans and emitted bytes |
| Strict JSON output | 64 MiB | Complete encoded result, including its final LF |

Syntax offsets refer to the original UTF-8 input. C# also retains absolute UTF-16 indices for text encoding failures; the retained encoder cause's span-scoped index is not a whole-input index. Encoding preflight and final owned-string copies are outside the syntax-work ledger. Schema parsing, typed projection, mapping and output serialization have their own existing checks; the syntax-work cap does not bound their work.

Rust returns `Json5BoundaryError`, retaining source/target schema, syntax, input, mapping and output causes through `Error::source`; resource variants retain requested and maximum values. C# uses `FerruleJson5BoundaryException` with `Stage`, schema side, limit fields and the actual `InnerException`. A descriptor failure can retain a direct `JsonException`; input/output paths retain the ordinary boundary chain when that path produces one. Mapping errors retain the original runtime exception and are not relabeled as syntax errors. C# argument errors remain `ArgumentNullException`.

These are serialized-boundary limits, not a process-memory ceiling or a streaming contract. Complete source text, normalized text, parsed input, mapped instances and serialized output can coexist. Output limits can be checked after allocation; buffer capacity, allocator/GC/runtime overhead and host-owned data are separate. See [Memory use and boundary limits](memory-and-limits.md).

## Physical boundary checks and measured memory

<a id="increment-checklist"></a>

The [physical-boundary study](https://github.com/DeandreT/ferrule/pull/159)
measured exact and one-byte-over inputs, normalized documents and outputs.
The normalized sizes came from independent corpus recipes; the public API does
not expose its intermediate buffer.

The table records whole-host peak resident memory in KiB. Each range covers four different routes: text and bytes, each with and without context. These are route ranges, not repeated trials of an identical call.

| Physical case | Rust peak RSS, KiB | C# peak RSS, KiB |
| --- | ---: | ---: |
| Original, exactly 64 MiB | 136,976–137,056 | 308,040–308,548 |
| Original, 64 MiB + 1 byte | 135,604–135,928 | 178,920–310,336 |
| Normalized, exactly 64 MiB | 105,380–106,164 | 472,980–473,168 |
| Normalized, 64 MiB + 1 byte | 82,328–82,632 | 225,120–226,652 |
| Output, exactly 64 MiB | 270,344–270,568 | 1,403,796–1,404,532 |
| Output, 64 MiB + 1 byte | 270,032–270,460 | 1,204,508–1,205,396 |

Measurement includes process startup, input reading, the public API call, returned-output retention and hashing, and the host's final evidence checks. Host builds and the outer controller's checks are outside that measurement. The Linux Rust hosts used the development profile with optimization level 0 and debug information level 2; the C# hosts used Release on .NET 10.

These ranges cover different routes, not repetitions of one call. For C#
allocation attribution and its limitations, see the [measured workload](performance/json5-allocations.md).
Whole-host RSS is not an API allocation limit.
