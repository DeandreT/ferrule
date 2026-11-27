# PDF input-limit observation journal

Observation for [#387](https://github.com/DeandreT/ferrule/issues/387),
2026-10-15. This fixture records staging and custody of the bounded input; it
does not parse a PDF or qualify the worker's parser memory.

## Capture policy

Each case and the campaign root have one exclusively created `raw.jsonl`.
Records retain the original ordered sequence/operation/raw envelope. Each log
admits at most 8,192 records and 8 MiB; the shared, non-recycled campaign ledger
admits 65,536 records and 64 MiB across at most seventeen logs. The newline is
charged. Each encoded record is capped at 256 KiB, with a 64 KiB write buffer.
Serialization refuses before growing the encoded body beyond its cap; Vec
capacity, the caller's existing JSON values and allocator overhead are separate
from encoded payload accounting.

Each log additionally reserves at most 1 MiB for one exclusive refusal original.
That record preserves a budget/write failure's triggering envelope and the exact
still-buffered bytes. Encoding overflow, failed storage or changed custody
refuses qualification, even if the independent refusal original cannot be
written. A refused journal is never reported as complete. Dependent fixture
work stops, no subsequent physical row starts, and already active manager
ownership still has to close. The campaign records incomplete capture and skips
semantic qualification. This is not a promise that exhausted/broken storage can
retain an unlimited subsequent event stream.

Successful checkpoints flush and sync the log, verify held/named inode, owner,
mode, link count and exact charged length, and sync its directory. Checkpoints
precede custody-dependent cleanup and expected comparisons. Finalization freezes
the file to 0444, captures its complete hash/identity, and writes a small exclusive
summary original. Seventeen summaries and the retained frozen expectation have
separate bounded metadata overhead; they are not charged as operation records.
The existing streamed PDF/job/terminal originals retain their own independent
file caps and hashes. This log quota is not a whole-fixture disk/RSS bound.

This replaces per-operation file creation and synchronization with buffered
appends and semantic checkpoints. Durability means the filesystem's successful
file/directory sync contract; no network filesystem, storage crash or power-loss
experiment ran.

## Reached checks

Base: `7ff83194882f49285b47d14bdc44464d44bf8dbe`.
Linux x86_64 GNU, Rust `1.97.0-nightly (9838411cb 2026-04-25)`, locked dependencies,
two build jobs, incremental disabled, dev/test debug information disabled.

The packaged CLI and sibling worker completed the sixteen frozen fixture rows.
An independent replay checked all seventeen journal hashes, byte lengths and
unbroken sequences. Setting `FERRULE_PDF_387_CASE_RECORD_LIMIT=2` produced the
expected refusal on the third operation before manager start or semantic
comparison. This test-only override can lower, never raise, the positive limit.

The five focused tests cover ordered full-value records, checkpoints/sealing,
exclusive creation, case and campaign count limits, byte/encoding overflow,
distinct-case shared accounting, the seventeen-log ceiling, a real buffered
write/flush error and a replaced named alias. Failed/incomplete logs cannot
produce a successful summary.

Final physical-run accounting:

| Quantity | Observed |
| --- | ---: |
| Operation journals | 17 |
| Complete ordered operation records | 36,468 |
| Encoded journal bytes | 4,758,261 |
| Largest individual log | 4,981 records / 661,047 bytes |
| All retained/reached files under this run root | 99 |
| Logical bytes across those files | 298,451,560 |
| Separately streamed `.original` files | 39 / 184,591,878 bytes |

These are actual logical sizes of this run, including sparse source/job files
and metadata; they are not allocated disk-block usage, an all-future-run bound
or a nominal estimate. The scope does not parse a PDF or measure parser memory.

The physical fixture required a single-link packaged worker; the hard-linked
Cargo artifact was refused. The recorded hashes identify the exact sources:

| File | SHA-256 |
| --- | --- |
| `stage_fault382.rs` | `0150237bc9cfb7aa75cd6b1fa82a49058162119b0e15682b4457098d2eb887ea` |
| `stage_fault382/journal.rs` | `5e8e41346915ae7e50a5afe202d5548a03633dd4e26a377e275c8be805a6ca98` |
| Unchanged `stage_fault382.expected.json` | `1ddce0b7cd306d3b35b3ef7cb6f8473681d2c29e9457f409838b9bbbdf6a2a14` |
