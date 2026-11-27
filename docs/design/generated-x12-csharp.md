# Generated C# X12 companions

The C# X12 companion parses and writes raw documents around a typed mapping
using embedded schemas and options. Select a single transaction (default),
declared groups in one interchange, or repeating interchanges; each
endpoint owns its format and version. No external EDI configuration is read.

## Select the boundary

```sh
cargo run -p cli -- generate \
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

For the single and grouped profiles, the fixed ISA12 and GS08 schema values
select an exact version pair; repeating interchanges select it from the retained
interchange version instead:

| ISA12 | GS08 and descriptor version | ISA11 |
| --- | --- | --- |
| `00401` | `004010` | `U`; configured repetition metadata is inactive |
| `00501` | `005010` | Active repetition delimiter |
| `00604` | `006040` | Active repetition delimiter |

Each selected side retains its own pair. A saved `x12_interchange_version`
must agree with its fixed ISA12. Missing, noncanonical or conflicting declarations
fail generation; the adapter does not derive a GS08 implementation convention
or convert envelope versions. An explicit mapping can construct another
supported target pair; the graph must supply its headers. In the single and
grouped profiles ST has exactly two fields, so an implementation reference in
ST03 is outside them.

`X12BoundaryOptions.interchange_version` constrains the saved version; it
does not select one independently. Rust callers may use `..Default::default()`
for omitted options.

### Declared grouped envelopes

`X12EnvelopeProfile::SingleTransaction` is the default. Select
`X12EnvelopeProfile::GroupedTransactions` on each intended X12 endpoint's
`X12BoundaryOptions`; retained format options do not infer grouped ownership.
The descriptor omits the default `envelope_profile` field and writes
`"grouped_transactions"` for explicit grouped ownership. An absent descriptor
field keeps the singleton contract.

For primary CLI companions, select either direction independently:

```sh
ferrule generate --project project.json --language csharp --out generated \
  --x12-adapters \
  --x12-source-envelope-profile grouped-transactions \
  --x12-target-envelope-profile grouped-transactions
```

Both envelope flags require `--x12-adapters` or `--static-document-adapters`.
An explicit selection, including `single-transaction`, on a JSON endpoint fails
admission. The additive writers `cli::generate_project_with_x12_envelope_profiles`
and `cli::generate_project_with_static_document_envelope_profiles` accept optional
source and target selections; the existing three-argument writers delegate with
neither selected. With static document adapters the flags select the primary
X12 endpoints only. Backend refusal precedes project loading. A Rust
static-document policy can select grouped ownership on its own named X12
endpoints; the primary CLI flags do not override named endpoints.

Grouped schemas contain exactly these declared owners:

- A singular root with ISA first, one repeating functional-group container,
  and IEA last.
- Each functional-group template with GS first, one repeating transaction
  container, and GE last.
- Each transaction template with ST first, its ordered body segments and loops,
  and SE last.

The six envelope templates are unique. Owner containers have non-segment names;
each envelope segment is singular and has its existing exact String element
width. Other nested envelope segments, missing owner trailers and additional
interchanges fail admission. All supported version pairs, scalar/composite
rules and directional options still apply. A generated document requires at
least one functional group and at least one transaction within every group.

Input validates physical owner transitions, each GS version, inclusive ST-through-SE
segment counts, each group's transaction count, and interchange group count.
Controls must match their owning header. Errors retain the physical segment
index and declared owner path with occurrence indices. Counts or equal control
values alone never establish ownership. Body qualifiers and scalar validation
run after envelope validation.

On output, format values first, complete with the supplied context, then
validate scalars and owners. The mapping supplies GS/ST groups; completion
can supply missing SE/GE/IEA and empty control fields but cannot repair a
wrong nonempty count. Fallback ST numbers span groups. Replacements use
declared slot and occurrence indices, so the same Group object reused twice
can receive different controls without mutation. Limits never reset per owner.

Once the companion is requested, a retained X12 family wins even for a
`.txt` path. Otherwise `.edi`/`.x12` or explicit strict JSON can select a
side; only in the absence of stored identity can a declared schema dialect
select X12. Other families refuse. The writer admits the complete contract
before staging `GeneratedMapping.X12.cs` and the .NET 10 `Runtime/X12`
sources, and does not replace an existing destination.

### Repeating interchange envelopes

`X12EnvelopeProfile::REPEATING_INTERCHANGES` admits the hierarchy below from
an imported X12 `Envelope.Config`. Select it on each intended X12 endpoint
with the CLI value `repeating-interchanges` for either envelope flag or through
`cli::generate_project_with_x12_envelope_profiles` (or its static-document
counterpart). The serialized descriptor retains its existing profile identifier.
The shape is never inferred from
a schema or retained options. Static-document and CSV routes apply the same
selection to their own X12 endpoints.

```text
Envelope                 singular root
└─ Interchange           repeating container
   ├─ ISA                repeating segment
   ├─ Group              repeating container
   │  ├─ GS              repeating segment
   │  ├─ Message_<type>  repeating container: ST, body, SE[, ParserErrors_*]
   │  ├─ GE              repeating segment
   │  └─ ParserErrors_*  optional, source only: repeating acknowledgment branch
   └─ IEA                repeating segment
