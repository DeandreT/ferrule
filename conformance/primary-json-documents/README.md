# Strict JSON primary filename documents

The authored grouped mapping shared with the XML cohort runs once over a direct
JSON document or a wildcard document set. `cases.json` specifies complete output,
typed failures and ordered host calls independently of the implementations.

The cases cover direct and saved-path identities, cross-document grouping and
aggregation in Unicode filename order, empty selection refusal and mapping,
parse and typed-value failures before later reads, missing parameters, URI
refusal, document and byte limits, current document paths, and strict rejection
of multiple top-level JSON values. JSON5 and JSON Lines are not admitted.

The CLI `primary_json_documents` tests compare interpreter and generated Rust/C#
results. The ignored generated-host test requires Cargo and .NET 10. All files
are self-authored; no external sample corpus is needed.

The interpreter and generated hosts also construct seventy distinct output
groups with a one-million-byte status value to verify a complete output over
64 MiB is refused. Separate boundary tests charge exact per-document and
aggregate input limits without requiring oversized fixture files.
