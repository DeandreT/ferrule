# PDF Worker Contract

The CLI and native editor can run file-based PDF extraction in an owned Linux
child. The option is off by default. The worker returns one typed extraction
result; mapping and publication stay in the parent. Library byte calls and
editor-text previews remain in-process.

## Current boundary

[`format-pdf`](../crates/format-pdf/src/lib.rs) exposes `read(path, layout)` and
`from_bytes(bytes, layout)`. Both reject a repair-dependent layout before reading
or decoding PDF bytes. Path reading checks file size and reads at most the input
limit plus one byte; byte reading checks the original slice length.

| Existing library boundary | Limit |
| --- | ---: |
| Original PDF input | 8 MiB |
| Pages, checked after document loading and page traversal | 256 |
| Collected visual events | 1,000,000 |
| Collected decoded text, in aggregate | 1 MiB |
| Layout output nodes | 1,000,000 |
| Layout output value bytes, in aggregate | 1 MiB |
| Layout evaluation work units | 1,000,000 |
| Output instance depth | 64 |
| Validated layout command nodes / depth / aggregate strings | 4,096 / 64 / 1 MiB |

MiB means 1,048,576 bytes. The separate collection and evaluation ledgers are
not interchangeable. Output field names, object overhead, retained capacities,
and upstream parser allocations are not a total-memory ledger.

[`extract_pages`](../crates/format-pdf/src/extract/mod.rs) loads the document,
traverses pages and extracts content before layout evaluation. It catches
unwinding panics around some upstream operations and rejects encrypted PDFs;
this does not recover an allocation abort or bound decompression, recursive
content traversal, CPU use, or wall time before collector callbacks. The worker
must preserve current positioned-text, painted-edge, page-box/rotation and
layout semantics, including explicit unsupported and repair-dependent errors.
It adds no PDF output, OCR, rendering service, or new extraction command.

The [filesystem dispatcher](../crates/cli/src/lib.rs) calls `format_pdf::read`
for configured primary, static and dynamic PDF sources. The
[payload dispatcher](../crates/cli/src/payload.rs) calls `from_bytes`; its general
64-MiB document limit does not replace PDF's smaller 8-MiB limit.
GUI [file runs](../crates/gui/src/app/run.rs) run on a Rust thread and check
cancellation through debug and before-publication controls. Without the owned
worker those checks cannot interrupt synchronous PDF decoding; the opt-in below
moves extraction into a child that cancellation can stop.

## Eligible host and compatibility

The `linux-x86_64-v1` profile requires Linux x86_64, a matching packaged
worker, PIDfd signaling/wait ownership, and the limits below.

Missing capabilities or failed setup return `UnsupportedHost` or `ResourceSetup`
before PDF parsing. Other platforms cannot use this profile; a failed request
never falls back to in-process extraction.

Use `ferrule run --pdf-worker` to select this policy for filesystem PDFs.
It accepts one regular local file per request, not URLs, pipes, devices, inline
payloads or a caller-selected command. The host selects the packaged worker
without searching beside the PDF or invoking a shell. Native paths need not
become UTF-8 wire identities. This is process/resource isolation, not a
filesystem, network or malicious-code sandbox.

