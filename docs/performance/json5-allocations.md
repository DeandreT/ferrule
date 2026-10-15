# Generated C# JSON5 allocation attribution — 2026-10-15

For [#160](https://github.com/DeandreT/ferrule/issues/160), all 96 physical C#
calls passed: 48 complete returned values and 48 expected typed resource refusals.
The 64 MiB output-success recipe consistently attributed about 704 MiB of sampled
allocation accounting to serialization, including large allocations beneath the
canonicalizer and the final newline append. Mapping, normalization, projection,
parsing and host handling also allocate substantially. This identifies a bounded
optimization opportunity; it does not measure an API-only peak or exact retained
heap, and this change implements no optimization.

The [fixture and protocol](../../examples/performance/json5-allocations/README.md),
[96 individual observations](json5-allocation-observations-2026-10-15.csv), and
[source/artifact identities and qualification summary](json5-allocation-evidence-2026-10-15.json)
are part of this report. Every observation includes repetition, exact public
route, mode, host peak RSS, elapsed time, sampled categories, post-GC observations,
trace hash and successful output hash. Blank profiling fields belong to controls;
blank post-GC fields mean no corresponding event was observed.

## Bound workload and build

The source base was `7ff83194882f49285b47d14bdc44464d44bf8dbe`. Production
runtime/emitter sources were unchanged. The new Rust example imports the original
[`corpus.rs`](../../crates/cli/tests/json5_resources/corpus.rs) and copies the
original [C# resource host](../../crates/cli/tests/fixtures/json5_resources_csharp.cs.txt)
without edits. It emits the same four Programs and preserves the host's input,
public call, full returned-value retention, original exception chain, literal
checks and final input/case guards. The small prerequisite cohort remains its
original Debug build; all four measured C# hosts use the original Release profile.

Let `M = 67,108,864`, `Q = 11,184,809`, `R = 2`, `S = 33,554,425`.

| Case | Original input bytes | Normalized bytes by literal recipe | Expected public result |
|---|---:|---:|---|
| original-exact | M | 7 | Complete 13-byte strict JSON |
| original-plus1 | M + 1 | 7 if admitted | Encoding / OriginalDocumentBytes refusal |
| normalized-exact | Q + R + 6 = 11,184,817 | M | Complete 13-byte strict JSON |
| normalized-plus1 | Q + R + 7 = 11,184,818 | M + 1 | Syntax / NormalizedDocumentBytes, offset 11,184,818 |
| output-exact | S + 6 = 33,554,431 | S + 8 = 33,554,433 | Complete M-byte strict JSON |
| output-plus1 | S + 6 = 33,554,431 | S + 8 = 33,554,433 | Output / OutputDocumentBytes refusal requesting M + 1 |

The original recipes are a comment-padded integer, a string containing `Q` literal
NUL bytes plus two/three ASCII `a` characters, and an `S`-character ASCII `x`
string mapped to two copies, optionally followed by `a`. The normalized-string
Program still validates/projects its complete input before returning constant 7.
No intermediate normalized buffer length is exposed by these public APIs; the
table labels literal dimensions rather than an extra private measurement.

Each case used `ExecuteJson5(string)`, `ExecuteJson5(string, context)`,
`ExecuteJson5Bytes(byte[])`, and `ExecuteJson5Bytes(byte[], context)`, with
`/logical/resource145.json` as the explicit context. Two repetitions paired each
route with an uninstrumented control and a separately traced process. The second
repetition reversed case/route order, and paired mode order alternated. Every call
started a new CLR; filesystem caches were not flushed. Full hash guards ran
outside measurements before calls.

The machine was Linux x86-64, kernel `7.2.7-arch1-1`, Intel i7-8550U (4 cores,
8 logical CPUs), with 16,120,096 KiB physical memory. Tools were .NET SDK
`10.0.112`, MSBuild `18.0.11`, Microsoft.NETCore.App `10.0.12`,
`dotnet-trace 10.0.750501` (commit `ad724c4b85e7ae775e42f393c6e95a7557a57954`),
and TraceEvent `4.0.0` with its checked-in NuGet lockfile. The Rust preparation
tool used nightly `1.97.0-nightly (9838411cb 2026-04-25)`; Rust resource memory
was not measured in this study.

Measured host projects target `net10.0`, Release, deterministic builds, nullable
and implicit usings enabled, invariant globalization, and warnings as errors.
Builds were serial with shared compilation/node reuse disabled and package
sources cleared. No runtime/allocator/GC settings, forced collections, warmup,
production hooks or source phase timers were added. The runner rejects inherited
CLR/profiler overrides. It binds the complete checkout, SDK/runtime, profiler,
analyzer, selected compiler/executables, generated sources/corpus and host DLLs.
The evidence JSON includes all 96 runtime source identities, fixture/verifier
hashes, the four host and generated-library hashes, and full binding-file digests.

## Whole-host RSS and collection effects

These are Linux `wait4` maximum RSS observations for the individual .NET host,
in MiB. Each range contains eight observations: four routes times two repetitions.
Delta compares matching repetition/case/route pairs.

| Case | Control RSS | Profiled host RSS | Paired profiled − control |
|---|---:|---:|---:|
| original-exact | 300.3–302.8 | 303.6–306.0 | +2.8–+3.8 |
| original-plus1 | 173.5–303.9 | 176.2–306.9 | +2.5–+4.0 |
| normalized-exact | 449.2–449.7 | 452.1–453.1 | +2.5–+3.9 |
| normalized-plus1 | 218.3–219.2 | 221.7–222.1 | +2.6–+3.5 |
| output-exact | 1351.3–1355.5 | 1352.4–1359.2 | −1.1–+7.8 |
| output-plus1 | 1154.4–1156.1 | 1158.8–1159.4 | +2.8–+4.6 |

The largest control was 1,388,064 KiB; the largest profiled host was 1,391,816 KiB.
Startup, framework/native memory, input decoding, mapping, returned values and
complete host checks are all included. Original-plus1 byte routes reject before
decoding, while text routes have already decoded the supplied text in the host;
this explains why that row's route range is especially wide. Individual route
observations remain available in the CSV.

The target and collector are separate children of one measurement wrapper. Only
the target's `wait4` record enters the table. The runtime's EventPipe buffers and
collection effects remain in the profiled target, while collector-process RSS is
excluded. Profiled elapsed times include the startup diagnostics handshake. The
paired deltas show observed variability and collection effects on this machine;
they establish no fixed overhead correction or latency/throughput ranking.

## Sampled managed allocation attribution

The documented [`gc-verbose` profile](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/dotnet-trace)
collects GC activity and sampled allocations through EventPipe. The analyzer uses
[TraceEvent's trace-log stacks](https://github.com/microsoft/perfview/blob/main/documentation/TraceEvent/TraceEventProgrammersGuide.md),
retaining each full allocation-tick stack and its reported amount/type. Existing
enclosing runtime/generated methods determine categories; shared byte-count
helpers inherit the enclosing serializer/normalizer phase. Schema admission and
`JsonDocument` parsing share a conservative category.

Allocation ticks provide sampled accounting, not exact per-phase/type totals.
Their stack/type identifies the triggering allocation; the reported amount can
include allocation accounting since an earlier tick. Inlining, sampling gaps and
missing stacks limit attribution. A zero sampled category therefore says nothing
about whether that phase allocated small objects. Allocated bytes are not a count
of objects alive together and cannot be added to RSS.

Across 48 traces there were 31,938 allocation ticks and no reported lost events.
Two ticks had no stack and retained the `unresolved` category: 221,320 reported
bytes, below 0.001% of campaign allocation accounting. Every allocation/heap
record's PID matched its measured host. The following ranges are reported MiB
across the eight profiled observations per case, rounded to 0.1 MiB.

| Case | Normalization | Schema / JSON parse | Typed projection | Mapping | Serialization |
|---|---:|---:|---:|---:|---:|
| original-exact | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| original-plus1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 |
| normalized-exact | 256.7–256.9 | 192.0–192.1 | 106.7 | 0.0 | 0.0 |
| normalized-plus1 | 128.7–128.8 | 0.0–0.1 | 0.0 | 0.0 | 0.0 |
| output-exact | 128.3–128.5 | 128.0–128.1 | 128.0 | 256.0–256.1 | 704.0 |
| output-plus1 | 128.3–128.5 | 128.0–128.1 | 128.0 | 256.0–256.1 | 576.0 |

Host/framework accounting and boundary encoding are separate CSV columns. For
output-exact, text routes placed about 193 MiB in host/framework and no large
sample in boundary encoding; byte routes placed about 65 MiB in host/framework
and 128 MiB in boundary encoding. Decoding/encoding moves across the public API
boundary with the route, so attributing all process memory to the serializer
would misstate the result.

The large serialization stacks include `WriteScalar` beneath `WriteNode`,
`CanonicalizeOutputStrings` (large char-array/string materializations), and
`Json5Serialize`'s final newline append. The output-exact sample consistently
contains a reported 134,217,752-byte string allocation at the latter stack; the
output-plus1 refusal occurs before that append. The source builds a canonical
string, counts its output bytes plus newline, then creates the newline-terminated
string. Together, source and repeated stacks support investigating one final
text materialization while preserving the exact writer, cap and error behavior.
They do not establish how much RSS such a change would save. The canonicalizer
also serves ordinary JSON, so any follow-up needs shared-owner coordination and
the ordinary writer regressions as well as the JSON5 cohorts.
[Follow-up #765](https://github.com/DeandreT/ferrule/issues/765) records this
bounded investigation and remains available for separate ownership.

## Retained heap and verification limits

The traces retain 672 `GCHeapStats` observations. The CSV's separate post-GC
column is the largest observed sum of the five reported generation sizes in that
trace. Collections occur at runtime-chosen points; no heap graph/root retention
measurement or forced full GC was taken. Exact retained heap, simultaneous
per-phase live memory, native/managed peak decomposition and the precise owner of
the RSS maximum remain inconclusive. These post-GC values are not interchangeable
with allocation accounting or process RSS.

The study establishes no streaming behavior, cross-language memory comparison,
universal RAM ceiling, grammar/schema expansion, desktop parity or optimization.
It preserves full materialization, validation and existing resource refusals.

## Evidence, failures and cleanup

Before physical preparation, the original CLI prerequisite cohort passed all 184
small public calls (92 Rust, 92 C#), with its 276 complete original per-call
records, source/library guards and complete receipt. The successful campaign then
closed all 154 commands without remaining or forcibly cleaned-up process-group
members. Its final source/tool and generated/artifact guards matched. A separate
[verifier](../../examples/performance/json5-allocations/verify.py) checked the full
96-row matrix, all six literal input recipes, every returned byte, original typed
error fields/chains, receipt records, trace PID/count/accounting consistency,
artifact hashes and process closure. It produced the checked-in CSV and summary.

The runner's four focused tests pass, including immutable failure evidence,
modified-source refusal, exact child-error retention and timeout/grandchild
reaping. The Rust example and all C# emitter targets pass strict local Clippy
(`--no-deps`); the Release analyzer builds without warnings, and workspace format
and whitespace checks pass.

Original evidence is retained locally under `target/qualification/160-campaign-2`,
with setup/tool/build originals under `target/qualification/160-tooling` and the
independent audit under `target/qualification/160-audit-final`. The finite protocol
reserved 4 GiB available memory and 20 GiB free disk, limited logical campaign
evidence to 8 GiB, bounded each command and the one-hour campaign, and used a
64 MiB trace buffer. Observed minima were 6.737 GiB available memory and
60.623 GiB free disk; the largest observed campaign size was 1.567 GiB. These are
admission/stop thresholds, not kernel quotas or a RAM cap.

Failed setup originals were retained separately. The first collector probe could
not attach through a launched wrapper; the successful protocol uses an explicit
short reverse diagnostic socket with target and collector as siblings. An early
example build exposed a private fixture-identity field, and an analyzer setup
attempt passed a restore-only option to build; both were corrected before the
campaign. During the first setup campaign, review found the classifier could
confuse shared byte-count helpers with normalization. Its source guard refused
after preparation/builds and before any physical call. That entire failed root,
`target/qualification/160-campaign-1`, remains intact; the corrected campaign
reran the complete small prerequisite cohort and all 96 physical calls.

Raw evidence includes complete effective environments and stays local; review
those records before sharing. Report, CSV and the offline verifier were produced
after measurement closure. The measured host, preparation, collector and analyzer
code stayed unchanged throughout the successful campaign.

After closure, verified prerequisite/CLI executable bytes were retained before
clearing compiler caches. Cargo removed 1.4 GiB of workspace build files. It
refused the dedicated generated cache's missing `CACHEDIR.TAG`; an initial
cleanup audit also caught the temporarily absent recorded CLI executable.
Saved exact executable bytes were restored, ownership of the dedicated directory
was checked, and its disposable cache was cleared while retaining the observed
executable. That directory shrank from 719 MiB to 116 MiB. The complete independent
audit then passed again with the identical observation CSV digest. Cleanup
refusals, file inventories, retained-executable identities and final audit
originals remain under `160-tooling` and `160-audit-after-clean-final`.
