# Borrowed ordinary JSON output: 2026-10-09

All eight candidate runs used less GNU-time process peak memory than their
fresh original-writer counterparts: **8.7–40.4 MiB (6.96–24.65%) less** for these
fixtures. Sixteen separate verifiers passed all twenty-four complete published
output documents. This is a workload-specific comparison of two ordinary debug
builds, not a universal RAM bound or an allocation-level measurement.
Scope: [#184](https://github.com/DeandreT/ferrule/issues/184); the dated
[#107 baseline report](json-output-set-memory.md) retains the earlier results.

## Change and remaining allocations

The ordinary [`to_string` route](../../crates/format-json/src/lib.rs) now prepares
an ordered [private output view](../../crates/format-json/src/borrowed_writer.rs)
whose ordinary string values and object member names borrow the materialized
target. It serializes that view into the same two-space pretty String with final
LF. It no longer creates a complete owned `serde_json::Value` tree solely to
render ordinary target text.

This view still allocates container vectors and ordering metadata. Numeric
coercion and arbitrary-JSON properties can retain owned values. Scalar assertions
can create temporary text values; alternatives, dependencies, `contains` and
uniqueness checks can create owned validation snapshots. Existing validators
and their ordering remain in use. These temporary allocations are released
before final text rendering. The returned complete String remains owned.

`to_value`, JSON5 and JSON Lines retain their previous routes. Input parsing,
engine target ownership, publication and allocator policy are unchanged by this
increment. The filesystem writer finishes validation and rendering before
writing the complete String; CLI output staging remains serial.

```mermaid
flowchart LR
  A[Complete mapped target trees] --> B[Ordered view with borrowed strings and names]
  B --> C[Existing validation; temporary owned snapshots where needed]
  C --> D[Complete pretty output String]
  D --> E[Stage file; then next selected output]
  E --> F[Publish outputs]
```

## Workload, storage and build identities

The [existing fixture preparer](../../scripts/performance/json_output_set_memory.py)
and [example host](../../crates/cli/examples/json_output_set_memory.rs) were not
changed. Each Project copies ordered `Rows` containing `Id:Int` then
`Text:String`. Selected-primary runs publish one document; all-target runs
publish Primary then the named `mirror`. Document `records_written` is 1.

| Shape | Rows | Text bytes per row | Input bytes | Each output bytes |
| --- | ---: | ---: | ---: | ---: |
| Many small rows | 32,768 | 1,024 | 34,296,997 | 35,148,973 |
| Single wide value | 1 | 33,554,432 | 33,554,462 | 33,554,496 |

All input/oracle bytes, UTF-8/LF conventions and 3,399-byte Project bodies are
identical to #107. Complete SHA-256 identities:

| Fixture | SHA-256 |
| --- | --- |
| Project | `19050a3cf66bfcad8c56d72d9236d71f32769406a9a765d5e0e4b7f170a323f6` |
| Many input | `972c7bb93619653e698f95fd9c3bc103a2f840ffcc26d2c1c74b347e5f6de66b` |
| Many expected / each published output | `27303a4d26ce742df757e8e5bf76b7efe7ee938bb61c0fa01e8ee949f099793c` |
| Wide input | `b9e29419ced6496de1b9f8236988eadf2e7a0b82752e499d341a3c885e8a726f` |
| Wide expected / each published output | `2cee8faec100279f7fb386bdbdd659f64eacb863acfe4a66468364000f398dd3` |

The original compiled #107 executable was retained with a Btrfs reflink before
rebuilding. Its complete bytes match the original baseline artifact. Output and
large verification originals moved to a spare local drive to preserve build
headroom. To account for that storage change, the retained original executable
was measured again in eight fresh trials on that drive, followed by eight
separate verifiers. The candidate then used fresh directories on the same drive.
The historical eight peaks remain visible below; they were not replaced.

| Identity | Original executable | Candidate executable |
| --- | --- | --- |
| Source base | `6e7963cae9d74e6c96c5b02125bfa56bb11e167d` | `7073ddc7ef9e3802916f64f37a624e405c5a016b` plus the three writer files |
| Executable bytes | 378,461,056 | 380,397,056 |
| Executable SHA-256 | `54ababde82c1f16b7dedc6953b0cfca93afb11955e2a0c3c7627485634a3aeb7` | `69a65fc57967d1ed2932ca9eb1c45f3709d1ca567fcae895e19e560633d2dbf2` |

Both builds use ordinary workspace dev, unoptimized with debuginfo, jobs 1 and
incremental compilation disabled. The candidate was built with
`CARGO_INCREMENTAL=0 cargo build --offline --locked -j1 -p cli --example json_output_set_memory`,
without an optimization override, reusing the shared target.
Compiler: Rust 1.100.0-nightly (`5a2be9f5f075d31e3ca5526b5b029881ce441253`),
LLVM 23.1.1, x86_64 Linux; compiler executable SHA-256
`4602f29c6b0157b35b198248e361ce0d0c49c27b66ff15770f3a2c29e5415adf`.
GNU time 1.10 executable SHA-256
`ccc576561484fe2c3f83bbceabfeba78408e59ac9c9a8161ad2a017b1037663d`.
Cargo.lock SHA-256 is
`50584279f72d209089cf6327557323ffdc1656647a8535c3636e79616bcfffc2`.

The benchmark/fixture source hashes remain
`5a9eba86dd4c7aca1c11d7ace7565f6e20e7397ab455f0752a3e5da2df4c5574` /
`49387007d6f586bd63bc62883bda58d006d9dbd4da7c471380834caa90087824`.
Candidate qualification retains the complete 2,074-body source inventory,
canonical sorted compact JSON SHA-256
`f1032fe35dbdae520182784fe45b67e8b0a2fa6a59c73142ec020578a174bdc0`.
That inventory precedes this report file. Original executable authority remains
its historical #107 build, rather than a claim that the current checkout built it.

## Every process peak

GNU `time -v` records the measured child's genuine `wait4` maximum RSS in KiB;
parentheses show MiB. Compilation, fixture preparation and verifier processes
are excluded. Delta is candidate minus the fresh original on the same drive.

| # | Shape | Repeat | Outputs | Historical #107 peak KiB | Fresh original KiB (MiB) | Borrowed KiB (MiB) | Change KiB (MiB; percent) |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | Many | 1 | 1 | 158,828 | 164,648 (160.8) | 144,848 (141.5) | -19,800 (-19.3; -12.03%) |
| 2 | Many | 1 | 2 | 198,768 | 206,464 (201.6) | 165,116 (161.2) | -41,348 (-40.4; -20.03%) |
| 3 | Wide | 1 | 1 | 123,212 | 129,096 (126.1) | 119,624 (116.8) | -9,472 (-9.2; -7.34%) |
| 4 | Wide | 1 | 2 | 156,072 | 161,940 (158.1) | 122,024 (119.2) | -39,916 (-39.0; -24.65%) |
| 5 | Many | 2 | 2 | 199,304 | 205,704 (200.9) | 164,840 (161.0) | -40,864 (-39.9; -19.87%) |
| 6 | Many | 2 | 1 | 158,980 | 164,348 (160.5) | 145,240 (141.8) | -19,108 (-18.7; -11.63%) |
| 7 | Wide | 2 | 2 | 155,700 | 161,620 (157.8) | 121,848 (119.0) | -39,772 (-38.8; -24.61%) |
| 8 | Wide | 2 | 1 | 123,708 | 128,536 (125.5) | 119,588 (116.8) | -8,948 (-8.7; -6.96%) |

The additional-output peak difference was 40.4–40.8 MiB for many rows and
32.1–32.3 MiB for a wide value in the fresh original cohort. It was 19.1–19.8 MiB
and 2.2–2.3 MiB, respectively, in the candidate cohort. All eight whole-process
peak comparisons decreased; this small campaign contains no observed peak
regression. It does not establish statistical significance or an exclusive
writer-allocation saving.

## Every boundary observation and elapsed time

Each boundary is **current VmRSS / reported VmHWM**, MiB. B0 is immediately
before the public call; B1 is evaluation complete with parsed input and selected
targets still live, before staging; B2 is return after publication. CPU is user
plus system time; wall is GNU time elapsed. Retained originals contain all
process-status and process-stat fields, not just these readings.

| # | Executable | B0 RSS / HWM | B1 RSS / HWM | B2 RSS / HWM | CPU / wall seconds | Measure / verifier exit |
| ---: | --- | --- | --- | --- | --- | --- |
| 1 | Original | 13.1 / 13.1 | 127.7 / 147.8 | 129.0 / 160.8 | 1.62 / 1.63 | 0 / 0 |
| 1 | Borrowed | 10.2 / 10.2 | 120.8 / 141.5 | 120.8 / 141.5 | 1.56 / 1.58 | 0 / 0 |
| 2 | Original | 12.8 / 12.8 | 168.1 / 168.1 | 169.3 / 201.6 | 3.97 / 4.71 | 0 / 0 |
| 2 | Borrowed | 10.3 / 10.3 | 161.5 / 161.5 | 161.6 / 161.6 | 2.85 / 2.89 | 0 / 0 |
| 3 | Original | 12.8 / 12.8 | 93.0 / 122.5 | 31.3 / 126.1 | 1.08 / 1.10 | 0 / 0 |
| 3 | Borrowed | 10.2 / 10.2 | 87.3 / 116.8 | 24.2 / 116.8 | 1.05 / 1.06 | 0 / 0 |
| 4 | Original | 12.4 / 12.4 | 125.1 / 125.1 | 31.6 / 158.1 | 1.97 / 1.98 | 0 / 0 |
| 4 | Borrowed | 10.2 / 10.2 | 118.9 / 118.9 | 23.8 / 119.2 | 1.95 / 1.96 | 0 / 0 |
| 5 | Original | 12.2 / 12.2 | 167.5 / 167.5 | 168.7 / 200.9 | 2.97 / 3.00 | 0 / 0 |
| 5 | Borrowed | 10.2 / 10.2 | 161.2 / 161.2 | 161.1 / 161.1 | 2.81 / 2.83 | 0 / 0 |
| 6 | Original | 12.5 / 12.5 | 127.5 / 147.6 | 128.5 / 160.5 | 1.70 / 1.73 | 0 / 0 |
| 6 | Borrowed | 10.2 / 10.2 | 121.5 / 141.8 | 121.5 / 141.8 | 1.56 / 1.57 | 0 / 0 |
| 7 | Original | 12.7 / 12.7 | 124.7 / 124.7 | 31.3 / 157.8 | 2.42 / 2.47 | 0 / 0 |
| 7 | Borrowed | 10.2 / 10.2 | 118.7 / 118.7 | 23.6 / 119.0 | 1.95 / 1.97 | 0 / 0 |
| 8 | Original | 12.6 / 12.6 | 92.6 / 122.0 | 30.9 / 125.5 | 1.58 / 2.07 | 0 / 0 |
| 8 | Borrowed | 10.3 / 10.3 | 87.4 / 116.8 | 24.6 / 116.8 | 1.05 / 1.06 | 0 / 0 |

The candidate's second and fifth trials report `wait4` peaks of 165,116 and
164,840 KiB, while their `/proc` B2 VmHWM readings are 165,508 and 164,968 KiB.
Trial five also reports B1 VmHWM 165,036 KiB, above its B2 reading. All original
readings are retained; these are distinct kernel accounting observations and
have not been forced to agree or to increase monotonically. Sparse boundaries
cannot locate the exact peak or identify an exclusive parser, mapper or writer
allocation.

## Correctness and qualification

A manually authored catalog covers 62 positive/refusal cases. Its inputs and
expected bytes or typed Debug errors were frozen before production adoption.
Tests preserve complete actual values, errors, input trees and ordered group
origins before assertions. The original and candidate each completed 126 public
calls: `to_string` and `to_value` for all 62 cases, plus two filesystem rejection
checks. Their complete 329,805-byte evidence sections are byte-identical.
Coverage includes nested/root rows, coercion, unions, dynamic/duplicate members,
null omission, constraints, error precedence, Unicode, non-finite values and
validation-before-publication.

Preparation failures were preserved and corrected separately: an intentionally
invalid required-field schema initially stopped before any writer call;
constructor constraints then stopped two and one schema preparations after
122 and 124 reached public calls. The two singleton-enum test schemas gained a
second distinct value; a fixed-plus-enum precedence control uses explicit test
construction because schema deserialization intentionally rejects that metadata
combination. All original inputs and expected outcomes remain unchanged. The
original and corrected metadata catalogs are retained separately. These failed
attempts are not counted as passing writer baselines.

Two direct unit tests verify pointer identity for borrowed ordinary 65,536-byte
text/member names and constrained text after validation. The complete JSON
unit/integration suite passed **396 tests**, with one existing ignored test.
Two existing CLI publication tests also passed. These totals include the
borrowing tests and the new catalog test; repeated focused runs are not added.
Workspace all-target/all-feature warnings passed, including the benchmark.

Both cohorts completed eight measured runs and eight separate verifiers.
Every verifier checked complete typed parser/engine outcomes, ordered group
origins, ordered public RunOutcome fields, and every byte/hash/EOF of its one or
two published outputs. All twenty-four documents match the independent oracles.
Passive analysis retains all rows, including any errors; there were none. The
unchanged numeric parsing functions passed all 45 original accept/refuse
controls for each namespace-only analysis adapter (90 calls total).

All thirty-two measurement/verification invocations closed normally, with direct
child reaping, empty final census, lease release, no timeout/signal/cleanup,
no primary/lifecycle/custody errors, unchanged source/survey and all seven
input/tool body-and-stat guards. Build and correctness gates retain their own
complete closure and source records. Exit zero alone was not treated as proof.

- [x] Preserve the original executable, historical peaks and independent fixtures.
- [x] Retain all sixteen fresh time results and forty-eight boundary observations.
- [x] Complete sixteen separate verifiers and twenty-four full-document comparisons.
- [x] Preserve all writer outcomes, source/origin evidence and preparation failures.
- [x] Demonstrate actual borrowing while preserving existing APIs and publication.
- [x] Complete independent actual-evidence/report review before merging.

## Disk use, reproduction and limits

The thirty-two gate readings retained at least 22,519,635,968 free bytes on the
build volume, above the 20 GiB reserve; readings ranged to 22,815,703,040 bytes.
At this report's draft inventory, the new spare-volume campaign and analysis
originals contain 412 files, 3,976,152,216 logical bytes and
3,977,228,288 allocated bytes by summed `st_blocks`. These are disk readings,
not process RSS or unique physical-storage attribution. The shared build target
was reused; no additional Cargo target cache was created for this campaign.

To reproduce, retain the original executable before rebuilding, use the existing
fixture preparer with fresh absolute directories, and preserve every complete
trial including failures. Measure the retained executable and candidate with the
same fixtures, profile, storage and eight-trial order. Run GNU `time -v` around
`run_argv` only, then run `verify_argv` separately. Preserve binary/source/tool
identities, all boundary/time originals, complete verifier outputs and disk
headroom before comparing results.

The second repetition reverses one/two-output order. The complete original
cohort preceded the complete candidate cohort; no alternating cohort order,
forced cache flush, allocator change or host isolation was introduced.
Preparation and verification can warm caches. Startup RSS differed by roughly
2–3 MiB between executables. They have different source bases and executable
layouts in addition to the writer change, so whole-process deltas cannot be
assigned exclusively to borrowed text. Ordinary host activity can also affect
elapsed time and page accounting.

The engine still materializes its chosen output trees; input parsing still
reads whole documents. Arbitrary JSON and validation snapshots can allocate
owned trees; every output still builds a complete pretty String. This increment
does not provide streaming, a hard host-RAM/time ceiling, generated-backend
memory evidence or full parity. Massive inputs require separate bounded-format
and end-to-end resource work, coordinated through the remaining roadmap issues.
