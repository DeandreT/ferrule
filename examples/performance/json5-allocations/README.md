# Generated C# JSON5 allocation study

Issue [#160](https://github.com/DeandreT/ferrule/issues/160) uses the six original
[#145](https://github.com/DeandreT/ferrule/issues/145) physical byte recipes and
the unchanged C# host. The preparer imports
[`corpus.rs`](../../../crates/cli/tests/json5_resources/corpus.rs) directly and
copies [`json5_resources_csharp.cs.txt`](../../../crates/cli/tests/fixtures/json5_resources_csharp.cs.txt)
without changing its calls, full output/error comparisons, or value lifetimes.
There are no runtime or emitter changes.

The finite campaign runs the original 184 small public calls first, retaining
their 276 full original call records and receipt. Only then does it create the
large inputs and build four package-free `net10.0` Release hosts. Two repetitions
cover six cases, four routes (text, text/context, bytes, bytes/context), and two
modes: uninstrumented control and `gc-verbose` EventPipe collection. That is 96
physical C# calls. The second repetition reverses case/route order; the paired
mode order alternates. Each call starts a fresh CLR process. No warmup, forced GC,
GC/allocator overrides, or production instrumentation is used.

## What the observations mean

- Python's Linux `wait4` records the individual .NET host's whole-process peak
  RSS. This includes startup, input, mapping, returned-value retention and the
  host's complete checks. It is not an API-only peak or universal memory ceiling.
- A separate `dotnet-trace` process uses an explicit reverse diagnostic socket
  to collect from managed startup. The host is the measurement wrapper's direct
  child; the collector is its sibling. Collector RSS is excluded from host RSS.
  Runtime EventPipe buffers and collection effects remain in the profiled host.
- `GCAllocationTick` events report sampled managed allocation accounting with
  stacks. The analyzer retains every tick, its reported amount/type, and its full
  stack. It groups existing frames into normalization, typed projection, mapping,
  serialization, boundary encoding, schema/JSON parsing, other runtime, and
  host/framework. Inlining and missing frames can limit attribution. These are
  not exact phase totals or simultaneous live allocations.
- `GCHeapStats` records are retained separately. They describe runtime GC
  generations after observed collections, not retained roots, an object graph,
  or peak live heap. This protocol does not measure exact retained heap; any
  statement about it must remain inconclusive. There is no forced-collection
  trial that could silently change the Release control's lifetimes.
- Compare each profiled host's RSS and elapsed time with its matched control.
  Profiled elapsed time includes the startup diagnostic handshake; differences
  are observed collection effects, not a transferable profiler overhead constant.
  No allocation samples or lost events makes attribution inconclusive; originals
  remain available on every failure.

## Reproduce

Use a reviewed Linux checkout, Python 3, the repository Rust nightly and cached
Cargo dependencies, .NET SDK 10/reference packs, and sufficient headroom. One
study owns its dedicated generated-Rust prerequisite build lane. Other agents
must use separate lanes or coordinate its lease. The runner holds an advisory
lock in that lane, owns each child process group, acts as a Linux subreaper, and
records closure. It never signals other process groups.

Use absolute paths below. `STUDY` must not exist, `TOOLS` must be owned by this
study, and `HOST_TARGET` must be an existing dedicated directory outside the
outer Cargo target. Keep one compatible outer Cargo cache; do not create extra
caches to bypass failed builds or headroom checks.

```bash
REPO='/absolute/path/to/ferrule'
TOOLS='/absolute/path/to/owned/json5-tools'
STUDY='/absolute/path/to/absent/json5-study'
HOST_TARGET='/absolute/path/to/dedicated/generated-host-target'
mkdir -p "$TOOLS/analyzer" "$HOST_TARGET"
cd "$REPO"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo +nightly build --offline -j2 -p codegen-csharp \
  --example json5_allocation_fixture --message-format=json \
  > "$TOOLS/emitter-build.jsonl" 2> "$TOOLS/emitter-build.stderr"
cargo +nightly test --offline -j2 -p cli --features codegen-tests \
  --test code_generation --no-run --message-format=json \
  > "$TOOLS/small-build.jsonl" 2> "$TOOLS/small-build.stderr"
cp examples/performance/json5-allocations/{Analyzer.csproj,Program.cs,packages.lock.json} \
  "$TOOLS/analyzer/"
dotnet restore "$TOOLS/analyzer/Analyzer.csproj" --locked-mode \
  --disable-parallel -p:NuGetAudit=false \
  > "$TOOLS/analyzer-restore.stdout" 2> "$TOOLS/analyzer-restore.stderr"
dotnet build "$TOOLS/analyzer/Analyzer.csproj" -c Release --no-restore \
  -m:1 -nr:false -p:UseSharedCompilation=false -p:NuGetAudit=false \
  > "$TOOLS/analyzer-build.stdout" 2> "$TOOLS/analyzer-build.stderr"
dotnet tool install dotnet-trace --version 10.0.750501 \
  --tool-path "$TOOLS/dotnet-tools" --disable-parallel \
  > "$TOOLS/tool-install.stdout" 2> "$TOOLS/tool-install.stderr"
```

Select the actual emitter and test executable from their successful
`compiler-artifact` records, checking the target name/kind/test profile. Retain
the exact executable bytes and their hashes before later cache cleanup. Do not
guess executable names or reuse a stale binary. The profiling tool and analyzer
need NuGet access during setup only; the generated mapping hosts use a cleared
package source list. The analyzer lockfile pins its dependency closure.

```bash
EMITTER='/actual/compiler-artifact/executable/for/json5_allocation_fixture'
SMALL_TEST='/actual/compiler-artifact/executable/for/code_generation'
python3 "$REPO/examples/performance/json5-allocations/campaign.py" \
  --repo "$REPO" --root "$STUDY" --emitter "$EMITTER" \
  --small-test "$SMALL_TEST" --host-target "$HOST_TARGET" \
  --dotnet '/absolute/path/to/dotnet' \
  --trace "$TOOLS/dotnet-tools/dotnet-trace" \
  --analyzer "$TOOLS/analyzer/bin/Release/net10.0/Analyzer.dll" \
  > "$TOOLS/campaign.stdout" 2> "$TOOLS/campaign.stderr"
```

The runner binds complete checkout sources, current SDK/runtime and selected
tools, generated sources/corpus, and built artifacts. Source/tool/artifact
hash guards run outside measurements. It retains full command stdout/stderr,
exit results, original successes/errors, traces, analyses, headroom and process
closure records. The environment is recorded locally; review it for secrets
before sharing evidence. The runner refuses inherited CLR/profiler overrides.
Finite limits are one hour overall, 15 minutes for the small cohort, three
minutes per other command, 64 MiB trace buffers, 4 GiB available-memory reserve,
20 GiB free-disk reserve, and 8 GiB logical campaign evidence. Headroom is checked
between and during commands. Thresholds are admission/stop rules, not hard
kernel quotas. Short private diagnostic socket paths are necessary on Linux;
their owned scratch contents are retained after child closure.

Review every original and the source/closure guards before interpreting the
result. Never delete or overwrite a failed campaign to retry it. Keep evidence
separate from disposable compiler caches; clear only closed build caches owned
by this study after saving executable identities and verification originals.