```

The tree above is exact. Optional envelope segments import as repetitions;
singular ISA/GS/GE/IEA variants refuse. ST is singular; SE may be singular
or optional (repeating in the imported model). Output allows at most one SE
per message and creates a missing one only with completion. The message
body contains no envelope segments. Other admission rules:

- ISA with 16 elements, GS with 8, ST with 2 or 3 (a String ST03), and SE, GE
  and IEA with 2. ISA13, GS06, SE01, GE01, GE02, IEA01 and IEA02 may be String,
  Int or Float. Every other envelope
  element is String, and no envelope element repeats.
- A retained `interchange_version` of `00401`, `00501` or `00604`, which
  selects `004010`, `005010` or `006040`. ISA12 and GS08 need not be fixed; a
  fixed value must agree with the selection. The existing ISA11/ISA16 fixed
  syntax rules apply.

A component with acknowledgment validation enabled imports two extra
`ParserErrors_*` branches: a trailing member of the functional group after GE
(starting with AK9) and a trailing member of the message after SE (AK3, AK4,
AK5 and similar). A source schema may carry either or both, exactly in those
trailing positions, as repeating non-segment containers without envelope
segments; their contents follow the ordinary segment rules. A target schema
refuses them, because output never writes parser acknowledgments. A
`ParserErrors_*` node anywhere else, an unprefixed trailing member, any other
extra child in the envelope grammar, more than one message container per
group, implied decimals on envelope elements and release separators are
refused. Each refusal is an `X12BoundaryPolicyError::Schema` with its owner
path and reason.

Input validates the physical segments as `(ISA (GS (ST … SE)+ GE)+ IEA)+`
before typed reading. Every interchange needs at least one functional group and
every group at least one transaction. Every ISA must carry the selected ISA12
(`UnsupportedProfile`) and declare valid ISA11 and ISA16 syntax
(`Syntax`). Without a configured separator set, each interchange's own syntax
applies to its body; configured separators must match every ISA. Under `00401`,
ISA11 is `U` and any other value is
`UnsupportedProfile`. GS08 is ordinary data: it is never
compared with the version or filled. Numeric wire controls are ASCII digits:
ISA13 has exactly nine, while GS06, GE02, IEA02 and the counts have one through
nine. SE01 counts ST through SE inclusive; GE01 and IEA01 count their owner's
transactions and groups. Counts, ISA13/IEA02 and GS06/GE02 compare by numeric
value; ST02 (four to nine digits) and SE02 compare as exact text. Identities,
dates, times and indicators keep the single-profile checks. `Envelope` errors
carry the physical segment index and declared owner path with occurrence
indices, such as `Interchange[1]/Group[0]/Message_945[0]/SE`. Typed reading then
uses occurrence-indexed paths, as in the grouped profile. Int and Float controls
read as native does, so `000000321` becomes 321.

Native reading treats acknowledgment branches as ordinary loops matched on
their segment IDs, so ordinary input reads each one as an empty collection;
generated input produces the same empty `FerruleRepeated`. A segment that
native would read into a branch, such as AK9 after GE or AK5 after SE, is
refused as an `Envelope` error at the branch occurrence path, such as
`Interchange[0]/Group[0]/ParserErrors_Group`. Acknowledgment content is outside
the profile.

Output mirrors the native writer:

1. Selected lexical formats produce the private output view.
2. One document-order traversal writes the segments. Typed values become wire
   text; an Int or Float envelope control must be a non-negative integer
   (`Value`), and a Float uses its shortest integral text. Fixed, length and
   constraint checks apply to supplied values; with completion selected, an
   empty completion-owned envelope field defers to completion. Each
   interchange occurrence has exactly one ISA and at most one IEA, and each
   group occurrence exactly one GS and at most one GE (`Envelope`). A missing
   SE is created only when completion is selected. An empty ISA12 takes the
   selected version, an empty ISA16 the component separator and, for
   `005010`/`006040`, an empty ISA11 the repetition separator.
3. Selected completion runs over the written segments as native completion
   does: ISA defaults, an ISA13 default of `000000000` zero-padded to nine
   digits, document-global GS06 and ST02 ordinals, SE01/GE01/IEA01 counts per
   owner, SE02/GE02/IEA02 copied from their headers, and SE, GE or IEA created
   when absent. Nonempty supplied values remain for final validation.
4. Every ISA receives its fixed-width encoding, and an all-digit IEA02 shorter
   than nine digits is zero-padded to nine, as the native writer does.
5. The input envelope validation runs on the final segments before
   serialization.

Without completion, a Float ISA13 must already have nine digits: 100000123
writes, while 123 fails as an ISA width error, as natively. Completion writes
123 as `000000123`.

This envelope check is stricter than native structural reading or completion.
For example, it refuses an empty group, a second trailer for one owner, a
later ISA with the wrong version or a segment that populates an acknowledgment
branch. A matching count alone does not establish ownership.

## Public generated methods

Methods are emitted for the selected direction, on
`Ferrule.Generated.GeneratedMapping`.

| Selected boundary | Text mapping method | Strict UTF-8 byte mapping method |
| --- | --- | --- |
| X12 source, JSON target | `ExecuteX12ToJson(string)` | `ExecuteX12ToJsonBytes(byte[])` |
| JSON source, X12 target | `ExecuteJsonToX12(string)` | `ExecuteJsonToX12Bytes(byte[])` |
| X12 source and target | `ExecuteX12ToX12(string)` | `ExecuteX12ToX12Bytes(byte[])` |

Each method has a `FerruleExecutionContext` overload and returns one complete
string or byte array. It parses once, calls the typed mapping once and
serializes once; the same context reaches output completion. Directional
helpers are `ParseX12` / `ParseX12Bytes` (returning `FerruleInstance`) and
`SerializeX12` / `SerializeX12Bytes` (with optional context overloads).
Publication remains the caller's responsibility.

## Static named documents and selected output

`--static-document-adapters` selects a separate C# companion for a complete
static JSON/X12/EDIFACT boundary table. Every primary and named endpoint retains its
own schema, format options and separators. At least one endpoint is X12 or EDIFACT. The
flag conflicts with `--x12-adapters`, `--json5-adapters` and `--csv-output`.
The public writer is `cli::generate_project_with_static_document_adapters`;
the emitter is `codegen_csharp::emit_with_static_document_adapters`, using a
`codegen::StaticDocumentBoundaryPolicy` whose named entries exactly match the
complete declaration order. This route adds `GeneratedMapping.Documents.cs`
and the same `Runtime/X12` sources. EDIFACT endpoints also use the
[shared EDI codec with strict envelope validation](static-edifact-documents.md).

The CLI selects an endpoint from its explicit saved X12/EDIFACT family, strict JSON
flag, or `.edi`, `.x12` or `.json` path identity. Unknown identities and other
physical formats fail admission. Schema shape and document contents do not
select a codec. All endpoints are static single documents; dynamic physical
loaders, output-path plans and document-set iteration fail admission. Ordinary
typed `CopyOf` and computed JSON object construction keep their existing
semantics when their own schemas admit one output document.

Borrowed admission encodes every primary source, named source, primary target
and named target before recursive ordinary validation or schema copies. Each
descriptor keeps the existing 1 MiB codec limit and exact depth/serialization
errors, with its endpoint owner. X12 endpoints additionally keep the stricter
X12 schema bounds. An unused source and an unselected target must still have an
admitted descriptor. This is per-endpoint admission; it establishes no combined
document or total-memory bound.

The methods on `Ferrule.Generated.GeneratedMapping` are:

```csharp
SelectedDocumentTargetOutput ExecuteDocumentSelectedTarget(
    string source, FerruleTargetSelection selection);
