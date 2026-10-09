# Generated C# X12 companions

Generated C# libraries can include an explicit raw X12 boundary around the
ordinary typed mapping. The supported profile is one 004010 interchange,
functional group and transaction. Source and target schemas, separators and
value constraints are embedded in the generated source; execution does not
download schemas or read an external EDI configuration.

## Select the boundary

```sh
cargo +nightly run -p cli -- generate \
  --project project.json \
  --language csharp \
  --out generated-csharp \
  --x12-adapters
```

The flag defaults to false, requires C#, and conflicts with `--csv-output` and
`--json5-adapters`. The public writer is
`cli::generate_project_with_x12_adapters`; the emitter is
`codegen_csharp::emit_with_x12`, using a `codegen::X12BoundaryPolicy` with
explicit source and target choices. At least one side must select X12.

For the CLI companion request, a retained X12 component family selects that
side, including an imported component whose stored instance path ends in
`.txt`. Otherwise an `.edi` or `.x12` identity selects X12, and an explicit JSON
identity selects strict JSON. Without a stored format identity, the schema's
declared EDI dialect can select X12. A different retained family or physical
format is refused. These choices only apply after the caller requests the
companion; stored paths and metadata never add it to ordinary generation.

The complete optional contract is admitted before the public writer stages a
source tree. An existing destination is not replaced. Ordinary generation
retains its artifact set and typed/JSON APIs. The companion adds
`GeneratedMapping.X12.cs` and package-free `Runtime/X12` sources to the standalone
.NET 10 project.

## Public generated methods

Methods are emitted for the selected direction, on
`Ferrule.Generated.GeneratedMapping`.

| Selected boundary | Text mapping method | Strict UTF-8 byte mapping method |
| --- | --- | --- |
| X12 source, JSON target | `ExecuteX12ToJson(string)` | `ExecuteX12ToJsonBytes(byte[])` |
| JSON source, X12 target | `ExecuteJsonToX12(string)` | `ExecuteJsonToX12Bytes(byte[])` |
| X12 source and target | `ExecuteX12ToX12(string)` | `ExecuteX12ToX12Bytes(byte[])` |

Each method also has an overload taking `FerruleExecutionContext`. A call
parses its source once, runs the ordinary typed mapping once, then serializes
the complete target. Mapping evaluation, runtime parameters and execution
context semantics remain those of the typed API. The companion returns one
complete string or byte array; it does not publish files or transport messages.

An X12 source also adds `ParseX12(string)` and `ParseX12Bytes(byte[])`, returning
a `FerruleInstance`. An X12 target adds `SerializeX12(FerruleInstance)` and
`SerializeX12Bytes(FerruleInstance)`. These helpers use the same embedded
boundary contract as the mapping methods.

## Schema and metadata

The program has a singular primary document and no named sources, named
targets, primary-document iteration or XML boundary. Ordinary graph lowering
and validation still determine whether its reachable mapping operations are
portable.

The X12 schema is an ordered group tree with segments, optional or repeated
loops, and positional elements. Segment names use their uppercase two- or
three-character X12 identifier, optionally with the supported `MF_` prefix or
numeric suffix. Elements are String, Int or finite Float scalars, or a single
flat composite group containing those scalars. Elements and composite
components cannot repeat. Duplicate sibling names, nested composites and
other scalar kinds are refused.

String values retain their source text, and length constraints count Unicode
scalar characters. Empty elements project to Null after their fixed, length and
code-list checks. Int and Float leaves use invariant numeric parsing and typed
values; they do not preserve an arbitrary numeric input spelling. A declared
numeric fixed literal retains its wire spelling when the supplied typed value
equals it. Identifiers whose leading zeros matter therefore use String fields.
Nonempty numeric fixed metadata must itself be a valid Int64 or finite Float;
invalid numeric defaults are refused as schema errors before they can materialize.

Supported schema metadata consists of fixed scalar values, String character
length ranges and cardinality on repeating loops or segments. Embedded EDI
value constraints retain their scalar paths, minimum/maximum character lengths
and exact code lists. Constraint paths must resolve to declared leaves. Other
IR metadata, leniency, release/repetition characters, implied decimals,
lexical compaction and autocomplete options are refused rather than silently
discarded. External configuration must already be resolved into the admitted
schema and options.

The schema contains exactly one each of ISA, GS, ST, SE, GE and IEA, in that
order and outside repeating loops. They have exactly 16, 8, 2, 2, 2 and 2
String fields respectively. ISA12 is fixed to `00401`; GS08 is fixed to
`004010`. String envelope fields preserve lexical controls and identifiers,
including leading zeros. Static qualifiers and fixed values are validated on
both input and output. Declared fixed literals may materialize from their schema
metadata. Controls must still be supplied as nonempty String values; fixed
metadata does not allocate a control or obtain a date/time from a clock.

The generated boundary consumes the complete interchange. Undeclared segments,
extra elements or components, ambiguous unsupported shapes and mismatched
qualifiers fail. An EDI constraint with a positive minimum length also rejects
an empty or missing value. This strict profile does not inherit the native
adapter's lenient handling of extra fields or missing Null constraints.

