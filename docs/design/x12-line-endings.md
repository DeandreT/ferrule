# Explicit X12 segment line endings

An explicitly identified X12 target chooses its segment suffix. LF is the
default on every host.

## Selection and bytes

`mapping::X12SegmentLineEnding` is the closed string policy `lf` / `crlf`.
`FormatOptions.x12_segment_line_ending` defaults to LF and is omitted when LF,
including explicit LF. It belongs to an explicitly identified X12 target.
Unknown strings, null, non-strings and duplicate known fields refuse. Ordinary
FormatOptions unrelated unknown-field handling is unchanged.

For a visible segment terminator, LF emits `~\n` and CRLF emits `~\r\n` after
every written segment, including the final trailer. The separator remains one
character. Under the legacy LF policy, an LF terminator is written alone, and
native CR terminators are written alone. CRLF combined with either terminator
refuses. Generated strict profiles still refuse CR entirely. No host newline,
input auto-detection, encoding change or field normalization selects a policy.

Saved CRLF projects use the existing version-3 capability envelope with
`x12-segment-line-ending-v1`. Detection covers primary/named source and target
options, even invalid placements, and propagates through Pipeline and
DocumentProject. This prevents older file readers silently discarding CRLF.
Ordinary model serde is separate: manually removing the capability envelope
does not provide a safe interchange format for older readers.

## Entry points and bounds

All existing native function signatures, Separators and Autocomplete remain.
The new private-field `format_edi::x12::OutputOptions` builder supplies separator,
version, completion, suffix and optional exact UTF-8 ceiling to
`to_string_with_output_options` and `to_document_string_with_output_options`.
Legacy wrappers select LF. The private X12 style emits the chosen suffix;
EDIFACT, TRADACOMS and HL7 keep their existing styles. Completion reads the
legacy intermediate spelling and applies the selected policy/ceiling only to
the completed serialization. Actual encoded segment sizes, including completed
trailers, are checked with overflow handling before growing the final buffer.

Native CLI file output and bounded payload output pass the target policy.
The generated Rust boundary stores it in its immutable descriptor profile and
calls the native document writer after the existing private-view/lexical steps.
Existing scalar reservations and LF refusal thresholds remain; the selected
output uses the exact 64-MiB wire ceiling before segment append as well as the
existing final size and wire validation.

Raw X12, CSV/X12 and static/named strict X12 C# targets include exactly one new
`FerruleX12.OutputPolicy.cs` partial only when CRLF is selected. It removes only
the recognized descriptor member, preserves nested raw JSON/duplicate members
and old validation precedence, caches immutable profiles with weak descriptor
identity, and reuses private SerializeOutput and EncodeOutput. Only that strict
writer's completed canonical builder is eligible for suffix expansion: field
control text is refused and the terminator is visible. It counts all LF suffixes
across builder chunks and checks the final UTF-8 length before expansion. String
and byte APIs share this path, with no caller-owned instance mutation.

Source selection, other formats, ordinary/unsupported generation, legacy
primary EDI and shared FerruleEdi/document codecs refuse nondefault policy
rather than discard it. Faithful MFD export also refuses; import settings and
private reference behavior are not inferred or activated.

## Compatibility and artifact identity

Adding `segment_line_ending` changes Rust source compatibility for exhaustive
`FormatOptions`, `X12BoundaryOptions` and `X12Descriptor` literals or patterns.
Use `..Default::default()` where supported, or set the LF field explicitly;
descriptor literals use `X12SegmentLineEnding::Lf`. Exhaustive
`EdiFormatError` matches must handle `X12OutputLimit`.

Saved default-LF bytes and C# runtime membership stay unchanged. Only an
explicit CRLF C# target includes `FerruleX12.OutputPolicy.cs`; its manifest
hashes the actual runtime inventory. Rust source identity changes even for
regenerated LF libraries; an existing library remains tied to its own runtime.

<a id="authored-unexecuted-validation"></a>

## Test coverage

[Mapping persistence](../../crates/mapping/tests/x12_line_endings.rs),
[CLI routes](../../crates/cli/tests/x12_line_endings.rs),
[Rust document hosts](../../crates/codegen-rust/tests/x12_documents/line_endings.rs)
and [C# hosts](../../crates/codegen-csharp/tests/x12/line_endings.rs) exercise
different boundaries. The controls distinguish LF/CRLF bytes, final trailers,
exact/+1 output limits and caller immutability. The C# composition record
protects the default artifact; these generic fixtures do not prove parity for
a particular saved design.