SelectedDocumentTargetOutput ExecuteDocumentSelectedTargetWithHost(
    string source, FerruleTargetSelection selection,
    IReadOnlyList<NamedDocumentInput> extraSources,
    FerruleExecutionContext? executionContext = null);
SelectedDocumentBytesTargetOutput ExecuteDocumentBytesSelectedTarget(
    byte[] source, FerruleTargetSelection selection);
SelectedDocumentBytesTargetOutput ExecuteDocumentBytesSelectedTargetWithHost(
    byte[] source, FerruleTargetSelection selection,
    IReadOnlyList<NamedDocumentBytesInput> extraSources,
    FerruleExecutionContext? executionContext = null);
```

`NamedDocumentInput` and `NamedDocumentBytesInput` contain `Name` and `Document`.
The result is either a `Primary` carrier with `Format` and `Document`, or a
`Named` carrier whose `Output` also retains the declared `Name`. `Format` is
`DocumentBoundaryFormat.Json`, `.X12` or `.Edifact`; select the target with
`FerruleTargetSelection.Primary()` or `.Named(name)`.

Calls first reject a null source, then a null selection, then resolve the
selection using exact ordinal names. Unknown targets fail before named-input
checks or document parsing. After checking the input list itself, preflight
visits entries in caller order: null entry, null name, text-document null,
unknown name, then duplicate name. Missing inputs fail in declaration order.
For a program with no named sources, entry and name checks precede the
unexpected-source error, and the supplied document is not inspected.

Parse the primary first, then every supplied named document in caller order,
including unused inputs. A null byte document fails at its own parse turn
(`extraSource.Document`). Each endpoint uses its own codec and limits.
The typed call runs global failures in declaration order, constructs only
the selected target and serializes with that target's codec. X12 completion
uses the same context. Failure returns no output; caller data stays unchanged.

## Default singular schema and metadata

The program has a singular primary document and no named sources, named
targets, primary-document iteration or XML boundary. Ordinary graph lowering
and validation still determine whether its reachable mapping operations are
portable.

The X12 schema is an ordered group tree with segments, optional or repeated
loops, and positional elements. Segment names use their uppercase two- or
three-character X12 identifier, optionally with the supported `MF_` prefix or
numeric suffix. Elements are String, Int or finite Float scalars, or a single
flat composite group containing those scalars. Under `00501` and `00604`, a body
element may repeat as described in
[Repeated element values](#repeated-element-values). Composite components,
envelope elements and `004010` elements cannot repeat. Duplicate sibling names,
nested composites and other scalar kinds are refused.

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
IR metadata is refused. External configuration must already be resolved into
the admitted schema and options; unresolved references remain a generation error.

Selected format metadata has explicit directionality:

| Retained option | X12 input | X12 output |
| --- | --- | --- |
| `lenient_segments` | Skip unknown IDs while preserving every segment ID declared in the admitted layout for validation | Inactive |
| `edi_implied_decimals` | Divide a Float leaf by its configured power of ten once, including repeated instances; preserve Null | Inactive |
| `edi_lexical_formats` | Inactive | Compact configured dates, times and plain decimals in a private output view |
| `EdiAutocomplete::X12` | Inactive | Complete selected empty envelope fields and trailers using an explicit execution context |

Every retained path resolves to a scalar; duplicate, absent and nonleaf paths
fail admission. Active input implied-decimal paths require Float leaves and
one through eighteen places. Inactive output paths require Int or Float.
Output date and time formats require String leaves; decimal formats accept
String, Int or Float. Inactive input lexical paths still resolve to scalars.
Compact-time widths are within four through eight digits; decimal maximum
lengths are within one through 255 characters. Each option collection has at
most 10,000 paths, each with at most 64 components.

Descriptors retain `lenient_segments`, `implied_decimals`, `lexical_formats` and
`autocomplete`; omitted fields retain strict defaults. Unknown or malformed
descriptor fields fail before wire traversal. The optional autocomplete object
contains `request_acknowledgement` and an optional `transaction_set`. A selected
transaction set is three ASCII digits and agrees with a nonempty fixed target
ST01. This check uses the actual transaction segment; positional element and
composite names do not establish segment ownership. Other autocomplete
dialects and version pairs remain refused.

The default envelope schema contains exactly one each of ISA, GS, ST, SE, GE and IEA, in that
order and outside repeating loops. They have exactly 16, 8, 2, 2, 2 and 2
String fields respectively. ISA12 and GS08 have one of the exact fixed pairs
above. String envelope fields preserve lexical controls and identifiers,
including leading zeros. Static qualifiers and fixed values are validated on
both input and output. Declared fixed literals may materialize from their schema
metadata. Without selected completion, controls must still be supplied as
nonempty String values; fixed metadata does not allocate a control or obtain a
date/time from a clock.

The generated boundary consumes the complete interchange. Strict input rejects
undeclared segments. Selected lenient input skips only IDs absent from the
admitted layout. Declared segments in the wrong position or with a wrong fixed
qualifier remain visible and are refused by schema or loop-cardinality validation;
a matching declared segment's malformed value remains an error. Valid qualified
optional and repeating segments retain their ordered owners. This generated
malformed-segment guarantee is stricter than the native
reader's qualifier-based skip behavior.
Extra elements or components, ambiguous unsupported shapes and mismatched
required qualifiers fail. An EDI constraint with a positive minimum length also
rejects an empty or missing value; leniency does not relax scalar constraints.

## Physical syntax and envelope

Text must contain valid Unicode and byte methods require strict UTF-8.
Malformed UTF-8 and unpaired UTF-16 surrogates fail. A BOM or leading whitespace
is not removed: a complete fixed-width ISA begins at offset zero. ISA fields
must have their declared ASCII widths. The reader discovers element,
component and segment separators from each ISA and verifies explicitly
configured separators against every header. A later interchange can reuse
characters that had different delimiter roles in an earlier interchange;
tokenization and typed component/repetition reads stay with their own header.

Element and component separators are distinct visible ASCII punctuation.
The segment terminator is a third distinct punctuation character or LF.
Release syntax is outside these profiles. A configured repetition character
is distinct visible ASCII punctuation. It is inactive under 00401: ISA11 remains
`U`, and elements are not split on that character. For 00501 and 00604, the reader
discovers it from ISA11 and verifies an explicitly configured repetition character.
A configured input separator set with no repetition character permits discovery.
The active delimiter differs from the element, component and segment delimiters.
It is admitted only in declared repeating elements, and the generated reader
never keeps just the first occurrence of a non-repeating element.
Formatting whitespace is allowed between completed segments; control characters inside a
segment fail. Data containing an unrepresentable separator cannot be escaped
or substituted by this adapter.

Modern output with no separator set uses `*`, `:`, `~` and `^`, matching the
native writer defaults. An explicit modern output separator set includes a
repetition character. The writer never infers that syntax from a supplied ISA11.
Missing, Null or empty modern ISA11 materializes the selected delimiter;
nonempty values remain supplied and must agree. ISA16 retains its supplied or
fixed component value requirement. Explicit empty fixed ISA11/ISA16 metadata
fails modern admission; a fixed syntax value is one punctuation character.
Conflicting fixed values fail admission whenever the selected syntax is known.
Ordinary values containing the active repetition delimiter cannot be represented
and fail output validation.

### Repeated element values

A repeating body element in a `00501` or `00604` schema is a String, Int or
Float scalar or a flat composite. It carries no fixed value or item cardinality,
and its components carry no fixed values, because qualifier matching and fixed
materialization address one occurrence. Constraint, implied-decimal and
lexical-format paths through a repeating element apply to every occurrence.

Input splits a declared repeating element on the active repetition delimiter
and reads each occurrence with the ordinary scalar or composite rules. As in the
native reader, an absent trailing element has no occurrences, while a present
element, even an empty one, has one occurrence per delimited part: `REF*ZZ**7~`
reads one Null `REF02` occurrence, and `N9*X~` reads no `N902` occurrences.
Every occurrence counts toward the repeated-instance limit. Occurrence paths are
indexed only under the grouped and repeating-interchange profiles, matching loop
paths.

A segment ID with no declared repeating element refuses repetition syntax during
tokenization, before envelope, schema or mapping validation. That includes
envelope segments and unknown segments that lenient input would otherwise skip.
In a segment ID that declares a repeating element, a non-repeating element
containing the delimiter fails with the same `UnsupportedProfile` error at its
segment index.

Output requires an ordered collection for a repeating element; a missing or Null
value has no occurrences. Each occurrence is written with the ordinary scalar or
composite rules, and occurrences are joined with the selected repetition
delimiter. No occurrences write an empty element. A single Null occurrence also
writes an empty element, so at a trailing position it is trimmed and reads back
with no occurrences, as in the native writer.

ISA receives its fixed-width padding. Other segments omit trailing empty
elements. Output is UTF-8 without BOM. By default a visible terminator is
followed by LF; an LF terminator stands alone. A target can explicitly select
[CRLF segment suffixes](x12-line-endings.md) with a visible terminator. The
native writer also admits a CR terminator; this generated profile refuses it.
The native 945 fixture supplies a width-formatted ISA and the generated
fixture an unpadded ISA; both compare the complete output to the same bytes.

Without selected completion, the host supplies all envelope controls, dates,
times and trailer counts.
The boundary validates the single ISA/GS/ST and matching SE/GE/IEA ordering,
lexical control shapes, matching controls and exact transaction/group counts.
ISA06/ISA08 and GS02/GS03 require nonblank sender and receiver identities.
Envelope dates must be valid calendar dates: ISA's `YYMMDD` uses the `20YY`
century, and GS uses `CCYYMMDD`. ISA time is `HHmm`; GS time is `HHmm`,
`HHmmss`, or `HHmmss` followed by one or two fractional digits. Hours, minutes
and seconds must be valid clock values.

Selected X12 completion requires a supplied `FerruleExecutionContext.CurrentDateTime`
with a valid calendar date and clock time, an optional decimal fraction, and an
optional `Z` or `±HH:MM` timezone within the XML dateTime offset range. It never
obtains the current time from a clock. This generated validity check is stricter than native completion's
timestamp-prefix check. It fills completion-owned empty
fields, supplies deterministic local fallback controls, and materializes missing
SE/GE/IEA instances in the declared schema. Sender and receiver identities remain
caller supplied. Nonempty controls and counts are preserved and must satisfy the
final envelope checks. Completion does not repair a supplied wrong count or
control and does not generate acknowledgments. Hosts that retry or transport
output own durable control allocation and retention of the resulting bytes.
Completion preserves supplied four-, six-, seven- and eight-digit GS05 times,
just like ordinary output. A missing, Null or empty unformatted GS05 receives
the six-digit host timestamp fallback. ISA10 keeps its four-digit output contract;
completion truncates a supplied six-digit ISA time to hours and minutes.

Output first formats a private view, then performs selected completion, then
checks fixed values, lengths, code lists and the complete encoded envelope.
A missing or Null lexical leaf remains available for selected completion.
An empty String date or time with a selected lexical format fails with `Value`;
completion does not bypass that formatting rule. Other completion-owned empty
String fields remain eligible for their defaults. A malformed lexical value
therefore fails before a missing or malformed completion
timestamp, which is an `Envelope` error at the completion step. Mapping failure
still precedes output processing. Ordinary profiles retain their missing-data
rules. Native lexical and completion helpers define the individual formatting
contracts; the generated final-constraint ordering has its own error precedence.

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
| Repeated loop, segment and element instances, shared across collections within each stage | 100,000 |
| Runtime traversal nodes per stage | 1,000,000 |

Limits bound admission and traversal. Parsing and serialization still materialize
the complete document; these are not a streaming API or a process-memory cap.
The lexical view, completion trailer population, completion replacement and
final writer each use an independent bounded traversal budget. Strict output
and inactive reading metadata bypass the private preprocessing views.
Completion counts all retained envelope field slots before cloning a header or
trailer, including unknown slots when no lexical formatting is selected.
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

The [self-authored fixtures](../../crates/codegen-csharp/tests/x12/) supply
complete native value and wire oracles, including cases the generated profile
refuses. [`x12_dotnet`](../../crates/codegen-csharp/tests/x12_dotnet.rs) separates native
and descriptor checks, a compiled saved-profile cohort, and additional
compiled strict source/target/both-side calls. With the .NET 10 SDK and
targeting pack on `PATH`:

```sh
cargo test -p codegen-csharp --test x12_dotnet
cargo test -p codegen-csharp --test x12_dotnet -- --ignored
```

The second command runs explicitly ignored hosts; successful emission does
not exercise their public calls. The package-free harness retains complete
artifacts and outcomes before comparison.