## Physical syntax and envelope

Text must contain valid Unicode and byte methods require strict UTF-8.
Malformed UTF-8 and unpaired UTF-16 surrogates fail. A BOM or leading whitespace
is not removed: a complete fixed-width ISA begins at offset zero. ISA fields
must have their declared ASCII widths. The reader discovers element,
component and segment separators from that ISA and verifies explicitly
configured separators against it.

Element and component separators are distinct visible ASCII punctuation.
The segment terminator is a third distinct punctuation character or LF.
Release and repetition syntax are outside this 004010 profile. Formatting
whitespace is allowed between completed segments; control characters inside a
segment fail. Data containing an unrepresentable separator cannot be escaped
or substituted by this adapter.

Output uses the configured separators, or `*`, `:` and `~` when none are
selected. ISA's fixed-width text fields are padded as an encoding operation.
Other segments omit trailing empty elements. A punctuation terminator is
followed by LF; LF terminators already provide the line break. Returned bytes
are UTF-8 without a BOM.

The qualification fixtures use a separate width-formatted ISA input for the
native comparison. The generated companion receives unpadded text and applies
its fixed-width encoding. Both paths compare against the same independent
complete 945 byte oracle.

The host supplies all envelope controls, dates, times and trailer counts.
The boundary validates the single ISA/GS/ST and matching SE/GE/IEA ordering,
lexical control shapes, matching controls and exact transaction/group counts.
ISA06/ISA08 and GS02/GS03 require nonblank sender and receiver identities.
Envelope dates must be valid calendar dates: ISA's `YYMMDD` uses the `20YY`
century, and GS uses `CCYYMMDD`. ISA time is `HHmm`; GS time is `HHmm`,
`HHmmss`, or `HHmmss` followed by one or two fractional digits. Hours, minutes
and seconds must be valid clock values. The adapter does not allocate
controls, read the clock, repair counts or generate
acknowledgments. Hosts that retry or transport output own durable control
allocation and retention of the resulting bytes.

## Bounds and failures

| Boundary | Maximum |
| --- | --- |
| Embedded descriptor per side | 1 MiB |
| X12 input or output, measured as UTF-8 | 64 MiB |
| Schema depth | 64 |
| Embedded schema JSON container depth | 128 |
| Schema nodes, at generation and runtime | 10,000 |
| Elements or components in one positional collection | 1,024 |
| Segments | 100,000 |
| Repeated loop/segment instances, shared across collections | 100,000 |
| Runtime traversal nodes | 1,000,000 |

Limits bound admission and traversal. Parsing and serialization still materialize
the complete document; these are not a streaming API or a process-memory cap.
The shared descriptor codec also applies its JSON container-depth ceiling;
each nested schema group introduces several JSON containers, so a constructed
tree below the schema-node depth limit can still exceed the descriptor limit.
Generation also requires a successful lossless round trip through that codec;
the depth ceilings do not override an earlier codec refusal.

Generation refusals retain `X12BoundaryPolicyError` through `X12EmitError`.
Runtime boundary refusals throw `FerruleX12Exception`, whose `Error` is one of
`UnsupportedProfile`, `Schema`, `Encoding`, `Syntax`, `Envelope`, `Value` or
`ResourceLimit`. Optional `SegmentIndex` and `FieldPath` identify structure;
messages do not include document field values. Ordinary typed mapping failures
retain their existing runtime types, and the unselected JSON side retains the
strict JSON boundary's errors. A failed call returns no partial document.

This profile supplies a format boundary. A particular transaction's semantic
rules still require its authored schema, graph and independent full-value or
full-byte fixtures. Transport, acknowledgments, control persistence and
external trading-partner acceptance remain host responsibilities.

## Repeatable qualification

Self-authored fixtures and complete independent oracles live in
`crates/codegen-csharp/tests/x12/`. The `x12_dotnet` integration target compares
native order values and shipment bytes, checks descriptor-depth admission, and
separately compiles fresh standalone C# libraries for source, target and both-side
profiles. Its compiled cohort invokes text, byte, execution-context and direct
helper methods, with ordinary typed/JSON controls and failure fixtures.

Run the native and descriptor tests with the repository's nightly toolchain:

```sh
cargo +nightly test -p codegen-csharp --test x12_dotnet
```

The compiled cohort is explicitly ignored in an ordinary test run. It requires
`dotnet` on PATH with the .NET 10 SDK and targeting pack installed. The generated
libraries and harness are package-free; their build clears NuGet package sources
and disables runtime-pack downloads. Run that cohort explicitly:

```sh
cargo +nightly test -p codegen-csharp --test x12_dotnet -- --ignored
```

The target prints its owned `ferrule_x12_native_*` or `ferrule_x12_cohort_*`
temporary evidence directory. Complete fixtures, generated source, command
outcomes, stdout/stderr and produced artifacts are retained before comparisons,
including failed runs. Keep those directories when investigating a failure;
source emission alone does not establish compiled execution. Direct runtime
controls also live in `runtime/csharp/Ferrule.Runtime.SmokeTests/X12Tests.cs`.
