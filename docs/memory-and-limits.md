# Memory Use and Boundary Limits

Ferrule currently maps materialized instance trees. Byte and item limits protect
specific boundaries; they are not a limit on the process's total memory.
Choose the execution route before planning a large-file workload, because local
filesystem readers, payload APIs, and generated host adapters have different
contracts.

## Where memory remains live

```mermaid
flowchart LR
    B[File reader or host-owned bytes] --> I[Parsed input instances]
    I --> E[Scope and expression evaluation]
    E --> T[Mapped target instances]
    T --> S[Serialized output buffers]
    S --> F[Files or returned host buffers]
    H[Host-owned named documents] --> E
```

Input instances can remain live while target rows and output buffers are built.
The filesystem CLI releases its primary input and dynamic-source cache after
evaluation and the before-publication check, before serializing selected or all
targets. Target instances own their data. Earlier parsing and mapping peaks,
allocator-retained pages, and the target/output buffers can still dominate RSS.
Joins, sorting, grouping, generated sequences, copies, multiple targets, and
retained preview results may require additional collections or clones. A
file-host pipeline run drops each host input and unselected stage target after
its last reading stage, so a linear chain holds about one input and one output
at a time. A small serialized file can still create many owned strings and tree
nodes. One very wide row has a different allocation pattern from many short rows.
Grouping owns each group's member collection but borrows intermediate ancestors
crossed by a multi-hop source path.

Ordinary CSV filesystem reading uses a buffered CSV reader but retains every
parsed row. CSV output validates the complete rows before encoding and releases
each row's temporary formatted fields; it retains the mapped rows and the full
serialized `Vec`/`String`. C# CSV serialization likewise avoids a retained
all-row formatted cache. These changes do not make mapping or output streaming.
Text and byte APIs also have unavoidable owned result boundaries and may copy
or encode at those boundaries.

## Limits are route-specific

MiB means 1,048,576 bytes. The following are selected current contracts, not a
complete resource-safety claim. Follow the linked document for eligibility,
error ordering, and additional schema/format limits.

| Boundary | Current limit or behavior |
| --- | --- |
| New and versioned project-file persistence | 64 MiB, 127 JSON containers; legacy unmarked decoding keeps its documented compatibility behavior |
| CLI payload documents | 64 MiB per document; 256 MiB combined input budget and a separate 256 MiB output budget per run |
| CLI payload artifacts and identities | 4,096 output artifacts; 4,096 UTF-8 bytes per logical path; 256 bytes per source name |
| Generated schema-shaped JSON | 64 MiB per document; 1 MiB embedded JSON schema; generated dynamic JSON loaders also share a 256 MiB execution byte budget |
| Generated raw XML codecs and serialized XML output | 64 MiB per serialized document; 8 MiB embedded XML schema; input/output-set APIs have separate aggregate and artifact-count checks |
| XSD import | 64 MiB per schema document; 4,096 materialized elements; 256 nesting and reference levels open at once ([formats](formats.md)), refused with a typed error before stack exhaustion |
| Generated flat CSV output | 64 MiB encoded UTF-8 output, including headers, quoting, line endings, and optional BOM |
| Path-bearing CSV document output | 4,096 documents; 4,096 UTF-8 bytes per path; 64 MiB per document and 256 MiB across the output list, before return or filesystem publication |
| Ordinary filesystem CSV | No general 64 MiB input/output cap; parsed rows and complete serialized output remain materialized |
| Ordinary JSON and JSON Lines files | Whole text is read; no general 64 MiB file cap at these ordinary readers |
| Local JSON5 input | File and text readers cap original UTF-8 bytes at 64 MiB, including a leading BOM; 128-container input bound; distinct from strict JSON and generated JSON APIs |
| Typed dynamic-source host loader | At most 1,000,000 loads per dynamic source and 4,096-byte logical paths; the host owns document acquisition and its byte budgets |
| Literal, fixed-length, regex, and integer-range generated sequences | At most 1,000,000 generated items; regex has additional pattern and compiled-program bounds |
| PDF extraction input | 8 MiB before reading; event/node/value/depth caps apply later, and do not isolate upstream decompression or recursive content traversal |

