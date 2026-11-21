# Design-time stored procedure recordsets

Two actions in **Edit Recordset Structures** are separate from running a saved
mapping. The [four cases](recordset-structure.tsv) select manual definition
and execution-based discovery in the two documented mapping roles: **Show
Nodes as Source** and **Insert Call with Parameters**.

| Ledger ID | Input and observable difference | Installer manual |
| --- | --- | --- |
| `authoring.define-procedure-recordset` | Enter the expected resultsets and columns. The authored structure can be wired without invoking the procedure at design time. | §4.4.6.1 p. 327; §4.4.6.2 p. 329; §4.4.6.3 p. 333 |
| `authoring.discover-procedure-recordset` | Explicitly invoke the procedure from the designer to obtain the returned shape. The source example requires no argument; the Call example supplies trial arguments. After confirmation the structure appears both in the dialog and on the mapping. | §4.4.6.1 p. 327; §4.4.6.2 pp. 328–330; §4.4.6.3 pp. 333–334 |

The [source receipt](oracles/recordset-structure-source.json) records numbered
topic and physical-page anchors from the pinned English Enterprise 2026r2
x64 installer manual, not the public PDF. It is a documentation receipt,
not a run log. The cases don't claim a detected or hand-entered layout
validates all runtime results. Multiple-resultset support depends on the
driver/API and is limited to a fixed count and fixed column structure
(§4.4.6 p. 324). The PostgreSQL **function** note on p. 325 describes using
declared output-parameter metadata without either action. It is a
database-specific boundary, not a third operation in this slice.

The manual warns that design-time invocation can have procedure-dependent
side effects. Trial arguments in the evaluation dialog obtain a structure;
the example wires the mapping's own arguments separately. The runtime target
mode is not an additional case, nor is a procedure's return value or OUT
parameter a declaration of its recordset columns.

**Matrix boundary.** §20.1.2 p. 1606 row 4, `Database stored procedures`, is
runtime context only. It has a BUILT-IN mark but neither design-time action
has its own matrix row. No individual environment inherits a mark, a blank,
or a `not_applicable` assessment from that row. The two native-profile IDs
were added as unassessed on all 32 cells, with no execution evidence. The
821 prior records and their evidence remain unchanged: the inventory now
has **823** records and `inventory_complete=false`.

Use `python -B verify_recordset_structure.py` for the live checks. Pass
`--pdf PATH` to verify the exact PDF hash, topic bookmarks and citation text
and to recheck the original graphical matrix marks. Pass
`--check-introduction` in a Git checkout to compare the historical ledger at
`b9a847ab` with its exact parent `2fc81739` and verify that only the two new
unassessed rows were introduced then.
Run `python -B -m unittest test_live_inventory` for the negative controls.
No procedure, mapping, vendor application or compiler was run for this slice.
