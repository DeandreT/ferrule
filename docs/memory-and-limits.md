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
retained preview or pipeline results may require additional collections or
clones. A small serialized file can still create many owned strings and tree
nodes. One very wide row has a different allocation pattern from many short rows.

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
| Generated flat CSV output | 64 MiB encoded UTF-8 output, including headers, quoting, line endings, and optional BOM |
| Ordinary filesystem CSV | No general 64 MiB input/output cap; parsed rows and complete serialized output remain materialized |
| Ordinary JSON and JSON Lines files | Whole text is read; no general 64 MiB file cap at these ordinary readers |
| Local JSON5 input | File and text readers cap original UTF-8 bytes at 64 MiB, including a leading BOM; 128-container input bound; distinct from strict JSON and generated JSON APIs |
| Typed dynamic-source host loader | At most 1,000,000 loads per dynamic source and 4,096-byte logical paths; the host owns document acquisition and its byte budgets |
| Literal, fixed-length, regex, and integer-range generated sequences | At most 1,000,000 generated items; regex has additional pattern and compiled-program bounds |
| PDF extraction input | 8 MiB before reading; event/node/value/depth caps apply later, and do not isolate upstream decompression or recursive content traversal |

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

## Compiled XML host observations

The [compiled XML resource checks](generated-xml-qualification-index.md#compiled-resource-qualification)
qualified locally by **2026-10-08** also retained GNU `time` maximum resident set
size for each physical boundary call. These are **whole fresh host-process**
observations: they include startup, input acquisition, the public API call,
result checks and retained-output writes. Where a host runs a writer prototype,
that work is also inside the bracket. Rust hosts use a debug build; C# hosts use
a Release build. The physical fixtures use valid ASCII
XML, apart from the deliberate malformed-primary priority control.

The ranges below are the observed minimum and maximum across the listed
physical calls for each backend, in **KiB (1,024 bytes)**. Ordinary small tests
and the boundary tests' own small prerequisites are excluded from these ranges.

| Finite boundary cohort | Physical observations per backend | Rust whole-host maximum RSS, KiB | C# whole-host maximum RSS, KiB |
| --- | ---: | ---: | ---: |
| [Combined static input bytes](../crates/cli/tests/code_generation/xml_input_sets/combined_bytes.rs) | 40 | 266,888–531,168 | 302,944–1,482,576 |
| [Combined primary-list output bytes](../crates/cli/tests/code_generation/xml_dynamic_outputs/combined_output_bytes.rs) | 8 | 467,912–468,252 | 1,967,360–2,049,680 |
| [Dynamic input request count](../crates/cli/tests/code_generation/xml_dynamic_inputs/request_count.rs) | 8 | 16,512–56,996 | 100,632–113,328 |
| [Late graph exception versus mixed-output count](../crates/cli/tests/code_generation/xml_mixed_named_outputs/late_mapping_count.rs) | 12 | 10,696–77,936 | 62,144–126,312 |

The first two cohorts physically reach the 256-MiB input or serialized-output
boundary and cross it by one byte. The count cohorts instead reach a
4,096-document/artifact limit; they do not contain 256 MiB of input. For the
mixed-output count cohort, the three physical inputs are 170,856, 170,898 and
170,897 bytes, and the largest returned document is 92 bytes. The full successful
output still contains 4,096 artifacts. Small documents can therefore exercise a
count limit without exercising a byte limit.

Each case/route was observed once in this finite cohort. The ranges are not
repeated independent performance trials, an API-only allocation peak, a general
RSS ceiling, a streaming guarantee or qualification of all XML routes. Different
host startup, Unicode/shape, typed allocations, profiles or measurement brackets
can change memory use. The separate 256-MiB input and output ledgers do not bound
one another, decoded instance trees or process memory.

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

This page describes source contracts and measurement practice. It supplies no
universal improvement claim or total-RAM cap. The separate
[filesystem input-lifetime measurement](performance/filesystem-input-lifetime-2026-10-07.md)
records eight normal-debug observations, exact output checks, and the workloads
where early input release did and did not reduce the observed process peak.
