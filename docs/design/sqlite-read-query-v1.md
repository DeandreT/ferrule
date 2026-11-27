# Declared SQLite reads, version 1

`source_options.sqlite_read` selects a declared read from a local SQLite
file. Ordinary table reads, replacement writes and imported MFD query
lowering have separate contracts. See [declared SQLite queries](../formats.md#declared-sqlite-queries).

## Boundary and ownership

The `sqlite_read_v1` filesystem profile reads one ordinary table in `main`,
materializes one repeating group of scalar fields and closes
its connection before mapping evaluation. It accepts projection, an ordered
conjunction of comparisons, and an explicit total row order. SQL text, SQL
fragments, joins, views, virtual tables, generated columns, subqueries, aggregate
expressions, custom collations/functions, attachments and mutations are excluded.
No network acquisition, driver extension, generated database adapter or external
application compatibility is implied.

The source SchemaNode owns the resulting group/field names and scalar types.
An ordered projection maps each schema field to exactly one physical column;
aliases are schema names, never SQL expressions. The root must repeat; every
child must be a distinct nonrepeating Scalar of String, Int, Float or Bool.
Reject scalar unions, nested groups and unrelated format metadata. The one
admitted annotation is the generated-key marker `import-db` records on a rowid
alias: it may appear only on the Int field projected from the key, where a read
ignores it, and the key proof below still applies. Every field
is present in every returned row. SQL NULL becomes `Instance::Scalar(Value::Null)`;
it never means absent, an omitted field or an empty repeated group. Zero rows
produce `Instance::Repeated([])`. No new nullability flag is inferred from SQL
`NOT NULL`, and the existing scalar NULL policy is retained.

The physical table must have a proved rowid-alias `INTEGER PRIMARY KEY` column.
Reject WITHOUT ROWID, composite keys and the `INTEGER PRIMARY KEY DESC` exception.
Use the existing rowid-primary-key metadata proof as the starting point. Never
fall back to physical scan order, `_rowid_`, a guessed key or an unqualified
unique index. When Ferrule's writer creates a table, it declares a sole
generated column `INTEGER PRIMARY KEY`, so the introspected table carries a
query without schema edits. A table it creates with no generated column, or with
several, declares no key and refuses; an existing table keeps its declaration.
Reject missing or ambiguous case-insensitive table/column matches;
store canonical metadata spellings in the admitted plan. Identifiers are exact
nonempty UTF-8 strings of at most 256 bytes, without NUL; quote them by doubling
double quotes. Punctuation remains literal identifier content.

## Closed saved descriptor

The [complete example](fixtures/sqlite-read-v1/profile.json) is the authored
descriptor exercised by the tests. The additive field is
`source_options.sqlite_read`, or `extra_sources[i].options.sqlite_read` for a
static named source. Target options and dynamic sources reject it. A tagged
body carries `kind`, physical `table`, `key`, `projection`, `parameters`,
`all` and `order_by`. All members are required, including empty arrays;
unknown members, duplicate JSON object keys and unknown kinds/operators refuse.
Do not use a permissive untagged fallback. Limits and cancellation are supplied
by the host, never relaxed by a saved descriptor.

There are at most 128 projected/referenced columns, 32 parameters, 64 predicates
and 128 ordering terms. Identifiers and all arrays must be unique in their own
domain; projection field order must exactly match schema child order. Column
reuse across projection/filter/order is allowed with one agreed scalar type.
Every referenced physical column has a single compatible declared affinity:
String/TEXT, Int/INTEGER, Bool/INTEGER and Float/REAL or INTEGER. An empty, NUMERIC
or BLOB affinity refuses, except that Bool also admits the exact declared type
`BOOLEAN` that Ferrule's writer creates. Its cells still require INTEGER 0/1
storage; other NUMERIC spellings, including `BOOL`, refuse. Type conflicts are
static errors. Types for unprojected
predicate/order columns are explicit in their entries.

Predicate grammar:

```text
all := [predicate, ...]              # AND only; [] is true
predicate := {op: eq|ne|lt|le|gt|ge, column, type, parameter}
           | {op: is_null|is_not_null, column, type}
order_by := [{column, type, direction: asc|desc, nulls: first|last}, ...]
```

Each comparison references exactly one declared parameter of the same type.
No inline values, field-to-field comparison, OR, NOT, LIKE, coercions, implicit
parameters or expression callbacks are admitted. Every declared parameter must
be referenced. Bool permits only eq/ne and NULL tests. All String comparison and
ordering explicitly use BINARY collation, including a table declared NOCASE.
SQL three-valued comparison applies: comparison with a NULL column does not
select the row; explicit NULL tests select NULL/non-NULL. NULL parameters are
refused before database access; use a NULL-test predicate instead.

`order_by` is nonempty and must end with the proved key, Int ascending, nulls
last. The key appears exactly once. Earlier terms select the requested order;
the final key resolves every tie. Always emit explicit directions, NULL placement
and String collation. Preserve duplicate projected values and repeated rows;
do not emit DISTINCT. Both ORDER BY and NULL placement are explicit so the
same query has a repeatable row order.
[SQLite SELECT](https://www.sqlite.org/lang_select.html)

## Parameters and first errors

Parameter names are unique, case-sensitive ASCII identifiers matching
`[A-Za-z_][A-Za-z0-9_]*`, up to 128 bytes. Each names a required runtime input
of exactly the declared type, even if its declaration is otherwise unreachable.
The filesystem CLI resolves `--param name=value` with ordinary runtime
coercion before acquisition; the query API takes typed values. Preview values
and defaults do not bind a query. Values are neither persisted nor interpolated
into SQL or errors.

Preserve existing project/host preflight precedence. Once that passes, the new
boundary has this ordered failure protocol:

1. Validate the descriptor and schema in stored member/array order. Report
   `InvalidQueryProfile { path, reason }`; no filesystem access has occurred.
2. Reject duplicate query-binding names in supplied order, then unknown names
   in supplied order. The library takes an ordered binding list, not a map that
   erases duplicates. An unrelated valid project parameter is not an unknown
   query binding: the host extracts just this query's declared references.
3. Visit query declarations in stored order. For each, choose missing, NULL,
   wrong scalar tag, then nonfinite Float. Report the first error with parameter
   name and expected/found tags, without the value. Finish every binding before
   opening any database, including on empty tables or an always-empty filter.
4. Observe cancellation, then elapsed deadline; then open the existing literal
   path read-only. Do not create a missing database. Relative paths use existing
   project-directory resolution; a `file:` spelling stays a literal filename,
   not a caller-controlled URI or VFS.
5. Admit database metadata and the key in one read snapshot; report the first
   metadata issue in table/key/projection/predicate/order sequence.
6. Validate all referenced candidate cells in key order, columns in first-use
   order (projection, predicates, order). This happens before SQL filtering, so
   a malformed value cannot be hidden by a predicate. NULL is always allowed.
7. Execute the admitted SELECT and materialize rows in declared order. At every
   callback/row/cell boundary, cancellation wins over deadline, then work/byte/
   row limits, then conversion. Preserve SQLite primary and extended error
   codes and source identity; SQLITE_BUSY/LOCKED is a typed acquisition refusal.
8. Finalize statements, end the read transaction and close before returning.
   A close failure refuses success; retain a primary error and attach cleanup
   failures without replacing its cause. Never return a partial input instance.

The [authored host cases](fixtures/sqlite-read-v1/requests.json) freeze
missing/NULL/wrong-tag/unknown/duplicate priority and host-route expectations;
`crates/cli/tests/sqlite_read_query.rs` executes all fourteen.

## Conversion and snapshot

Reuse `format-db`'s existing cell conversion: Int accepts SQLite INTEGER; Bool
accepts only INTEGER 0/1; String accepts UTF-8 TEXT; Float accepts finite REAL or
an INTEGER exactly representable as f64, including exactly representable large
integers. Do not replace the existing significant-bit test with an approximate
2^53 interval. NULL is accepted for every type. No parsing strings, lossy
integer conversion, BLOB projection, NaN/infinity or structured XML columns.
Validate candidate cells, including unprojected filter/order columns, under
these same rules. Candidate validation may scan a column twice only if the
byte/work ledger counts both passes.

Use a private-cache connection and a deferred read transaction. Metadata
inspection establishes the snapshot; candidate validation and SELECT use it.
Revalidation and retry must not silently switch snapshots. Two different source
acquisitions have independent snapshots, even for the same filename. A pipeline
stage does not promise a cross-source or whole-pipeline transaction.
[SQLite isolation](https://www.sqlite.org/isolation.html)

## Read-only, cancellation and finite work

Open with READ_ONLY and private cache, with URI interpretation disabled through
the existing literal-path treatment. Set query_only, trusted_schema off,
extension loading off and temp_store MEMORY using fixed host statements. Refuse
if required configuration cannot be installed. Allow only the host's fixed
metadata/transaction statements before installing an authorizer for the emitted
SELECTs. The authorizer permits SELECT and reads of the admitted table/columns
in `main`, and denies everything else. Keep it installed during execution and
automatic reprepare; do not use IGNORE to mask an unauthorized column as NULL.
[SQLite authorizer](https://www.sqlite.org/c3ref/set_authorizer.html)

Require each compiled candidate/selection statement to be read-only and verify
its parameter count/names, projected columns and absence of trailing statements.
This check supplements the closed descriptor and authorizer: SQLite's read-only
test alone also admits some transaction-control statements.
[Statement read-only check](https://www.sqlite.org/c3ref/stmt_readonly.html)

Read-only means no main-database changes by Ferrule. WAL readers can require
shared-memory sidecars; do not advertise zero directory writes, copy just a main
file from a live WAL database, or set immutable=1 on a mutable file. Refuse WAL
environments that SQLite cannot open read-only, preserving the original error.
[SQLite WAL](https://www.sqlite.org/wal.html)

The maximum policy is 100,000 candidate rows, 100,000 output rows,
1 MiB per cell, 64 MiB cumulative decoded scalar payload across validation and
output, 64 KiB generated SQL, 32 parameters, a 5-second elapsed deadline, zero
busy retries and 10 million approximate VM work units. A host can lower these
limits, never raise them in v1. Count repeated projections and the validation
pass; use checked additions before retaining bytes/nodes. Charge field/group
slots separately (at most 128 fields per row); the byte ledger is not a heap/RSS
bound. Refuse excess output rather than silently truncating it with LIMIT.

Install a progress callback every 1,000 VM instructions before preparation.
Check cancellation/deadline and cumulative work there and around open, metadata,
row and cell operations. The VM interval is approximate, not an exact instruction
bound. Sorting can do work before its first row; cancellation must work there
too. Immediate busy refusal avoids an uncancellable busy wait. SQLite progress
callbacks can interrupt prepare and execution.
[Progress callbacks](https://www.sqlite.org/c3ref/progress_handler.html)

An owned interrupt handle may request cancellation from another thread, with a
lifetime guard that outlives every interrupt call and joins before connection
closure. Never detach a query thread on cancellation. Recheck cancellation after
the last row and before returning success; an interrupt can arrive too late to
stop a nearly completed statement.
[SQLite interrupt](https://www.sqlite.org/c3ref/interrupt.html)

These are cooperative deadlines and finite application work/payload limits;
blocking OS I/O and SQLite internal memory are not hard-bounded by a callback.
Version 1 assumes a trusted local database and working filesystem. Hard wall
time/memory isolation for adversarial database files requires a separately owned
process profile before that claim or activation. Materialized output is not a
streaming promise. No mapping/output publication begins after failed acquisition.

## API, host routing and compatibility

The types live at the mapping/format boundary, outside graph nodes and
portable generated Program IR:

```rust,ignore
// mapping
pub fn validate_sqlite_read_profile(
    schema: &SchemaNode, profile: &SqliteReadProfile,
) -> Result<ValidatedSqliteRead, QueryProfileError>;
// format_db::query
pub fn read_query(
    path: &Path, schema: &SchemaNode, plan: &ValidatedSqliteRead,
    bindings: &[(String, Value)], control: &QueryReadControl<'_>,
) -> Result<Instance, QueryReadError>;
```

Validated plans cannot be deserialized or constructed unchecked. `QueryReadError`
retains source identity, phase, optional parameter/column/one-based candidate
row, a typed kind, SQLite codes and the original cause. Runtime errors do not
invent graph node IDs. Field/value order and the logical failure protocol above
are the cross-host contract; OS-specific error text is retained separately.

Filesystem Run and Preview admit primary and static named query sources.
Bind parameters and the query cancellation control before opening the database;
`before_publish` cannot cancel acquisition. Named acquisition retains its
ordinary order and selected outputs do not widen required-source closure.
Preview returns in-memory artifacts after connection closure.

Payload, standard-stream, REST JSON, pipelines, offline bundles, generated
adapters and MFD export refuse the profile. A host may supply a pre-acquired
instance to an otherwise supported typed mapping. Import does not reinterpret
an MFD query as this descriptor.

Saved projects require the [`sqlite_read_v1` file capability](../project-files.md#file-capabilities).
Older readers refuse rather than silently perform an ordinary table read.
The option is source-only; absent profiles preserve legacy serialization.

## Authored fixtures and implementation gates

The adjacent [SQL seed](fixtures/sqlite-read-v1/seed.sql),
[SQL/typed oracles](fixtures/sqlite-read-v1/sql-cases.json) and
[verifier](fixtures/sqlite-read-v1/verify.py) create a fresh database locally.
They demonstrate ties, duplicate projected values, explicit NULL ordering,
BINARY text comparison despite NOCASE declaration, Boolean/Float/NULL tags,
quoted parameter text, an empty result, read-only write refusal and a WAL
snapshot across a concurrent committed insert. The verifier captures complete
outcomes before assertions. It verifies SQLite premises; the mapping,
`format-db` and CLI tests verify the Ferrule model, parameter admission, source
closure and bounded cancellation against the same fixtures. Their
[complete profile result](fixtures/sqlite-read-v1/profile-output.json) uses
explicit `repeated`/`group`/scalar-tag notation for independent logical comparison;
it is not the current Instance serde wire format.

The [CLI regression](../../crates/cli/tests/sqlite_read_query.rs) exercises
the public routes, parameter error precedence, restricted access and
cancellation. The SQL verifier alone cannot establish host behavior.
