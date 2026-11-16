# Read-only Ferrule host API

Use `ferrule_host::load` to decode the exact native project bytes with a caller-selected input budget. Pass the project's model and file layout to `inspect` for structure and unresolved resource references or to `validate` for typed, ordered engine diagnostics. A host can also pass an in-memory project and the layout of its corresponding native snapshot.

Inspection counts dependencies and resource references as well as structural nodes. Validation applies that same preflight before invoking the engine. Diagnostic emission and UTF-8 display text have separate limits. A shortened display value carries a truncation flag. Native locators retain exact IDs or fall back to the document when a pointer is too long or its representation is uncertain. The API treats saved resource references as metadata.

## Worker parity fixtures

`tests/fixtures/orchard-inventory.json` covers a legacy project with primary and named boundaries, graph dependencies, nested scopes, and all eight unresolved resource kinds. `tests/fixtures/versioned-ledger.json` covers a versioned project with an exact integer, negative zero, and diagnostics owned by functions with IDs above JavaScript’s safe-integer range.

Run `cargo test --locked -p ferrule-host --test worker_parity` for the shared API assertions. To qualify a downstream worker migration, replay the original fixture bytes through both adapters at the same engine revision. Compare the complete framed replies, including the orchard inspection with an item budget of 35 and the ledger validation with an emitted diagnostic budget of 1.
