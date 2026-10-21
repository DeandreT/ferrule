# Primary CSV acquisition

Primary filename evaluation, source selection, loading, parsing and mapping are
separate operations. `engine::acquire_primary_csv` connects the first four for
an explicitly selected CSV input. Generated libraries can opt in with
`emit_with_primary_csv_acquisition`, which adds `acquire_primary_csv` in Rust
and `GeneratedMapping.AcquirePrimaryCsv` in C#.

These APIs return row occurrences, not mapped output. They do not enable an MFD
executable profile, change ordinary instance traversal, perform grouping, write
files or publish output. Flattening the rows into unrelated values would discard
the identity needed by later cross-document mapping.

## Host boundary and evaluation order

The host supplies a selector and a byte loader. The selector receives the exact
evaluated filename once and returns requests in its chosen order. The loader
returns bytes and an optional resolved location for each reached request.
Neither library interprets wildcard syntax, normalizes a path, accesses a
filesystem or starts network access. A host must explicitly enforce its own
selection, authorization and resource policies.

Construction validates the document-count limit but does not evaluate the
filename or call the host. On the first iteration:

1. Evaluate the primary filename without using saved previews.
2. Call the selector once.
3. Request one selected path and check the document limit and path shape.
4. Load and parse that complete physical document.
5. Return its rows in order before requesting another document.

An empty document consumes one document slot. Repeated equal paths are distinct
loads and parses, with distinct document identity. Each row retains its owning
document, exact request path, optional resolved path and one-based local data-row
position. A retained row can outlive the iterator.

## Failures and bounds

Filename, selection, request-enumeration, loading and input failures retain their
stage and original cause. Input failures include invalid selected paths and CSV
parse/decoding errors. The first reached error or callback unwind permanently
ends the attempt; later requests are not read. A malformed document exposes none
of its rows, while rows already returned from earlier documents remain valid.
Retry requires a new attempt and an explicitly chosen request set.

CSV parsing retains the existing per-document input, decoded-byte, field, row
and node limits. Strict UTF-8 stays the default reader contract; the acquisition
caller explicitly supplies its byte-decoding policy. Host allocations before
returning bytes and retained rows across documents are not covered by a total
process-memory ceiling. This acquisition boundary does not provide order-level
checkpointing or replace an integration's independent-order failure policy.

Generated companions require the existing closed flat CSV schema and explicit
input policy. They reuse the ordinary filename evaluator and the same Rust/C#
acquisition state machine. Ordinary generated entry points and static document
adapters retain their existing behavior.