These per-feature bounds are not universal execution limits. A host that runs
mappings it does not trust can opt into a
[restricted execution profile](design/execution-profiles.md). That profile adds
deny-by-default filesystem roots and HTTP origins, plus explicit work,
evaluation-depth and join-row budgets. It enforces a policy inside the process
and is not a sandbox.

The [PDF worker contract](pdf-worker-contract.md) is a proposed filesystem host
route for #104. Its Linux child, transport limits, deadline, cancellation and
owned cleanup require implementation and qualification. Current PDF library and
payload calls remain in-process; no worker or RSS ceiling is claimed here.

See [Project files](project-files.md), [Supported formats](formats.md),
[Code generation](code-generation.md), and the
[payload API](../README.md#quick-start) for the full boundary contracts.
Relevant implementations include
[payload budgets](../crates/cli/src/payload.rs),
[ordinary CSV encoding](../crates/format-csv/src/lib.rs),
[bounded CSV encoding](../crates/format-csv/src/bounded.rs), and
[generated typed/JSON loaders](../crates/codegen-runtime/src/dynamic_source.rs).

The generated CSV text/byte companions call the existing ordinary typed
mapping once, then serialize its flat primary result. Supplying a typed dynamic
loader does not acquire the JSON loader's input byte limits. Named input checks
and mapping failures still happen before CSV serialization. Ordinary filesystem
CSV remains uncapped; choosing stdin/stdout transport also does not turn the
interpreter into a row-at-a-time engine.

Typed `Instance`/`FerruleInstance` inputs do not inherit a serialized-document
byte cap. In particular, legacy singular XML wrappers take typed inputs and
render XML output; their names do not imply bounded XML input decoding.

A serialized-byte limit may be checked after some parsing, formatting, or
mapping allocations. Buffer capacity can exceed logical length. Multiple live
buffers, runtime overhead, JIT/GC, allocator behavior, and host-owned inputs
remain outside an individual output-byte limit.

<a id="compiled-xml-host-observations"></a>

## XML boundary fixtures

- [Combined static input bytes](../crates/cli/tests/code_generation/xml_input_sets/combined_bytes.rs)
- [Combined primary-list output bytes](../crates/cli/tests/code_generation/xml_dynamic_outputs/combined_output_bytes.rs)
- [Dynamic input request count](../crates/cli/tests/code_generation/xml_dynamic_inputs/request_count.rs)
- [Late graph exception versus mixed-output count](../crates/cli/tests/code_generation/xml_mixed_named_outputs/late_mapping_count.rs)

Byte limits and document-count limits measure different resources. Neither limits whole-process memory.

## Plan and measure a large-file run

- [ ] Record the route, backend, build profile, input format, row count,
  field width, Unicode/quoting, and selected targets.
- [ ] Check the limits for that route and the largest single row/document;
  do not substitute a payload limit for an ordinary filesystem contract.
- [ ] Start with a representative small case and compare full values/bytes,
  including late validation failures and destination preservation.
- [ ] Measure many short rows and one wide row separately. Keep inputs and
  output publication policy identical across comparisons.
- [ ] Distinguish sampled RSS, sampled high-water values, and a process-lifetime
  peak; a point sample is not an API-call peak. Keep whole-process CPU and wall
  time separate from call-bracket timing.
- [ ] Retain individual trials and repeats, including regressions. State
  debug/release, warm/cold, setup/publication, and measurement-phase limits.
- [ ] Check disk space for inputs, artifacts, compilation, and retained results.

Whole-process peaks for bounded filter/map workloads do not attribute individual
allocations or bound RAM for arbitrary large files.

This page describes source contracts and measurement practice. It supplies no
universal improvement claim or total-RAM cap.

Whole-process memory includes executable verification as well as parsing.
