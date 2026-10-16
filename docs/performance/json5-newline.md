# C# JSON5 final-newline materialization — 2026-10-15

For [#765](https://github.com/DeandreT/ferrule/issues/765), JSON5 now appends its
document newline to the canonicalizer's builder before materializing the final
string. The repeated 64 MiB output-success recipe reported about 576 MiB of
serialization allocation accounting, versus 704 MiB in the retained
[#160 baseline](json5-allocations.md). Matched uninstrumented whole-host peak RSS
fell by 123.6–128.6 MiB on this machine. Output-limit refusals retained the same
typed errors; their RSS varied slightly upward in some runs.

The [96 candidate observations](json5-newline-observations-2026-10-15.csv) and
[source identities, comparisons and qualification summary](json5-newline-evidence-2026-10-15.json)
retain individual results. Allocation accounting and whole-host RSS measure
different things. Exact retained heap and an API-only peak remain unmeasured.

## Change and source boundary

The shared canonicalizer has a private builder-returning helper. Ordinary JSON
continues through its existing string-returning wrapper and existing newline
append. JSON5 takes the builder, appends one newline, calls `ToString()` once,
then applies the unchanged UTF8 byte census and output cap to the complete text.
It retains full materialization and the same typed output-resource exception.

The only production differences are
[`FerruleJson.Canonical.cs`](../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.Canonical.cs)
and [`FerruleJson.Json5Codec.cs`](../../runtime/csharp/Ferrule.Runtime/Json5/FerruleJson.Json5Codec.cs).
All 96 runtime source/project identities are recorded. Each of the four generated
C# library closures has the same 87 C# source paths as its retained baseline and
differs in exactly those two runtime files. The generated mapping and host source
are unchanged. There is no new byte-counting algorithm or public signature.

The measured checkout was based on
`60ff0466a4d1e7fbefbaf428b8b3bb95f18ce833` (the profiling fixture in PR #766),
whose production runtime preimage is the baseline's
`7ff83194882f49285b47d14bdc44464d44bf8dbe`. The candidate binding and retained
patch identify the working changes; the evidence JSON identifies the complete
candidate runtime. A later remote resource-binding update did not overlap these
runtime files. The follow-up remains stacked on the profiling fixture.

## Matched workload and observations

The unchanged [finite profiling protocol](../../examples/performance/json5-allocations/README.md)
reran the original 184-call prerequisite before preparing any physical input.
It then ran all six original literal recipes, all four public generated C# routes,
two repetitions and paired control/`gc-verbose` modes: 96 separate CLR processes.
The second repetition reversed case/route order; paired mode order alternated.
The baseline and candidate campaigns were collected sequentially, rather than
interleaving the two source variants. Both used the same machine, .NET SDK
10.0.112/runtime 10.0.12, Release settings, hosts, profiler and analyzer. No GC
overrides, forced collections, warmup or source phase timers were introduced.

These ranges contain eight uninstrumented observations per cell, in MiB of Linux
`wait4` whole-host maximum RSS:

| Case | Before | After |
|---|---:|---:|
| original-exact | 300.3–302.8 | 300.7–302.8 |
| original-plus1 | 173.5–303.9 | 172.7–303.4 |
| normalized-exact | 449.2–449.7 | 449.2–449.6 |
| normalized-plus1 | 218.3–219.2 | 218.8–219.3 |
| output-exact | 1351.3–1355.5 | 1223.0–1227.7 |
| output-plus1 | 1154.4–1156.1 | 1155.8–1162.0 |

For output-exact, matching repetition/route pairs decreased by 123.6–128.6 MiB in
controls and 124.1–128.4 MiB in profiled hosts. Profiled output-exact RSS changed
from 1352.4–1359.2 to 1226.1–1231.1 MiB. For output-plus1, control differences
were +0.03–+6.37 MiB and profiled differences were −0.40–+4.44 MiB; profiled
candidate RSS was 1159.0–1163.6 MiB. The refusal path therefore has no demonstrated
RSS improvement. These are dated observations including startup, native memory,
input decoding, mapping, full returned-value retention and host checks. They do
not establish a universal RAM ceiling or a fixed profiler-overhead correction.

Reported serialization allocation accounting across the eight profiled calls
per case was:

| Case | Before (MiB) | After (MiB) |
|---|---:|---:|
| output-exact | 704.0 | 576.0–576.1 |
| output-plus1 | 576.0 | 576.0–576.1 |

Output-exact's decrease is approximately 128 MiB, or 18.2% of that phase's
sampled accounting. Every baseline output-exact trace contains one large
`System.String.Concat` sample beneath `Json5Serialize`; none of the eight
candidate traces contains it. The candidate retains the large builder and final
`StringBuilder.ToString()` stacks. Source and repeated stacks support removal of
the extra full-text newline concatenation. The builder may allocate a small
additional chunk for the appended newline, including on the refusal path.

The 48 candidate traces contain 31,928 allocation ticks, zero missing stacks and
zero reported lost events. The unchanged classifier places the new helper's
allocations under the enclosing `Json5Serialize` phase; no samples fell into
`other-runtime` or `unresolved`. Allocation-tick amounts are sampled accounting,
not exact object sizes, exact per-phase totals or simultaneous live memory. The
672 `GCHeapStats` events remain separate post-GC observations in the CSV and do
not establish retained roots, a heap graph or ownership of the RSS maximum.

## Complete behavior and evidence checks

- The original prerequisite passed all 184 small public calls: 92 Rust and 92 C#,
  retaining its 276 complete original result records and source/library guards.
- All 96 physical calls passed: 48 complete returned values and 48 expected typed
  refusals. The independent verifier checked every literal input and returned
  byte, original exception fields/chains, trace PID/accounting consistency,
  source/artifact identities and all 154 closed command groups.
- All 221 C# smoke tests passed, including the new newline/ordinary-writer
  regression. Its seven literal cases make 14 JSON5 and 28 ordinary-JSON complete
  comparisons across text/byte results, escaping, supplementary/BMP Unicode,
  U+2028/U+2029 and builder-sized string boundaries.
- Separate [Unicode cap controls](../../examples/performance/json5-newline/README.md)
  passed eight calls across baseline/candidate and text/byte routes. Complete
  67,108,864-byte outputs containing 22,369,616 `雪` characters matched the literal
  oracle. Adding one ASCII character preserved `Output / OutputDocumentBytes`,
  requested 67,108,865, maximum 67,108,864, the original inner resource cause and
  absence of a partial result. A separate readback checked every retained byte
  and all ten command closures. These controls ran after the profiling campaign
  and do not enter its RSS/allocation comparison.
- Strict Clippy passed for all C# emitter targets (`--no-deps`), and workspace
  formatting, whitespace, report links and checked-in data/source digest checks
  passed.

Original evidence remains under `target/qualification/765-campaign-1`, its
independent audit under `765-audit-1`, supplementary controls under
`765-unicode-controls-1`/`765-unicode-audit-1`, and setup/build/smoke originals under
`765-tooling`. The campaign retained every full output/error, raw trace and
analysis record, and complete source/tool/generated/artifact bindings. Its final
guards passed with no remaining or forcibly cleaned-up process-group members.
The original baseline remains separately retained under `160-campaign-2`.

The finite protocol reserved 4 GiB available memory and 20 GiB free disk, bounded
logical campaign evidence to 8 GiB and each command to 180 seconds (900 seconds
for the prerequisite), and imposed a one-hour campaign deadline. Candidate minima
were 6.685 GiB available memory and 59.376 GiB free disk; largest observed logical
campaign size was 1.566 GiB. These are admission thresholds and observations,
rather than kernel quotas. Complete environments stay private in local evidence;
checked-in data contains selected non-secret source/artifact identities.

After all builds and calls closed, exact prerequisite CLI/Rust-host executables
were retained before Cargo removed 1.4 GiB of workspace build files and 734.8 MiB
from the owned generated-host cache. The exact observed executables were restored
at their recorded paths. A complete post-cleanup independent audit passed with
the identical 96-row CSV digest. Original evidence and other agents' caches were
preserved. Cleanup records and retained-executable identities remain local under
`765-tooling` and `765-retained-build-artifacts`.

This result qualifies the bounded JSON5 newline change. It does not extend
grammar/schema admission, add streaming, change allocator/GC policy, establish
cross-language memory rankings or advance desktop/Basic/ScopeContext gates.
