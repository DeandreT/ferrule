# Pipeline Intermediate Release

File-host pipeline runs drop each host input and unselected stage target after
the last stage that reads it. In a linear chain of whole-document copies, peak
live heap therefore stays at about two documents regardless of chain length,
while retaining every intermediate grows by one document per stage.

## Measurement

`crates/engine/tests/pipeline_intermediate_release.rs` installs a counting
global allocator that tracks live bytes on the measuring thread and records the
peak relative to the start of each run. Each run builds its host input inside
the measured region and executes a chain of `CopyCurrentSource` stages over one
batch of 1,024 rows, each holding 1 KiB of text. Only the last stage target is
retained. The baseline is `engine::run_pipeline_with_stage_contexts`, which
keeps every value; the candidate is `engine::run_pipeline_retaining`. Both runs
must return the same final value.

```sh
cargo test -p engine --test pipeline_intermediate_release -- --nocapture
```

Allocation sizes depend on the standard library and allocator layouts, so the
exact byte counts can drift slightly between toolchains. One batch value occupies
1,143,739 live bytes in this observation.

| Stages | Retain-all peak (bytes) | Releasing peak (bytes) | Releasing / retain-all |
| ---: | ---: | ---: | ---: |
| 1 | 2,288,671 | 2,289,250 | 1.000 |
| 2 | 3,432,187 | 2,290,032 | 0.667 |
| 4 | 5,718,209 | 2,290,432 | 0.401 |
| 8 | 10,290,322 | 2,291,232 | 0.223 |

The test asserts the shape rather than exact numbers: the eight-stage
retain-all peak exceeds the one-stage peak by at least six documents, the
eight-stage releasing peak stays within half a document of the one-stage peak,
and it is less than a third of the eight-stage retain-all peak.

## Scope

Execution remains sequential in the existing stable dependency order. The
analysis only changes when values are dropped or moved into a stage's final
named-source binding; the interpreter and dynamic-source loaders are unchanged.
A named-source binding still copies its value unless it is that value's final
read and the same stage does not also read the value as its primary source. Preview keeps every stage value because it renders them all.
The measurement covers the interpreter's live heap, not process RSS, and does
not include parsing or output serialization.
