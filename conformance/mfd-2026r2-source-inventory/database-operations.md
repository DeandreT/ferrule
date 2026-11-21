# Database target actions and stored procedure calls

The [operation cases](database-operations.tsv) give **six native-profile
capabilities** separate assessment slots. They resolve four previously broad
[matrix rows](installer-support-matrices.csv) next to the already itemized
[custom SELECTs](select-workbook.md). The source is the pinned English
Enterprise 2026r2 x64 installer manual (executable version `28.2.0.0`); the
pages below are physical pages in that PDF, not in the public download.

| Ledger ID | What distinguishes the operation | §20.1.2 p. 1606 |
| --- | --- | --- |
| `connectors.db-bulk-insert` | Pass multiple inserts as a batch, not just ordinary `Insert All`. §4.4.3.1 pp. 291–292. | Row 1: BUILT-IN; actual bulk transfer requires MapForce Server. |
| `connectors.stored-procedure-source` | Read a procedure's recordset or output parameters as a source, without a mapped procedure argument. §4.4.6.2 pp. 328–331. | Row 4: BUILT-IN only. |
| `connectors.stored-procedure-call` | Bind input arguments and map resultsets, return value or OUT parameters from the other side of a function-like call. §4.4.6.3 pp. 331–335. | Row 4: BUILT-IN only. |
| `connectors.stored-procedure-target` | Invoke with mapped inputs as a database target. Optional pre-call SQL and rollback are target settings; a local relation can pass a generated key to a dependent insert. §4.4.6.4 pp. 335–338 and §4.4.6.7 pp. 343–347. | Row 4: BUILT-IN only. |
| `connectors.db-transaction-rollback` | Configure transaction scope and stop/continue on database errors; current and top differ when parent and child transactions are enabled. §4.4.2.1 p. 265, §4.4.3.1 pp. 289–290 and §4.4.3.3 pp. 297–302. | Row 5: BUILT-IN, C++, C#, Java; note 3 on p. 1607 requires the connected driver to support the selected language. |
| `connectors.db-action-trace` | Produce the XML database-action trace for a write, with hierarchical event/field filtering and driver error details. §4.4.2.1 pp. 265–268, §4.4.3.1 p. 290 and §4.4.6.4 pp. 337–338. | Row 6: BUILT-IN only. |

**Bulk transfer.** Design configuration does not perform the transfer.
MapForce Server must run the mapping (standalone or managed by FlowForce), the
server license must not be limited to single-thread execution on a multi-core
machine, the table must use `Insert All`, and no related table, view or procedure
in the mapping may reference it. The manual also requires a driver that supports
bulk insert in `WHERE` conditions. Its table explicitly marks native PostgreSQL
Yes and native SQLite No; MySQL and MariaDB JDBC require the stated connection
option (`rewriteBatchedStatements=true`). Bulk transfer and the table's
`Use transactions` option are mutually exclusive. These are release
documentation conditions, not claims about
Ferrule's ordinary SQLite writes or vendor preview execution.

**Stored procedure modes.** §4.4.6 pp. 323–325 explicitly describes the three
roles; the common matrix mark does not collapse them. The source and
parameterized-call roles do not support the target role's transaction option.
The manual does not let a mapped call omit an argument to use a server default.
Multiple recordsets depend on the driver/API and require a fixed count and
column structure; SQLite has no stored procedures. Defining a recordset schema
manually and executing a procedure at design time to discover it are separate
choices (§4.4.6.1 p. 327). Neither discovery nor target preview is treated as
a receipt. Target-local relations and duplicate inputs are documented
conditions of target calls in this slice, not additional assessment IDs.

**Rollback and trace.** The rollback mark concerns database errors, not
arbitrary graph exceptions or a promise that a generated program has the
designer's interactive transaction dialog. At target-component level the
manual distinguishes one preview target from the sequential targets of Server
or generated-program execution. Trace generation is narrower than rollback:
an error-only parent prevents full child tracing; a trace's error text, code
and state come from the connected driver. These restrictions do not transfer
to Ferrule's general trace inspector.

The prior `connectors.enterprise-databases` and `connectors.sqlite` IDs remain
connector families. The stored procedure rules do not extend to
`connectors.select-without-inputs` or `connectors.select-with-inputs`. Existing
[mapping](mapping-contracts.tsv) and [workflow](workflow-contracts.tsv) examples
remain source contracts, not counted as additional capabilities. No database
driver, server, generated code or vendor mode was executed for these cases.

The ledger has **821 records at addition**, versus 815 at `b99891ed`. All
**96 new cells** (48 Ferrule and 48 vendor) are unassessed with no execution
evidence. The table records documented applicability only; blanks are not new
`not_applicable` assessments. Existing assessments and the original matrix
remain unchanged. `inventory_complete=false`; normal table-action CRUD, MERGE,
recordset discovery and other database contracts are not exhausted by this group.

Run `python -B verify_database_operations.py` for the joins and cell checks.
Pass `--pdf PATH` to verify the pinned hash and in-section anchors. Run
`python -B verify_installer_matrices.py --pdf PATH` to verify the graphical
marks separately, and `python -B -m unittest test_live_inventory` for mutation
controls. The examples are documentation-derived expectations, not run results.
