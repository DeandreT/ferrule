# PDF worker resource profile: 2026-10-22

This report compares the packaged Linux PDF worker with in-process extraction.
It uses ten self-authored inputs. Nine produced the same complete outcome both
ways in every repeat. The tenth, a compressed page that inflates to 320 MiB,
completed in-process with a peak resident set of about 710 MiB. In the worker,
a failed allocation aborted the child. The report is a finite observation on one
host. It changes no default or fixed limit, and it does not qualify arbitrary or
hostile PDFs. It also claims no parser-exclusive memory, resident-memory ceiling
or streaming behavior. Scope: [#328](https://github.com/DeandreT/ferrule/issues/328).

## Reproduce

```sh
cargo build --release --locked -p cli --bins --example pdf_worker_profile
target/release/examples/pdf_worker_profile --repeats 10 --raw trials.jsonl > profile.md
```

The [program](../../crates/cli/examples/pdf_worker_profile/main.rs) generates
the fixtures into a private temporary directory. It copies the built worker into
a private package directory because Cargo hard-links build outputs and worker
admission requires a single-link file. It then runs every trial in a fresh
process of itself and prints the tables summarized below. `--raw` keeps one JSON
record per trial; `--fixtures a,b` selects fixtures. Fixtures and modes are
interleaved within each repeat.

## Environment and build

| Item | Value |
| --- | --- |
| Host | Linux x86_64, kernel `7.2.7-zen1-1-zen`, AMD Ryzen 7 5800X3D (16 logical CPUs), MemTotal 32,773,216 KiB |
| Toolchain | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1` |
| Source | Base `4a2f8605`, plus the program and test changes that accompany this report; `Cargo.lock` SHA-256 `6fe9d63f20e344bc61d1ebaaeba5891dedec5ab80c0213f8bac565a353dc4f59` |
| Build | Release profile, `--locked`, two build jobs |
| Worker | `ferrule-pdf-worker`, 4,669,736 bytes, SHA-256 `7c4d408c08ef266447e67f6f036d9bfaa81b8e4ba92cbc04d5cdafa42b7e11bb` |
| Effective limits | Reported in the Ready frame of every launched worker: `RLIMIT_AS` 536,870,912 bytes (soft and hard); `RLIMIT_CPU` 30 s soft, 31 s hard; `RLIMIT_FSIZE` 67,108,864 bytes; `RLIMIT_CORE` 0 |
| Owner deadlines | 30 s work and 5 s stop allowance (unchanged) |
| Runs | Three runs of 10 repeats (310 trials each) |
| Host load | One-minute load averages were 54→33, 29→22 and 18→14 during the three runs. The host was shared with other builds. |

All three runs produced identical outcome tables. Memory values above 16 MiB
agree within 7% across runs, and smaller values vary by up to 1.3 MiB. The
tables below come from the third and quietest run. The first two
runs had wider wall-time spreads: some worker trials took 7–13 s.

## Fixtures and modes

| Fixture | Bytes | Layout | Written expectation |
| --- | ---: | --- | --- |
| `empty-page` | 431 | first-page capture | `Value` = Null |
| `hello-page` | 578 | first-page capture | `Value` = `Hello` |
| `pages-256` | 69,244 | group per page | 256 groups, `Page 001`…`Page 256` |
| `pages-257` | 69,514 | group per page | `PdfError::TooManyPages` |
| `wide-text` | 68,165 | first-page capture | 256 lines × 32 words, joined by spaces and newlines |
| `flate-32m` | 33,233 | first-page capture | `Value` = `Hello` after 32 MiB of inflated spaces |
| `flate-320m` | 326,762 | first-page capture | Observed only (320 MiB of inflated spaces) |
| `late-invalid` | 847 | group per page | Observed only (page 2's Flate stream is corrupt) |
| `truncated` | 47 | first-page capture | `PdfError::InvalidPdf` |
| `input-over` | 8,388,609 | first-page capture | `PdfError::InputTooLarge` |

| Fixture | SHA-256 |
| --- | --- |
| `empty-page` | `f4f187fef575e0437fa83ff7e1de7cad4c25a947534a918bca84c90cbc29b2ac` |
| `hello-page` | `a0e1d43d4b6c4a36c9ebd8c87996fc002af01877a57be8e4a41a4a018e20031a` |
| `pages-256` | `50c7d1422e63b7edc24476879ad43173fcfd271603e7b2a714a0e5072a322628` |
| `pages-257` | `24449721053ecc3a4da8cda65c24d52c0f080cac833f3e2198e73d2d2089ad57` |
| `wide-text` | `1c6401f2500d44bf093e7975fc7ddd996971e845b7b5763651a183db34ce9ffb` |
| `flate-32m` | `fae0620fb95c35afa579d47116374612ee10828cd4644ba67ffaf9830bfed853` |
| `flate-320m` | `404db3f671cde41b94585262980be45654f81c6b02325c91daf4304c4f17d683` |
| `late-invalid` | `17002da22479947adc68778682d6126428c5bfbd60e82c567f8ecd89d34920f8` |
| `truncated` | `2ab4f6335c02e6e755401af5246eb07cc5e1927aef8bff889a03904e61fa75be` |
| `input-over` | `b2159155f8ab94cf0a39d4421db20a261747df17efdaff029bbcacc674df57e8` |

The two one-page fixtures are the existing `crates/pdf-worker/tests/fixtures`
files. The others are generated deterministically in
[`fixtures.rs`](../../crates/cli/examples/pdf_worker_profile/fixtures.rs), and
the Flate streams use `flate2` at its best compression level.

| Mode | What runs in the trial process |
| --- | --- |
| `in-process` | `format_pdf::read`, the CLI's default in-process path |
| `in-process-as512` | The same call after the trial lowers only its own `RLIMIT_AS` to 512 MiB; this is the address-space control |
| `worker` | `PdfWorkerManager` with the packaged worker, successful originals not retained (the CLI default), polled every 1 ms; a failed job is then removed with `cleanup_closed_failure` |
| `worker-ballast` | `hello-page` through the worker while the owner holds 256 MiB of touched memory |

## Outcomes

Outcomes were identical across all ten repeats of every cell. Typed trees are
identified by the byte length and SHA-256 of their complete worker-wire
encoding.

| Fixture | In-process | Worker | Worker child | Same complete outcome |
| --- | --- | --- | --- | --- |
| `empty-page` | tree T1 | same | exit 0 | yes, matches expectation |
| `hello-page` | tree T2 | same | exit 0 | yes, matches expectation |
| `pages-256` | tree T3 | same | exit 0 | yes, matches expectation |
| `pages-257` | `PdfError::TooManyPages` | same | exit 2 | yes, matches expectation |
| `wide-text` | tree T4 | same | exit 0 | yes, matches expectation |
| `flate-32m` | tree T2 | same | exit 0 | yes, matches expectation |
| `flate-320m` | tree T2 | `WorkerExit`, worker terminated by signal | SIGABRT | **no** |
| `late-invalid` | tree T5: page 1 `Hello`, page 2 Null | same | exit 0 | yes, observed |
| `truncated` | `PdfError::InvalidPdf` | same | exit 2 | yes, matches expectation |
| `input-over` | `PdfError::InputTooLarge` | the owner's typed `InputTooLarge` before any child | none | yes, matches expectation |

| Tree | Encoded bytes | SHA-256 |
| --- | ---: | --- |
| T1 | 53 | `e234b9df28ef8db70d45c225ed9588d943fd00f6d672be244af5db9287f201f5` |
| T2 | 71 | `26568ddac894b6853afc5c1b41373cc9ea7ab2ed567196deb495aceb085abc61` |
| T3 | 19,266 | `3c62bd9f9801a403ea5d04aadd680c1fd5e3656df0b109b4e9f26fb374100f47` |
| T4 | 65,856 | `304e5b33d735432fb3eb01ff1f5b59907565fdd2bd562c7aeda013fb6fa1895e` |
| T5 | 192 | `a0015707141647ead104c4f48b4dc7685e6e2f1953fae0ea94c599f330c2c2ab` |

The `in-process-as512` mode produced the in-process outcome for every fixture
except `flate-320m`.

**Recorded failure.** For `flate-320m`, the worker's captured stderr was
`memory allocation of 334020608 bytes failed`. The child terminated by signal 6
(SIGABRT), and the owner closed the job as `WorkerExit`. The in-process call completed. With
only its own address space lowered to 512 MiB, the in-process trial aborted with
the identical allocation message. This matched control is consistent with the
address-space limit refusing that allocation. The signal alone does not
establish this, and the allocating component was not identified. The corrupt
second page of `late-invalid` produced a Null capture rather than an error in
both paths.

No timeout, cancellation, resource-setup refusal, `ClosureIncomplete` poll or
cleanup failure occurred in the 930 trials.

## Memory

Values are KiB, minimum–maximum over ten repeats.

| Fixture | In-process VmHWM | In-process VmPeak | Owner VmHWM in worker trials | Worker own-reap `ru_maxrss` | Worker value above owner VmHWM |
| --- | ---: | ---: | ---: | ---: | --- |
| `empty-page` | 5,328–5,576 | 8,820 | 4,688–4,880 | 4,904–5,860 | yes |
| `hello-page` | 5,516–5,832 | 8,820 | 4,780–4,912 | 4,976–6,264 | yes |
| `pages-256` | 7,020–7,268 | 10,304 | 4,884–5,036 | 6,504–6,916 | yes |
| `pages-257` | 6,324–6,492 | 10,120 | 4,720–4,908 | 5,908–6,480 | yes |
| `wide-text` | 16,644–17,192 | 20,448 | 4,980–5,132 | 16,288–17,428 | yes |
| `flate-32m` | 87,128–89,328 | 107,012 | 4,724–4,952 | 87,124–89,004 | yes |
| `flate-320m` | 726,524–728,876 | 989,532 | 4,720–5,004 | 331,448–332,052 (aborted) | yes |
| `late-invalid` | 5,756–6,016 | 9,004 | 4,756–4,928 | 5,032–5,804 | yes |
| `truncated` | 5,004–5,276 | 8,820 | 4,616–4,868 | 4,572–4,928 | mixed |
| `input-over` | 4,388–4,548 | 8,820 | 4,644–4,852 | no child | — |
| `hello-page`, 256 MiB ballast | — | — | 266,508–266,924 | 266,420–266,796 | no |

`VmHWM` is the trial's own peak resident set and `VmPeak` its peak virtual
address space. Both come from `/proc/self/status` at the end of the trial.
`RLIMIT_AS` admits address space, not resident memory. In `flate-320m`, the
in-process `VmPeak` exceeded the worker's 524,288-KiB address-space limit,
while the aborted worker's own-reap RSS stayed below it.

Neither `ru_maxrss` value is an independent peak:

- Every trial whose own `VmHWM` stayed below 14,072 KiB reported exactly
  14,072 KiB, which is consistent with the driver's high-water mark rather than
  the trial's peak.
- In the ballast row, the worker reported the owner's 256 MiB.

Linux `ru_maxrss` is a process-lifetime value that persists across `execve`.
These observations are consistent with that value starting from the spawning
process's high-water resident set: std launches through a spawn that shares the
parent's address space until exec. A worker's own-reap value is therefore at
least the owner's high-water at launch.

- If the worker value exceeds the owner's whole-trial `VmHWM`, it is the worker
  image's own peak.
- Otherwise, it bounds the worker's peak only from above. The `truncated` row is
  mixed, and the ballast row is upper-bound only.

Do not add owner and worker values or subtract one from another. None of these
high-water marks is attributed to the parser.

## Time

Values are milliseconds, median with minimum–maximum.

| Fixture | In-process extraction wall | Worker job wall, begin to terminal | Owner CPU in job | Worker CPU (own reap) |
| --- | ---: | ---: | ---: | ---: |
| `empty-page` | 0.1 (0.1–2.7) | 1,152 (658–5,713) | 20.1 (17.6–29.9) | 2.7 (2.2–3.2) |
| `hello-page` | 0.2 (0.2–29.2) | 1,438 (644–2,537) | 20.7 (19.4–25.9) | 2.9 (2.7–3.1) |
| `pages-256` | 5.3 (4.6–18.7) | 1,424 (725–3,841) | 22.5 (19.9–25.2) | 35.8 (31.7–38.8) |
| `pages-257` | 2.9 (2.2–15.4) | 324 (209–1,022) | 12.4 (12.0–13.8) | 5.1 (4.4–5.6) |
| `wide-text` | 19.8 (17.8–39.2) | 1,183 (604–2,753) | 22.0 (20.5–24.8) | 25.2 (22.5–31.9) |
| `flate-32m` | 97.7 (46.3–117.0) | 1,471 (666–3,222) | 24.0 (20.6–26.7) | 101.7 (48.6–112.7) |
| `flate-320m` | 976 (535–1,048) | 616 (276–778), aborted | 15.9 (14.7–17.7) | 104.8 (68.9–208.0) |
| `late-invalid` | 0.4 (0.3–1.9) | 1,605 (571–1,939) | 20.4 (18.8–22.0) | 3.2 (2.9–3.4) |
| `truncated` | 0.1 (0.1–0.1) | 288 (171–626) | 12.5 (11.5–13.7) | 1.9 (1.8–2.2) |
| `input-over` | 0.0 (0.0–5.4) | 103 (51–617), no child | 6.6 (6.2–9.7) | — |

Wall brackets are monotonic `Instant` intervals. The in-process bracket covers
only `format_pdf::read`. The worker bracket runs from `begin` to terminal
progress. CPU is `getrusage(RUSAGE_SELF)` user plus system time over the same
bracket, together with the worker's own-reap usage.

The owner's receipt timeline locates the worker's wall time. Medians across the
successful jobs were:

| Phase | Median |
| --- | --- |
| Begin to spawn | 118–138 ms |
| Spawn to Ready | 20–40 ms |
| Ready to own reap | 100–270 ms |
| Reap to terminal receipt | 800–1,070 ms (68–78% of the job) |

After the reap, a successful job validates the result and instance and publishes
its receipt. It then removes the job, writing and synchronizing one
cleanup-intent record per removed leaf (seven for a successful job). A failed job keeps its directory until the
caller cleans it after the terminal result, outside this bracket. That is why
failed jobs close sooner.

These timings are on a loaded host and are not benchmarks. They do not separate
scheduling delay from file synchronization latency.

## Job budgets and closure

| Fixture | Job leaves before cleanup, bytes | Removed by cleanup |
| --- | --- | --- |
| successful jobs | `control.bin` 251; `input.pdf` (= input); `instance.json` 53–65,856; `request.json` 652–920; `result.json` 248–251; `stderr.bin` 0 | the six leaves, `receipt.json` and `cleanup-0…6.json` |
| `pages-257`, `truncated` | as above without `instance.json`; `result.json` 316 / 414 | five leaves, `receipt.json`, `cleanup-0…5.json` |
| `flate-320m` | `control.bin` 251, `input.pdf` 326,762, `request.json` 655, `stderr.bin` 122 | four leaves, `receipt.json`, `cleanup-0…4.json` |
| `input-over` | none; no snapshot, request or child | `receipt.json`, `cleanup-0.json` |

These are far below the fixed limits:

| Item | Largest observed | Fixed limit |
| --- | ---: | ---: |
| Instance | 65,856 bytes | 64 MiB |
| Request | 920 bytes | 4 MiB |
| Result | 414 bytes | 64 KiB |
| Stderr | 122 bytes | 64 KiB |
| Logical job size | under 140 KB | 80 MiB |

The 96 MiB scratch headroom was admitted for every job. Across all 930 trials,
every job directory was removed and the scratch parent was empty afterwards. The
open-descriptor count after dropping the manager equalled the count before it
was created.

The exact and one-over budget boundaries and the incomplete-closure paths need
controlled children rather than PDFs. They are exercised by the plain tests in
`cargo test -p pdf-worker`:

- `byte_boundary_host` covers these budgets:
  - Input: exactly 8 MiB is admitted. One byte over is refused before launch,
    leaving only `receipt.json`, with no spawn, Ready limits or reap usage.
  - Instance: exactly 64 MiB is admitted. One byte over is refused by the
    encoder as `InstanceTooLarge`, or stopped by `RLIMIT_FSIZE` (SIGXFSZ).
  - Logical job size: exactly 80 MiB and one byte over.
  - Stderr: exactly 64 KiB and one byte over.
  - Receipt-leaf collisions.
  - The 96 MiB scratch reservation.
  - Unsafe scratch parents.
- `protocol_boundary_host` covers request, Ready and result frame refusals,
  cancellation between Ready and Start, unpublished partial output after
  abnormal exits, and changed snapshots.
- `startup_host` and `inheritance_host` cover packaged-worker admission,
  inherited CPU and file-size limits, SIGCHLD disposition, dropped descriptors
  and environment, and the worker's death with its owner.
- The `ownership_fault351`, `terminal366`, `escalation358` and `budget363`
  unit tests cover `ClosureIncomplete` retention, physical TERM-to-KILL within
  one second, terminal-record publication and the 30 s cutoff with modeled
  clocks. No physical 30 s timeout ran in this profile.

These refusal tests check owned descriptor closure with a census of
`/proc/self/fd` before and after. No fixed limit was raised to make a case pass.

## Limits of this report

- One host, one build and ten finite self-authored inputs, run under heavy
  shared load. Outcomes are exact. Memory and time are brackets for these
  inputs, not bounds for other PDFs.
- `VmHWM`, `VmPeak` and `ru_maxrss` are high-water marks, not sampled current
  values. The worker value is bounded by the owner's high-water at launch, as
  described above.
- The profile does not use the inputs of the earlier
  [memory observations](pdf-worker-memory.md), so it makes no improvement claim
  against them.
- Any default-policy change, such as enabling the worker by default or altering
  a fixed limit, needs its own review. This report supports neither.
