# Generated XML qualification index

This index maps the current public Rust and C# XML document APIs to their source
tests and remaining verification work. A source fixture is evidence that a test
has been written; it is not evidence that generation, compilation or execution
passed. This documentation change runs no mappings and promotes no conformance
status. The index is finite, rather than a complete parity inventory.

Use the [XML host guide](code-generation.md#xml-host-boundary) for signatures and
examples. The authoritative adapter dispatch and admission are
[`XmlOutputMode`](../crates/codegen/src/model.rs) and
[`validate_boundary`](../crates/codegen/src/validate/xml.rs). The two emitters are
[`codegen-rust::xml_api`](../crates/codegen-rust/src/xml_api.rs) and
[`codegen-csharp::mapping::xml_api`](../crates/codegen-csharp/src/mapping/xml_api.rs).
Successful typed or JSON generation does not by itself establish an XML adapter.

## Route families

Every row has text and UTF-8 byte inputs and an execution-context variant.
The table shows the text method without a context. In Rust, replace
`execute_xml` with `execute_xml_bytes` for the byte method. In C#, replace
`ExecuteXml` with `ExecuteXmlBytes`. Input bytes and returned bytes are distinct
coverage cells even when one call exercises both.

| Input domain and output form | Rust text method | C# text method | Source fixture family |
| --- | --- | --- | --- |
| Structured or admitted RootView primary → one primary document | `execute_xml` | `ExecuteXml` | [structured_xml_public](../crates/cli/tests/code_generation/structured_xml_public.rs), [xml_text](../crates/cli/tests/code_generation/xml_text.rs) |
| Primary → primary plus static named documents | `execute_xml_outputs` | `ExecuteXmlOutputs` | [xml_output_sets](../crates/cli/tests/code_generation/xml_output_sets.rs) |
| Primary plus complete static named inputs → primary or static output set | `execute_xml_with_sources`, `execute_xml_outputs_with_sources` | `ExecuteXmlWithSources`, `ExecuteXmlOutputsWithSources` | [xml_input_sets](../crates/cli/tests/code_generation/xml_input_sets.rs), [rootview_named_inputs](../crates/cli/tests/code_generation/rootview_named_inputs.rs) |
| Structured primary, optional static inputs and dynamic named loader → primary or static output set | `execute_xml_with_sources_and_dynamic_source_loader`, `execute_xml_outputs_with_sources_and_dynamic_source_loader` | `ExecuteXmlWithSourcesAndDynamicSourceLoader`, `ExecuteXmlOutputsWithSourcesAndDynamicSourceLoader` | [xml_dynamic_inputs](../crates/cli/tests/code_generation/xml_dynamic_inputs.rs) |
| Structured primary → ordered primary document list | `execute_xml_documents` | `ExecuteXmlDocuments` | [xml_dynamic_outputs](../crates/cli/tests/code_generation/xml_dynamic_outputs.rs) |
| Structured primary plus static named inputs → primary document list | `execute_xml_documents_with_sources` | `ExecuteXmlDocumentsWithSources` | [xml_input_document_sets](../crates/cli/tests/code_generation/xml_input_document_sets.rs) |
| Structured primary, optional static inputs and dynamic named loader → primary document list | `execute_xml_documents_with_sources_and_dynamic_source_loader` | `ExecuteXmlDocumentsWithSourcesAndDynamicSourceLoader` | [xml_dynamic_input_document_sets](../crates/cli/tests/code_generation/xml_dynamic_input_document_sets.rs) |
| Structured primary → static primary plus named document lists | `execute_xml_document_outputs` | `ExecuteXmlDocumentOutputs` | [xml_mixed_document_outputs](../crates/cli/tests/code_generation/xml_mixed_document_outputs.rs), [xml_multiple_named_document_outputs](../crates/cli/tests/code_generation/xml_multiple_named_document_outputs.rs) |
| Structured primary plus static named inputs → static primary plus named document lists | `execute_xml_document_outputs_with_sources` | `ExecuteXmlDocumentOutputsWithSources` | [xml_input_document_outputs](../crates/cli/tests/code_generation/xml_input_document_outputs.rs) |
| Structured primary, optional static inputs and dynamic named loader → static primary plus named document lists | `execute_xml_document_outputs_with_sources_and_dynamic_source_loader` | `ExecuteXmlDocumentOutputsWithSourcesAndDynamicSourceLoader` | [xml_dynamic_input_document_outputs](../crates/cli/tests/code_generation/xml_dynamic_input_document_outputs.rs), [validation child](../crates/cli/tests/code_generation/xml_dynamic_input_document_outputs/validation.rs) |
| Structured primary, no named inputs → static primary, one static named document and one or more named document lists | `execute_xml_mixed_outputs` | `ExecuteXmlMixedOutputs` | [xml_mixed_named_outputs](../crates/cli/tests/code_generation/xml_mixed_named_outputs.rs) |

Context names follow three rules:

| Method family | Rust context spelling | C# context spelling |
| --- | --- | --- |
| No `with_sources` suffix | Append `_with_context` | Same method, context overload |
| Static `with_sources` suffix | Append `_and_context` | Same method, context overload |
| Dynamic loader suffix | Replace `_with_sources_and_dynamic_source_loader` with `_with_sources_context_and_dynamic_source_loader` | Replace `WithSourcesAndDynamicSourceLoader` with `WithSourcesContextAndDynamicSourceLoader` |

Context reads remain lazy. An overload's existence does not mean a runtime value
or parameter is evaluated. C# null argument/list/accessor guards are also distinct
from wrapped XML input and mapping failures.

### Input admission

`Structured` admits closed ordinary groups, repetition, attributes, scalar text
and String/Int/Float/Bool leaves, including admitted nillable scalars. Each named
input has its own embedded schema. Unsupported alternatives, generic/mixed
content, runtime-named input fields and other advanced shapes do not gain this
reader merely because their typed mapping lowers. See
[structured transport source tests](../crates/codegen/src/tests/structured_xml_transport.rs)
and [transport-emission tests](../crates/codegen/src/tests/structured_xml_transport_emission.rs).

`RootView` is a separate closed observed-primary profile. Its zero-named route
retains eight entry points: singular/output-set × text/bytes × context/no context.
Admitted static Structured named inputs add eight source-aware entry points,
giving sixteen signatures in the
[Rust](../crates/codegen-rust/src/xml_api/named_inputs.rs) and
[C#](../crates/codegen-csharp/src/mapping/xml_api/named_inputs.rs) emitter tests.
An observed primary can also use the separately proved flat static named-output
and combined static-input/static-output shapes. Dynamic inputs or document-list
outputs and observed named-input profiles remain outside RootView admission.
The [observed-input guide](code-generation.md#observed-root-view-input) records
the restrictions; the sixteen-signature count is not a count of all combinations.

Document-list output modes require the proved primary repeating-group driver
and closed member schema. Mixed static/list outputs currently require no named
inputs and exactly one static named declaration. The table does not imply that
all input and output forms compose freely. Invalid explicit policies return a
typed validation/emission failure; unsupported ordinary optional boundaries can
retain the typed/JSON core without XML methods.

## Phase order, resources and ownership

For source-aware Structured calls, the high-level order is:

```mermaid
flowchart TD
    A[Check initial input count] --> B[Check exact names and document shape]
    B --> C[Check each original document size]
    C --> D[Charge combined original UTF-8 input]
    D --> E[Parse primary then static declarations]
    E --> F[Execute the complete mapping]
    F --> G[Check output alignment and actual artifact count]
    G --> H[Serialize and charge primary then ordered outputs]
    H --> I[Return the complete result]
    F --> J[Reached dynamic scope reserves a request]
    J --> K[Host returns bytes; size and shared charge precede parsing]
    K --> F
```

The host owns the loader's I/O and any publication of returned documents.
Generated libraries do not open output paths or schema-hint locations. Logical
member paths remain opaque and ordered, including duplicate paths. A singular
static-output call still maps and serializes the complete set before selecting
primary. A failure returns no partial result; this is an API-result property,
not a claim about an external application's filesystem publication.

| Resource | Unit and maximum | Counting/priority rule | Source controls |
| --- | --- | --- | --- |
| Each input/output document | Original or serialized UTF-8 bytes; 64 MiB | Byte input size precedes decoding. Each static input size is checked before the combined sum. | [boundary tests](../crates/codegen-runtime/src/xml_boundary/tests.rs), [C# input/runtime tests](../runtime/csharp/Ferrule.Runtime.SmokeTests/XmlTests.cs) |
| Initial/dynamic input count | Documents; 4,096 | Includes primary and statics; each dynamic request reserves a slot before its callback. Repeated loads count again. | [input_set](../crates/codegen-runtime/src/xml_boundary/input_set.rs), [dynamic_inputs](../crates/codegen-runtime/src/xml_boundary/dynamic_inputs.rs) |
| Combined input | Original UTF-8 bytes; 256 MiB | Primary, static declaration order, then reached dynamic requests; independent of output. Returned dynamic bytes charge before parsing. | [input_set](../crates/codegen-runtime/src/xml_boundary/input_set.rs), [C# dynamic document tests](../runtime/csharp/Ferrule.Runtime.SmokeTests/XmlDynamicInputDocumentSetTests.cs) |
| Output count | Actual documents; 4,096 | Static sets include primary; primary lists can be empty; named lists include primary plus members; mixed outputs count primary + static named + all members. Check after mapping and before writers. | [output_set](../crates/codegen-runtime/src/xml_boundary/output_set.rs), [document_set](../crates/codegen-runtime/src/xml_boundary/document_set.rs), [mixed host count cases](../crates/cli/tests/code_generation/xml_mixed_named_outputs.rs) |
| Combined output | Serialized UTF-8 bytes; 256 MiB | Charge before retaining/converting the next document; excludes opaque paths. | [document_outputs](../crates/codegen-runtime/src/xml_boundary/document_outputs.rs), [C# document-output tests](../runtime/csharp/Ferrule.Runtime.SmokeTests/XmlDocumentOutputsTests.cs) |
| Embedded schema / literal hint descriptor | UTF-8 bytes; 8 MiB / 2 MiB | Separate setup limits; hint locations are metadata, not I/O instructions. | [Rust boundary](../crates/codegen-runtime/src/xml_boundary.rs), [C# output](../runtime/csharp/Ferrule.Runtime/FerruleXml.Output.cs) |
| Structured parser/projection | Separate depth, node, field/String-byte, namespace and work ledgers | Per document; not an aggregate Instance or RSS limit. Descriptor JSON depth is separate from logical schema depth. | [limit table](code-generation.md#output-and-limits), [C# structured reader](../runtime/csharp/Ferrule.Runtime/FerruleXml.Input.Structured.cs) |

MiB means 1,048,576 bytes. Text APIs count UTF-8; C# can reject invalid UTF-16
while measuring text. Input ledgers do not count decoded mapping allocations,
and output ledgers do not bound the typed outputs already constructed. These are
eager APIs, with no streaming or peak-memory guarantee.

### Error wrappers

The inner boundary distinguishes `Schema`, `DocumentLimit`, `Utf8`, `Input`,
`Mapping` and `Output`. Preserve the wrapper, phase owner, original boundary and
complete typed cause chain together; category text alone cannot establish parity.
Rust uses the standard `Error::source`; C# retains the boundary as
`InnerException`. Per-document `DocumentLimit` has document bytes/limit. Set
refusals instead retain a resource cause with `resource`, `observed_count` and
`limit`; the boundary's per-document byte fields remain unset.

| Route | Rust error / C# exception | Owner information |
| --- | --- | --- |
| Singular without sources | `XmlBoundaryError` / `FerruleXmlBoundaryException` | Original boundary; no source-aware wrapper |
| Static output set without sources | `XmlOutputSetError` / `FerruleXmlOutputSetException` | Primary or named output declaration for serializer failures |
| Static/dynamic source-aware primary or set | `XmlExecutionError` / `FerruleXmlExecutionException` | Exclusive input/output owner; optional authenticated dynamic request |
| Primary list | `XmlDocumentExecutionError` / `FerruleXmlDocumentExecutionException` | Final primary member index and path |
| Static sources → primary list | `XmlInputDocumentExecutionError` / `FerruleXmlInputDocumentExecutionException` | Input declaration or final primary member |
| Dynamic sources → primary list | `XmlDynamicInputDocumentExecutionError` / `FerruleXmlDynamicInputDocumentExecutionException` | Input/member plus optional authenticated request |
| Static primary + named lists | `XmlDocumentOutputsExecutionError` / `FerruleXmlDocumentOutputsExecutionException` | Primary or named declaration/final member/path |
| Static sources → primary + named lists | `XmlInputDocumentOutputsExecutionError` / `FerruleXmlInputDocumentOutputsExecutionException` | Input declaration or existing output owner |
| Dynamic sources → primary + named lists | `XmlDynamicInputDocumentOutputsExecutionError` / `FerruleXmlDynamicInputDocumentOutputsExecutionException` | Input/output owner plus optional authenticated request |
| Static primary + mixed static/list named outputs | `XmlMixedExecutionError` / `XmlMixedExecutionException` | Primary, static named declaration, or final named member/path |

Whole-set name errors and mapping failures have no guessed document owner.
Output alignment/count and document-list serializer schema setup are unowned.
Input descriptor errors in source-aware routes retain their input declaration.
A dynamic request is present only for an adapter's own recovered product-input
refusal: original declaration index, source, path, one-based reservation ordinal
and whether the host callback ran. A host error, similar text or externally
thrown XML exception cannot fabricate that channel. Recovery requires the
original Rust marker allocation or C# private exception identity; the adapter
is terminal after a failed load.

## Written controls and retained execution

The linked source families contain real generated Rust/C# hosts, distinct text
and byte calls, context variants, manual physical/typed output comparisons, and
original wrapper/cause assertions. They also contain generation/admission tests
and direct budget tests. Those levels must be recorded separately: calling a
budget with a supplied size does not exercise a generated parser or writer with
a document of that size.

The following previously retained finite cohorts can be reused after checking
their exact source/project/tool bindings. This index does not rerun them or
claim that they qualify every route in the table on a new source revision.

| Retained cohort | Bounded result scope | Exclusions |
| --- | --- | --- |
| Fourteen strict-imported item-error projects | 56 XML calls per backend: text/bytes × context/no context; 140 total typed/JSON/XML calls per backend; four complete XML writer oracles | No named-input loader, list-output or combined-resource coverage is inferred. Native error/publication categories remain separate. |
| Seven original signed-integer/global-rule projects, with explicit XML-adapter clones | 64 XML calls per backend; 156 total calls per backend across original and XML profiles; three XML and three JSON writer oracles | Original default format and explicit XML adapters are distinct. This does not establish named-input/list resource boundaries. |
| Nine saved ordinary designs replayed locally | Three full successful typed/XML-content/XSD checks and six exact global-rule messages | This is import/local replay evidence, not generated-host XML evidence. Root schema annotations, numeric categories and failed native publication remain explicit differences. |

An XML expression such as `Node::XmlSerialize`, a typed host whose result is later
written by an external XML writer, or a JSON host with an XML-shaped schema is
not a physical XML-input API call. The
[XML expression tests](../crates/codegen-rust/src/tests/xml_serialize.rs) and
[C# expression tests](../crates/codegen-csharp/tests/xml_serialize_dotnet.rs)
remain useful evidence for their own layer.

## Concrete remaining verification cells

These unchecked items are source-audit proposals, not observed product defects.
The existing small cases and direct counters should be reused; no admission
change is proposed. Large controls need serial execution, bounded retained
artifacts and measured build/disk headroom.

- [ ] **[Combined input bytes through compiled public APIs](https://github.com/DeandreT/ferrule/issues/116).** Exercise exactly
  256 MiB and one byte over, with every individual document within 64 MiB, text
  and byte calls, and reverse supplied names. Add malformed primary and a later
  oversized document as competing controls. Assert exact crossing declaration,
  resource units, original cause, no mapping/loader call and no returned result.
  The current `full_size_preflight_refuses_a_later_document_before_aggregate_overflow`
  test supplies numeric sizes to the helper; it does not call these generated APIs.
- [ ] **[Combined output bytes through compiled document/list APIs](https://github.com/DeandreT/ferrule/issues/117).** Cross the
  256 MiB limit using documents individually within 64 MiB. Check exact final
  member/declaration owner, opaque duplicate path retention, resource cause and
  no returned partial envelope for text and byte outputs. Current direct budget
  tests charge numbers; the inspected compiled host fixtures do not perform this
  combined-output boundary case.
- [ ] **[Dynamic request count through compiled loader APIs](https://github.com/DeandreT/ferrule/issues/118).** Reach the exact
  4,096-document total and then a refused 4,097th reservation, including primary
  and statics. Check the whole-execution ordinal, original declaration index and
  `callback_invoked=false` on refusal, with no extra host callback. Direct adapter
  tests currently prime counters; the compiled loader fixtures use small requests.
- [ ] **[Late graph mapping exception versus final output-count refusal](https://github.com/DeandreT/ferrule/issues/119).** Combine a later
  selected lazy graph Raise with an otherwise over-limit document output count,
  with global failure rules absent. The complete MappingException should retain
  its exact graph node and optional message before any output count or writer. Existing mixed-output `count-before-primary` covers
  count versus a primary writer, and existing late-error cases cover mapping
  versus writers; this combined priority cell remains to be added.

For each completed cell, retain the exact project/input, selected generated
sources, compiler result, full raw host result before assertions, complete output
or typed error/cause, and after-execution source guards. A skipped, empty or
unreached selection is not completion. Keep error-priority and owner assertions
independent of expected error-message strings, and keep JSON, typed and XML
entry points as separate rows in the result record.
