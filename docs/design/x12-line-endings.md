# Explicit X12 segment line endings

Issue [#1426](https://github.com/DeandreT/ferrule/issues/1426). Source preparation;
the implementation and tests have not yet been compiled or executed.

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

This 0.1 implementation intentionally adds public metadata fields to
FormatOptions, X12BoundaryOptions and X12Descriptor. **Rust source compatibility
is not unchanged:** exhaustive downstream literals/patterns must migrate.
FormatOptions/X12BoundaryOptions literals may use `..Default::default()` or an
explicit LF field; exhaustive patterns may add `..`. Descriptor literals must
set `segment_line_ending: X12SegmentLineEnding::Lf`. Known workspace exhaustive
construction sites in descriptor production/tests and the exhaustive JSON5
FormatOptions admission pattern were migrated. The private runtime Profile
test constructor was also updated. The new X12OutputLimit error variant requires
exhaustive downstream EdiFormatError matches to account for it.

No version bump or package interchangeability is promised. Rust runtime source
identity honestly changes globally, including newly regenerated default-LF
manifested libraries. Existing generated libraries remain bound to their old
runtime; runtime identity checks must not be weakened or old hashes fabricated.

Default LF descriptor members/order and C# runtime inventories remain unchanged.
Every existing C# runtime template stays byte-identical, including its original
checkout line endings. The optional partial is not added to global ordinary
identifier-word admission: its external type names are globally qualified and
its additional types are private nested members. Existing public mapping method
text is retained for LF; only selected output calls choose the additional
internal helper. Manifested CRLF C# output hashes its actual selected runtime
inventory. The project already compiles Runtime/**/*.cs, so selected membership
is sufficient; no default project-template change is needed.

The existing 9,217-line `csharp_composition.txt` is protected and must pass
unchanged on qualified source bytes. No update flag, token delta or regeneration
is part of this change. Source-byte conservation is not a claim that the
artifact-record test has run.

## Authored, unexecuted validation

- Mapping tests: strict string enum, omitted/explicit LF canonical bytes, CRLF,
  malformed/duplicate full and staged descriptors, nested duplicates, capability
  placement, plain/version-2 refusal, mismatch/unknown capability and delegated
  project/pipeline/document persistence.
- Native/runtime/admission tests: suffix bytes, repeated segments, internal
  holes/trailing omissions, final termination, empty output, CR/LF conflicts,
  exact/+1 small ceilings, unchanged caller data and old error precedence.
- CLI tests: file/bounded payload/named target forwarding and source/foreign
  dialect/ordinary-generation/export/shared-document refusals.
- Pure C# generation tests: deterministic selection, preserved old runtime
  files, one optional partial, manifested actual runtime inventory, CSV/static
  forwarding and unchanged ordinary identity acceptance.
- Modest Rust and package-free C# Release consumers: default LF and CRLF,
  supplied trailers and actual MapForceInterchanges completion, repeated public
  string/byte/context/noncontext calls, caller snapshots, literal ASCII expected
  wires and C# bounded parser/duplicate/precedence/small expansion-limit cases.

These tests are not automatically ignored. Parent must authorize and run finite
qualification after the existing compiler lane drains, resolve compiler/test
findings, and check the protected record without regeneration. Whole-MFD or
private reference equivalence is not established by these generic cases.
