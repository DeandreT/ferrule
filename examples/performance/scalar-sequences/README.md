# Scalar filter/map memory study

These Linux fixtures and hosts reproduce the bounded study for
[#207](https://github.com/DeandreT/ferrule/issues/207). See the
[report](../../../docs/performance/scalar-sequence-memory.md) for its dated source,
build profiles, individual observations and verification limits.

Both Projects generate `1..=Count`, evaluate one string capture, keep every item,
and call predicate then mapper with item, input position and capture. The numeric
mapper returns item; the capture mapper returns capture. Each Project has one
primary JSON output. There are no named/dynamic sources or targets, failure rules,
nested functions, JSON5 routes or cancellation hooks. Default existing item/work
limits apply. [fixtures.json](fixtures.json) freezes six dimensions, twelve complete
input/output recipes, two small byte literals and the two reversed repetitions.
The largest output is 33,558,419 bytes; the hosts require every document to be
strictly below 64 MiB.

The commands below reproduce public preparation and host routes. The reported
study used a separately reviewed owning runner for deadlines, exclusive compiler
ownership, source/survey guards, process reaping, lease release and retained
originals. These command patterns are not a replacement for that runner or proof
of its closure. Retain all commands, full stdout/stderr and exit statuses, including
refusals; review complete artifacts and outcomes before calling a campaign qualified.

## Tools, paths and shared build profile

Use Linux with `/proc`, GNU `time -v`, Python 3, Ferrule's Rust nightly
toolchain and a .NET 10 SDK with its `net10.0` reference packs already installed. Offline
Rust builds also require the locked dependencies in the existing Cargo cache.
No NuGet package is used by the C# host; offline does not install missing SDK packs.

Run these Bash patterns from the exact reviewed Ferrule checkout. Replace every
absolute placeholder, keep all paths quoted, and use an absent study directory.
Use one compatible existing Cargo target directory for both Rust builds. Check
available space before and after each build and process: retain the study's 20 GiB
reserve plus adequate build/output headroom. Do not create another target cache
to work around insufficient space or a profile mismatch.

```bash
set -euo pipefail
SCALAR_STUDY_REPO='/absolute/path/to/reviewed/ferrule'
SCALAR_STUDY_ROOT='/absolute/path/to/fresh/scalar-sequence-study'
SCALAR_STUDY_TARGET='/absolute/path/to/compatible/shared/cargo-target'
SCALAR_STUDY_CARGO='/absolute/path/to/qualified/cargo'
SCALAR_STUDY_RUSTC='/absolute/path/to/qualified/rustc'
SCALAR_STUDY_DOTNET='/absolute/path/to/qualified/dotnet'
SCALAR_STUDY_TIME='/absolute/path/to/qualified/GNU-time'
SCALAR_STUDY_FIXTURES="$SCALAR_STUDY_ROOT/fixtures"
SCALAR_STUDY_PREPARED="$SCALAR_STUDY_ROOT/prepared"
SCALAR_STUDY_BINDING="$SCALAR_STUDY_ROOT/root-binding.json"
SCALAR_STUDY_BOUND="$SCALAR_STUDY_ROOT/bound"
test ! -e "$SCALAR_STUDY_ROOT"
test ! -L "$SCALAR_STUDY_ROOT"
mkdir "$SCALAR_STUDY_ROOT"
cd "$SCALAR_STUDY_REPO"
export RUSTC="$SCALAR_STUDY_RUSTC"
export CARGO_TARGET_DIR="$SCALAR_STUDY_TARGET"
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_NET_OFFLINE=true
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
```

Use ordinary dev/Debug profiles. Do not add release/optimization flags, allocator
settings, forced collection or cache flushing. Record the actual Cargo/rustc,
.NET, Python, GNU time and operating-system versions and the effective environment.
The final-target `rustc -- -D warnings` below denies host warnings without changing
the dependency profile through a global `RUSTFLAGS` override. Workspace warning
qualification remains a separate gate.

## Materialize and prepare, outside measurement

[prepare.py](prepare.py) writes independent fixture/oracle bytes and an unbound
`campaign.json`; it does not run Ferrule or compile a host. Build the native
example and use its public `prepare` command to decode/validate/lower both saved
Projects, retain complete emitted artifacts and compose the two generated hosts.

```bash
python3 "$SCALAR_STUDY_REPO/examples/performance/scalar-sequences/prepare.py" \
  "$SCALAR_STUDY_FIXTURES" \
  > "$SCALAR_STUDY_ROOT/fixtures.stdout" 2> "$SCALAR_STUDY_ROOT/fixtures.stderr"

"$SCALAR_STUDY_CARGO" rustc --offline --locked -j1 -p cli \
  --example scalar_sequence_memory --message-format=json -- -D warnings \
  > "$SCALAR_STUDY_ROOT/native-build.jsonl" 2> "$SCALAR_STUDY_ROOT/native-build.stderr"
```

Read the `compiler-artifact` JSON line for target `scalar_sequence_memory` and set
`SCALAR_STUDY_NATIVE` to its complete absolute `executable` path. Do not guess a
Cargo layout or select a stale executable by basename. Retain the build records
and hash the complete selected ELF before use.

```bash
SCALAR_STUDY_NATIVE='/absolute/executable/from/native-build.jsonl'
"$SCALAR_STUDY_NATIVE" prepare "$SCALAR_STUDY_FIXTURES" \
  "$SCALAR_STUDY_PREPARED" "$SCALAR_STUDY_REPO" \
  > "$SCALAR_STUDY_ROOT/preparation.stdout" 2> "$SCALAR_STUDY_ROOT/preparation.stderr"

"$SCALAR_STUDY_CARGO" generate-lockfile --offline \
  --manifest-path "$SCALAR_STUDY_PREPARED/rust/Cargo.toml" \
  > "$SCALAR_STUDY_ROOT/rust-lock.stdout" 2> "$SCALAR_STUDY_ROOT/rust-lock.stderr"
"$SCALAR_STUDY_CARGO" rustc --offline --locked -j1 \
  --manifest-path "$SCALAR_STUDY_PREPARED/rust/Cargo.toml" \
  -p scalar_sequence_host --bin scalar_sequence_host \
  --message-format=json -- -D warnings \
  > "$SCALAR_STUDY_ROOT/rust-build.jsonl" 2> "$SCALAR_STUDY_ROOT/rust-build.stderr"
```

The freshly composed Rust workspace initially has no lockfile; generate and retain
it before the locked build. Set `SCALAR_STUDY_RUST` from that build's
`compiler-artifact` target `scalar_sequence_host`, again retaining and hashing the
complete ELF. The preparer preserves raw emitted sets under `emitted-originals`.
Its Rust packaging removes only each composed member's standalone workspace
marker; C# packaging changes the two generated namespaces and shares identical
runtime files. Both packaging receipts are retained. Build from `prepared/rust`
and `prepared/csharp`, not the raw standalone emission directories.

Use a source-cleared NuGet configuration and the prepared `Host.csproj`:

```bash
SCALAR_STUDY_RUST='/absolute/executable/from/rust-build.jsonl'
cat > "$SCALAR_STUDY_ROOT/NuGet.Config" <<'XML'
<?xml version="1.0" encoding="utf-8"?>
<configuration><packageSources><clear /></packageSources></configuration>
XML
"$SCALAR_STUDY_DOTNET" restore "$SCALAR_STUDY_PREPARED/csharp/Host.csproj" \
  --configfile "$SCALAR_STUDY_ROOT/NuGet.Config" --disable-parallel \
  -p:NuGetAudit=false -p:UseSharedCompilation=false -m:1 \
  -p:DisableTransitiveFrameworkReferenceDownloads=true \
  -p:EnableTargetingPackDownload=false -p:EnableRuntimePackDownload=false \
  > "$SCALAR_STUDY_ROOT/csharp-restore.stdout" 2> "$SCALAR_STUDY_ROOT/csharp-restore.stderr"
"$SCALAR_STUDY_DOTNET" build "$SCALAR_STUDY_PREPARED/csharp/Host.csproj" \
  --configuration Debug --no-restore -p:UseSharedCompilation=false -m:1 \
  > "$SCALAR_STUDY_ROOT/csharp-build.stdout" 2> "$SCALAR_STUDY_ROOT/csharp-build.stderr"
SCALAR_STUDY_CSHARP_DLL="$SCALAR_STUDY_PREPARED/csharp/bin/Debug/net10.0/ScalarSequenceHost.dll"
```

`Host.csproj` declares `TreatWarningsAsErrors=true`, uses no `PackageReference`,
and copies `source-schema.json` beside the assembly. Retain and bind the complete
runtime files in the output directory, not just the DLL. Use the same qualified
.NET runtime and complete host output for qualification and measured calls.

## Qualify six small cases before the 72 rows

Run the two `Count=3`, capture-width `4` controls on each of native, Rust and C#,
then a separate untimed verifier for each result: six public JSON calls and six
typed/byte verification calls. These twelve invocations are outside the measured
72 rows. Every output and verifier directory below must be absent.

```bash
for SCALAR_STUDY_MODE in numeric capture; do
  SCALAR_STUDY_SMALL="ordered-$SCALAR_STUDY_MODE-3"
  "$SCALAR_STUDY_NATIVE" measure "$SCALAR_STUDY_MODE" \
    "$SCALAR_STUDY_FIXTURES/project-$SCALAR_STUDY_MODE.json" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE" \
    > "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE.stderr"
  "$SCALAR_STUDY_NATIVE" verify "$SCALAR_STUDY_MODE" 3 4 \
    "$SCALAR_STUDY_FIXTURES/project-$SCALAR_STUDY_MODE.json" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.expected.json" \
    "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE/actual.json" \
    "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE-verify" \
    > "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE-verify.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-native-$SCALAR_STUDY_MODE-verify.stderr"

  "$SCALAR_STUDY_RUST" measure "$SCALAR_STUDY_MODE" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE" \
    > "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE.stderr"
  "$SCALAR_STUDY_RUST" verify "$SCALAR_STUDY_MODE" 3 4 \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.expected.json" \
    "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE/actual.json" \
    "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE-verify" \
    > "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE-verify.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-rust-$SCALAR_STUDY_MODE-verify.stderr"

  "$SCALAR_STUDY_DOTNET" "$SCALAR_STUDY_CSHARP_DLL" measure "$SCALAR_STUDY_MODE" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE" \
    > "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE.stderr"
  "$SCALAR_STUDY_DOTNET" "$SCALAR_STUDY_CSHARP_DLL" verify "$SCALAR_STUDY_MODE" 3 4 \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.input.json" \
    "$SCALAR_STUDY_FIXTURES/$SCALAR_STUDY_SMALL.expected.json" \
    "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE/actual.json" \
    "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE-verify" \
    > "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE-verify.stdout" \
    2> "$SCALAR_STUDY_ROOT/small-csharp-$SCALAR_STUDY_MODE-verify.stderr"
done
```

The native host alone takes a saved `PROJECT` argument. For each mode, all three backends use the same input and oracle; the generated
hosts embed the prepared schemas. Check all four
fields in every `verification.original.json`: `source_equal`, `output_equal`,
`bytes_equal`, `input_stable`. Review complete typed outcomes, field/item order,
Group origins, errors and full bytes through EOF; exit zero and hashes alone do
not replace those comparisons. Preserve any refused/failed preparation or control
attempt and use a distinct fresh output for a corrected attempt.

## Bind and run the frozen schedule

[bind_campaign.py](bind_campaign.py) requires a reviewed binding JSON. It retains
the original campaign and binding bytes and pins the complete host/tool files.
The binding has these keys:

```text
root_reviewed_final_main_and_sources: true, only after the actual review
reviewed_main: exact 40-character verified source commit
authority_originals: nonempty list of complete reviewed receipt file identities
native, rust, csharp_dll, dotnet, time: complete file identities
root_owner_binding: the reviewed owning-runner/environment/closure prerequisites
```

Each file identity is `{"path":"/absolute/path","bytes":N,"sha256":"full hex"}`.
The binder adds complete stat fields when checking those bodies. A reviewed
receipt is an original record of actual source/lock/host/tool identities, commands,
build results and the six small verifier outcomes, rather than an asserted pass.
The DLL identity is accompanied by receipt coverage of its complete runtime files.
`root_owner_binding` records the owning runner identity, work/closure bounds,
source inventory and survey guards, serial ownership and disk-reserve admission.
The binder copies this field; it does not independently prove those conditions.
It does not launch a child or grant semantic acceptance merely because the
reviewed flag is supplied.

```bash
python3 "$SCALAR_STUDY_REPO/examples/performance/scalar-sequences/bind_campaign.py" \
  "$SCALAR_STUDY_FIXTURES/campaign.json" "$SCALAR_STUDY_BINDING" "$SCALAR_STUDY_BOUND" \
  > "$SCALAR_STUDY_ROOT/binding.stdout" 2> "$SCALAR_STUDY_ROOT/binding.stderr"
```

Run the exact arrays in `bound/fixed72.json`, serially, in ordinal order. For each
row, the reviewed owner invokes `run_argv`, retains complete GNU time/stdout/stderr
and child/closure status, then invokes `verify_argv` in a separate untimed process.
Do not substitute `cargo run`, `dotnet run`, a shell `time` keyword or a build for
the recorded executable. The arrays are ordinary argv vectors, not shell text.
Their interfaces are:

```text
GNU_TIME -v -o TIME_ORIGINAL -- NATIVE measure MODE PROJECT INPUT FRESH_TRIAL
NATIVE verify MODE COUNT WIDTH PROJECT INPUT ORACLE FRESH_TRIAL/actual.json FRESH_VERIFY

GNU_TIME -v -o TIME_ORIGINAL -- RUST measure MODE INPUT FRESH_TRIAL
RUST verify MODE COUNT WIDTH INPUT ORACLE FRESH_TRIAL/actual.json FRESH_VERIFY

GNU_TIME -v -o TIME_ORIGINAL -- DOTNET CSHARP_DLL measure MODE INPUT FRESH_TRIAL
DOTNET CSHARP_DLL verify MODE COUNT WIDTH INPUT ORACLE FRESH_TRIAL/actual.json FRESH_VERIFY
```

Repetition 1 traverses `(1,32)`, `(16,32)`, `(128,32)`, `(16,4096)`,
`(16,262144)`, `(128,262144)`; for each dimension it runs native, Rust, C#, and
numeric then capture within each backend. Repetition 2 reverses dimensions,
backend order and mode order. That is 72 measured calls plus 72 separate verifiers.
Keep both repetitions and every refused/failed/unavailable row; do not replace
individual measurements with an average or silently retry the same output path.

## Reading the evidence

Each measured host reads the input, calls its ordinary JSON route once and retains
the returned complete string. Startup, native saved-Project decoding or generated
embedded-schema handling, parsing, eager mapping, ordinary serialization and
result-file retention remain measured. Compilation, fixture creation, oracle
construction, source hashing by the outer owner and typed/byte verification are
outside that child measurement.

GNU time maximum RSS belongs to the genuine measured child, not its owning runner.
Keep raw CPU/wall precision and units. The three `/proc` records are sparse current
VmRSS and cumulative VmHWM observations at before-call, after-call and after-write;
they do not identify exact parser, stage or writer peaks. Native/Rust owned strings
may be cloned; C# strings are immutable references. Logical `N × L` output content
does not establish physical string allocation count. Two repetitions without
forced collection or cache flushing support finite observations, not allocation
attribution, statistical significance, a total-RAM ceiling or an optimization claim.
