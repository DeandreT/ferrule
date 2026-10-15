# Complete static document outputs

The static-document emitter retains the shared JSON/X12 endpoint admission
policy. It emits both the existing selected-target APIs and these complete
physical-output APIs on `Ferrule.Generated.GeneratedMapping`:

- `ExecuteDocumentOutputs(string source)`
- `ExecuteDocumentOutputsWithHost(string source, IReadOnlyList<NamedDocumentInput> extraSources, FerruleExecutionContext? executionContext = null)`
- `ExecuteDocumentBytesOutputs(byte[] source)`
- `ExecuteDocumentBytesOutputsWithHost(byte[] source, IReadOnlyList<NamedDocumentBytesInput> extraSources, FerruleExecutionContext? executionContext = null)`

`DocumentExecutionOutputs` and `DocumentBytesExecutionOutputs` expose
`PrimaryFormat`, `Primary`, and `Extras`. Each named output retains its exact
declaration name, own format and serialized document. Extras are read-only and
remain in declaration order, not caller-input order or alphabetical order.
Returned byte arrays belong to the caller; no source byte array is reused.

All declared inputs, including unused named inputs, are admitted before mapping.
Each document is parsed once. One ordinary complete mapping shares the supplied
context, clock and runtime parameters across primary and all named targets.
Each target is written with its own descriptor, including explicitly selected
X12 ownership, syntax, version and completion settings. No implicit clock or
profile inference is added.

All output writers must finish before the bundle is returned. A late failure
returns no primary-only or partially populated result. This does not roll back
host side effects, provide external file publication, or promise streaming/RSS
bounds. Input byte arrays must not be mutated concurrently during the call.

Each complete input and output bundle has a 4096-document and 256-MiB strict
UTF-8 budget, including primary. Existing per-codec document and traversal
limits also apply. Codec/encoding/budget failures preserve their original
inner cause in `DocumentExecutionException`, together with primary or exact
named endpoint ownership. Whole-bundle count/alignment errors have no endpoint
owner. Ordinary mapping and source-name exceptions remain unchanged.

This boundary admits static, single-document JSON/X12 endpoints with at least
one X12 endpoint. XML, dynamic sources/targets and document iteration are not
newly admitted. Nested X12 transaction/group occurrences already admitted by
an explicit profile are distinct from document iteration.

## Literal fixtures and focused tests

The fixtures are invented and self-contained. No expected output is computed
by another backend or by a selected-target call. JSON expected files retain the
production writer's two-space indentation and final LF. JSON and X12 fixtures
preserve every output byte; neither expected nor actual outputs are normalized.

- Mixed: JSON primary, named JSON/X12/JSON in `Zeta`, `Wire`, `Alpha` order;
  reversed supplied input order; context/parameters; custom 005010 syntax and
  envelope completion; malformed unused input; strict encoding; incorrect
  source types/names; late X12 and later JSON writer failures on both routes.
- Primary X12: the same policies exchanged between primary and named `Wire`,
  proving formats and ownership belong to endpoints rather than codecs/names.
- Grouped: actual grouped X12 source and true named grouped X12 output with two
  transactions, requiring explicit source and named-target profile selection.
- Single: X12 primary-only text/bytes calls and empty-named host overloads.

The source-level tests check actual emitted methods and policy refusals. The
explicit SDK test is:

```text
cargo test -p codegen-csharp --test static_document_outputs generated_static_document_outputs_match_literals_and_refuse_whole_bundles -- --ignored --exact --nocapture --test-threads=1
```

It retains complete generated sources, literal inputs, actual bundles, original
failures, build/host output and input/source guards in a unique temporary
directory. It does not remove that evidence on success or failure. Private
serializer/budget checks supplement, but never replace, physical API execution.