The packaged worker is `ferrule-pdf-worker` in the directory of the running
`ferrule` or `ferrule-gui` executable. Admission requires a regular single-link
file, owned by the user or root, executable and not writable by group or others,
in a directory whose every ancestor is owned by the user or root and is not
writable by group or others (a root-owned sticky directory excepted). Cargo
hard-links each built binary between `target/<profile>/` and
`target/<profile>/deps/`, so a build directory is refused. Development builds use
[`scripts/dev/stage-pdf-worker-package.sh`](../scripts/dev/stage-pdf-worker-package.sh),
which builds the front ends and the worker and copies them into a private package
directory to run from; see [Development](development.md#run-the-pdf-worker-from-a-build).
Staging is an ordinary package to admission: no development path, environment
variable or relaxed check selects or admits a worker.

One owner runs one worker at a time per mapping run. Each PDF load has its own
request; the existing dynamic-source cache and target/source selection rules
stay in the parent. A per-extraction deadline is not a deadline for the whole
mapping or all dynamic loads. Payload PDF isolation and generated backends are
outside this first profile.

## Bounded request and result

The parent creates a fresh owned directory with mode `0700`. Names inside it are
fixed: `request.json`, `input.pdf`, `instance.part`, `instance.json`, and
`result.json`. No request supplies an output destination or relative sibling
dependency. Reject symlinks and nonregular files at open, using the opened file's
identity for subsequent checks. Close unneeded inherited descriptors and clear
unneeded environment entries. The worker must not launch children.

The parent first validates the layout and its repair dependency, checks the host
profile, and takes a bounded snapshot of the opened PDF into `input.pdf`.
Compare input identity/size/modification metadata before and after copying;
refuse an observed change. Freeze the complete snapshot and request read-only
before launch. The worker rereads and verifies the snapshot's byte count/hash.
Metadata checks do not prove that a concurrently written original was unchanged;
the request identifies the exact staged bytes, and callers must keep the
original stable when they require original-file snapshot semantics.

| New host transport boundary | Proposed fixed limit |
| --- | ---: |
| One request JSON, original UTF-8 bytes | 4 MiB |
| Staged PDF | Existing 8 MiB |
| Serialized typed instance, original UTF-8 bytes | 64 MiB |
| Result/error envelope | 64 KiB |
| Ready frame / total captured stderr | 4 KiB / 64 KiB |
| JSON container depth in request and result transport | 256 |
| Logical bytes retained in one job directory | 80 MiB |

These are additional host limits, not new library limits. A layout/output that
the pure library can represent may exceed the transport cap and receive a typed
host refusal. Admit at least 96 MiB of additional local scratch headroom per
job, while preserving the caller's other disk reservations. File-system block
overhead, retained earlier failures and concurrent unrelated work are separate.
Do not copy a full instance to stdout or accumulate unbounded diagnostics.

Request fields are exactly `protocol: "ferrule.pdf-worker/1"`, a fresh 32-character
lowercase hexadecimal `request_id`, `profile: "linux-x86_64-v1"`, the existing
serialized `PdfLayout`, and `input: {name: "input.pdf", byte_len, sha256}`.
Unknown versions, duplicate envelope keys, unknown envelope fields, invalid
integer lengths or a mismatched profile refuse. A bounded structural scan
precedes recursive deserialization; `PdfLayout` construction must still perform
its existing validation. No request may raise resource limits or choose paths.

The child installs policy before reading the request or PDF, publishes one
bounded ready frame containing the request identity and effective limits, then
waits for the owner's single start token. The parent acquires ownership before
that token. EOF or malformed control input refuses. Control pipes carry only
handshake/diagnostics; the PDF and typed result remain filesystem documents.

The result envelope repeats protocol, request ID and profile and contains exactly
one outcome. `ok` names only `instance.json` and records its complete length and
SHA-256. A separate tagged wire tree represents ordered Group field pairs,
Repeated items, String scalars and Null scalars; those are the current PDF output
variants. It must preserve field/item order, empty strings, nulls and Unicode
exactly. No generic numeric coercion, DocumentSet or MappedSequence is admitted.
The encoder writes through a capped writer to `instance.part`, closes it and
renames it only after complete encoding. The parent independently recounts
nodes/depth/value bytes and validates the wire shape before rebuilding an
`ir::Instance`; checksums alone do not establish semantic correctness.

`pdf_error` carries the existing `PdfError` variant and its structured data,
including repair dependency, I/O kind and any value path. Bounded display/cause
text is diagnostic, with an explicit truncation flag; it never substitutes for
the variant. A valid library error can exceed the 64-KiB envelope cap because
layout names and paths have a larger aggregate string budget. In that case,
return the distinct host outcome `ErrorEnvelopeTooLarge`, retaining the original
`PdfError` variant identity, complete attempted encoded length and SHA-256 of the
attempted error envelope. Count and hash through a streaming sink without
buffering the oversized envelope. Do not reconstruct a `PdfError` from truncated
structured fields; any bounded diagnostic excerpt must declare truncation.
Host protocol, snapshot, resource and output-limit errors have separate
categories. `ok` requires child exit 0; a complete declared error
requires exit 2. Other exits/signals are retained as `WorkerExit`, even if a
partial document exists. A missing, trailing, duplicate or inconsistent result
is `ProtocolError`. Never interpret an empty/malformed result as an empty PDF.

## Resource and child-ownership policy

<a id="acceptance-gates"></a>

| Profile control | Proposed policy |
| --- | --- |
| Address space | `RLIMIT_AS` soft and hard 512 MiB |
| Child CPU | `RLIMIT_CPU` soft 30 s, hard 31 s |
| Each child-created file | `RLIMIT_FSIZE` soft and hard 64 MiB; smaller transport caps still apply |
| Core dumps | `RLIMIT_CORE` soft and hard zero |
| Owner work deadline | 30 s from admission, including snapshot, launch, handshake, extraction, result validation and normal reap |
| Stop/reap allowance | At most 5 additional seconds; at most 1 s TERM grace, then KILL for the held child |

Install no broader limits than the caller already has: reject an inherited limit
that cannot admit this fixed profile, rather than raising it. Verify effective
limits before the start token. The finite
[resource profile](performance/pdf-worker-resource-profile.md) measures these
numbers with the packaged executable and small self-authored PDFs; it does not
change them. Linux [`RLIMIT_AS`](https://man7.org/linux/man-pages/man2/getrlimit.2.html)
limits virtual address space. Allocation or stack expansion can fail or terminate
the child; it is not an RSS measurement. `RLIMIT_RSS` supplies no usable modern
Linux RSS enforcement for this policy. CPU and file-size limits likewise do not
replace the owner's wall clock or per-file protocol checks.

The sole launch/reap owner keeps the `Child` and a held PIDfd, records child PID
and birth identity, and validates the spawn relationship before start. Acquire
the PIDfd while the direct child is unreaped; require ordinary `SIGCHLD`
disposition, no `SA_NOCLDWAIT` and no competing reaper. Refuse an incompatible
embedding host. [`pidfd_open`](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)
is available from Linux 5.3; its direct-child acquisition guarantee depends on
those reaping conditions. A version number alone does not prove availability.
Signals use the [held PIDfd](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html),
never a reused PID, broad process name or unrelated process group. Unknown child
or descendant observations are lifecycle errors; the profile authorizes no
signal to an unowned process.

Before ready, the worker also installs a parent-death `SIGKILL` and checks the
expected parent on both sides of setup. The launch thread remains alive until
reap. [`PR_SET_PDEATHSIG`](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html)
refers to the creating thread, and a parent already gone before installation
does not trigger the signal. This is a last-resort exit mechanism, not a
replacement for the live owner's wait proof or a promise of cleanup after a
parent crash.

## Deadline, cancellation and terminal states

```mermaid
flowchart TD
    A[Filesystem PDF request] --> B[Validate layout and admit profile]
    B --> C[Freeze bounded input and request]
    C --> D[Launch owned child and verify ready]
    D --> E[Permit extraction]
    E --> F[Bounded complete result]
    F --> G[Reap and validate identity and typed tree]
    G --> H[Return Instance to parent mapping]
    B --> X[Return failure without an instance]
    E --> S[Cancel, timeout or failure: stop owned child]
    S --> R[Reap or retain incomplete closure]
    R --> X
```

```mermaid
stateDiagram-v2
    [*] --> Admitting
    Admitting --> Refused: prelaunch failure
    Admitting --> Starting: frozen request
    Starting --> Running: ownership and ready verified
    Starting --> Stopping: cancel, deadline or startup failure
    Running --> Checking: complete result
    Running --> Stopping: cancel, deadline or failure
    Checking --> Succeeded: valid result and normal reap before cutoff
    Checking --> Stopping: late, cancelled or invalid result
    Stopping --> FailedClosed: child reaped and capture closed
    Stopping --> ClosureIncomplete: stop allowance expired or ownership error
    Refused --> [*]
    Succeeded --> [*]
    FailedClosed --> [*]
```

Cancellation is checked before any job creation, during bounded staging and
owner polling, and immediately before returning a validated instance. Once
cancellation is observed it wins over a late success and stops the active child;
no mapping target is staged by extraction. A worker deadline is independent of
GUI debug pause time, which starts after input extraction. The work cutoff is
checked again after complete result validation, normal reap and required
evidence publication. Success cannot be published by an operation that overran
the cutoff while writing its receipt.

Closing control input, TERM grace, PIDfd KILL, exit collection, pipe closure and
all descriptor/file releases are explicit owner steps. Retain raw exit/signal,
observed primary reason, monotonic timestamps, effective limits, byte counts,
bounded diagnostics and every cleanup error. A signal number or `ENOMEM` alone
does not identify the cause. [Dropping Rust `Child`](https://doc.rust-lang.org/std/process/struct.Child.html)
does not kill or wait for it; `try_wait`/wait must establish actual reap.

`Cancelled` and `TimedOut` may be returned as closed failures only after reap;
`ClosureIncomplete` preserves the original reason and every ownership/cleanup
error. It returns no instance and permits no target publication or reuse of that
job/owner. The manager retains the child handle and unresolved ownership and
keeps servicing closure rather than detaching it. GUI integration must remain
Stopping/defer close while that responsibility is unresolved. Blocked kernel or
filesystem operations can defeat prompt termination; 30+5 s is an acceptance
cutoff, not a guaranteed bound on physical exit. Parent crash, SIGKILL, unexpected
descendants and a lost wait relationship cannot produce a normal-closure claim.

Delete a successful job only after its child and files are closed and the typed
result is owned by the caller. Retain bounded failed-job evidence for explicit
caller cleanup; protect it from mapping target replacement. Cleanup failure
prevents normal success and stays observable. Retention does not grant permission
to read arbitrary sibling files or delete a caller's original PDF.

## GUI coupling and source ownership

The worker client needs a cancellation token that can be polled during input
loading. It must not invoke the existing `before_publish` callback to implement
PDF cancellation: that callback runs after evaluation and may pause or authorize
target publication. Preserve that boundary and all existing target-file
transaction rules. File-run and filesystem pipeline adapters can pass their
existing cancellation flags through an explicit worker-enabled API. Pending GUI
state must own the extraction manager until closed; a detached Rust thread and
a flag are insufficient closure evidence.

Project previews select their inputs explicitly through
[`ProjectPreviewInputs`](../crates/cli/src/preview_inputs.rs): caller-owned payloads,
which never reach a PDF reader, or the configured files, acquired exactly as a
file run acquires them (the primary once, only selected static sources, dynamic
sources as requested) and optionally through a borrowed `FilesystemPdfReader`.
[Pipeline previews](../crates/cli/src/pipeline_preview.rs) accept payload hosts or
host files. File hosts are read once each, after graph, identity, format, policy
and metadata byte-budget preflight; dynamic named sources stay refused. Previews
return artifacts and publish nothing. Arbitrary editor text and payload library
calls remain in-process.

The GUI has an off-by-default session option to extract file PDFs in the packaged
`ferrule-pdf-worker` beside `ferrule-gui`; a failed admission is an error, never an
in-process fallback. A signal-free [`OwnedPdfReader`](../crates/gui/src/app/pdf_extraction.rs)
owns the manager on the run or preview thread, forwards that thread's single
cancellation flag, and returns only after the job is closed. A job whose closure
is unresolved keeps being serviced, so its thread stays pending. Saved-file runs,
pipeline runs, file-input previews and pipeline previews use it; editor-text
previews stay on the payload route. Cancellation observed before, during or
after extraction wins over a late result, so nothing is published or reported.
Pending runs keep their exact thread until it is joined. An owned preview whose
UI owner is dropped hands its thread to the
[preview service](../crates/gui/src/app/preview_service.rs), and the desktop
runner joins whatever the service still retains after the window closes. App
close is deferred while an owned run or preview, or a retained preview thread, is
pending; the existing dirty-document confirmation follows.
There is no hard process-memory claim for the parent, decoded tree, mapping,
preview retention or serialized targets. See [Memory use and boundary limits](memory-and-limits.md).

The [`pdf-worker` wire codec](../crates/pdf-worker/src/lib.rs) preserves ordered
fields, duplicate data names, empty structures, absent values and Unicode. It
checks the instance length and digest before decoding. If a structured error
is too large, `ErrorEnvelopeTooLarge` identifies its attempted length and digest.

For observed limits on particular workloads, see the [resource profile](performance/pdf-worker-resource-profile.md)
and the [input-limit journal](qualification/pdf-input-journal-2026-10-15.md).
Codec tests, live child closure and whole-process RSS measure different things.
