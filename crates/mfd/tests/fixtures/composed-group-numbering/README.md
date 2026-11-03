# Imported nested starting groups with positional numbering

This independently authored, synthetic package covers one composition from
#1306: optional repeated `Batch/Row` feeds `group-starting-with` inside each
repeated target `Batch`, with positional `auto-number` starting at 3 and stepping
by 4. It has no global counter identifier or restart-on-change input.

The first row starts a group even when `Start` is false; each later true row
starts the next group. Scalar reads select its first member, count uses all its
members, and numbering uses the compact group ordinal in the current batch.
The contract is [Ferrule's starting-boundary contract](../../../../../docs/design/owned-group-starting-with.md),
not a vendor-parity claim. The MFD, schemas, inputs and full expected documents
were written from that contract, not obtained from customer or vendor material.

`cases.json` freezes full source and expected document trees, including retained
empty batches, consecutive boundaries, trailing ordinary members and numbering
reset after an empty middle batch. Revisited inputs follow different calls to
check fresh invocation state. The native test separately checks malformed
Boolean input as `ScalarParse` before engine evaluation and a missing boundary
edge as an executable-import refusal, not a successful empty result.

The ordinary test lowers and emits both existing backends but does not compile
them. The ignored `generated_release_hosts_match_complete_documents_and_fresh_invocations`
test reuses the maintained CLI host-isolation/retention harness, builds dedicated
Rust and C# Release hosts, and compares each complete document with the same
literal oracle. It needs Cargo, cached dependencies and the pinned .NET SDK.
Set `TMPDIR` to disk-backed storage; `FERRULE_CODEGEN_KEEP_ARTIFACTS=1` retains
generated sources and original command/status/output records for diagnosis.
Generated hosts use typed JSON inputs; the physical XML failure is a separate
native adapter assertion. Compiling or running these hosts does not execute any
external reference application.
