# Custom SELECT results and workbook writes

Four existing [matrix rows](installer-support-matrices.csv) had broad owners but
no operation-level assessments. The [six cases](select-workbook.tsv) separate
the four operations by their inputs and observable results. Formula handling is
a condition of workbook update, not another format or two new backends.

| Native ledger ID | Distinguishing behavior | Documented vendor environments |
| --- | --- | --- |
| `connectors.select-without-inputs` | Map the result tree of a custom SELECT; the statement itself determines the projection and filter. No mapped Call argument is needed. | C++, C#, Java, BUILT-IN |
| `connectors.select-with-inputs` | Wire a value into **Call with Parameters** and map fields on its result side. Even a constant on that wire is an input parameter. | BUILT-IN |
| `formats.xlsx-create` | Generate an ordinary OOXML workbook; the overwrite option replaces a same-name destination rather than retaining its formatting. | C#, Java, BUILT-IN |
| `formats.xlsx-update` | Change designated cells of the existing ordinary workbook and preserve its formatting. At a formula cell, **Stop on error** differs from **Ignore (with warning in preview)**. | BUILT-IN |

**Release source:** [pinned English installer PDF](installer-support-matrices.md),
physical pages. §4.4.2.2 pp. 269–272 describes both SELECT examples and the
two sides of the Call; §20.1.2 p. 1606 rows 2–3 supplies their backend marks.
§4.6.7 p. 451 distinguishes overwrite, selective update and both formula
policies. §4.6.11 pp. 460–462 shows the configured range, retained styling and
a chart reacting to changes in its source cells. That example is not a general
chart-calculation guarantee. §20.1.1 p. 1603 rows 2–3 lists the two target
modes. Note 3 on p. 1604 and §4.6 p. 435 limit **Strict OOXML to input**;
the shared `.xlsx` suffix does not identify an eligible target.

**Representation:** Keep `connectors.sqlite`, `connectors.enterprise-databases`
and `formats.xlsx`. The matrix's database family is not an exclusive driver
assignment: the manual uses SQLite, and the existing broad enterprise-driver
refusal says nothing about an imported SQLite query. SQL CRUD, general-purpose
query execution, a stored procedure, and an editor command to open a database
are different contracts. The [existing alias case](mapping-contracts.tsv)
on topic 154 and [existing formatting case](workflow-contracts.tsv) on 232
remain valid. Neither has independent assessment slots. The six examples here
are not additional entries in those frozen case counts.

### What the repository checks

| Case | Test source inspected | Limit that affects qualification |
| --- | --- | --- |
| No query parameters | `crates/mfd/tests/standalone_db_query.rs` · `all_columns_descending_limit_one_imports_the_winner` | Checks a bounded SQLite winner; local export uses sort/first-items. Matching the result does not preserve authored SQL or prove arbitrary SELECT execution. |
| Mapped query parameter | `crates/mfd/tests/db_query_import.rs` · `static_single_table_query_imports_and_executes`, `required_host_only_query_threshold_survives_two_native_cycles` | The first fixture has a wired `:DepartmentID` despite its test name. The second checks different row counts and a missing required host value after local cycles. These are imported/lowered cases; [#103](https://github.com/DeandreT/ferrule/issues/103) concerns a separate read-only SQLite API. |
| New workbook | `crates/mfd/tests/xlsx_hierarchical_import.rs` · `imports_and_executes_runtime_named_hierarchical_workbook`; `crates/codegen-csharp/tests/flat_xlsx_dotnet.rs` · `generated_workbook_decodes_to_interpreted_row`, `update_existing_is_not_a_new_document` | The former builds two new sheets. The latter checks a generated flat byte API and refuses update mode. [#1450](https://github.com/DeandreT/ferrule/issues/1450) is not a file replacement or update receipt. |
| Update a styled workbook | `crates/mfd/tests/xlsx_export.rs` · `hierarchical_update_existing_target_imports_runs_and_roundtrips`; `crates/format-xlsx/tests/hierarchical_update.rs` · `updates_row_bands_and_preserves_everything_outside_them`, `dates_override_non_date_number_formats`, `refuses_formula_cells_inside_the_layout` | The tests check the saved flag, local cycle, bold font and numeric format. A date can **replace** a non-date format. The hierarchical path refuses a targeted formula; it does not establish the vendor's skip-with-warning behavior. [#1093](https://github.com/DeandreT/ferrule/issues/1093) and [#1215](https://github.com/DeandreT/ferrule/issues/1215) describe the bounded writer and file path. |

Use two styled cells, one a formula, to qualify the two update policies
separately; inspect the formula, ordinary cell, styles and preview warning.
Compare parameterized SELECT results for two inputs over the same source, not
merely the presence of a parameter declaration. A generated-host test using
schema-normalized JSON for `Sales_to_Excel.mfd` cannot show that a chart or a
cell style survived. No vendor, Rust or C# execution was performed for this review.

### Assessments and controls

At addition, **12 of 64 cells were `unverified`**: import and interpreter on
all four, local roundtrip on both SELECTs and existing-workbook update, and
generated Ferrule C# on new workbooks. They point to six repository test files,
not six execution receipts. The other 52 cells were `unassessed`, including
all 32 vendor cells. A matrix dot is a documentation claim, not a vendor run;
a blank was not promoted to `not_applicable`. The 811 prior rows, their
assessments and earlier evidence remained intact. `inventory_complete=false`.

Run `python verify_select_workbook.py --pdf PATH` for the joins and in-section
citations and `python verify_installer_matrices.py --pdf PATH` for the dots.
Omit `--pdf` for repository-only checks. The workflow, adapter and SELECT/
workbook controls check the **current** IDs, assessment axes and evidence
references. A supported/partial native-export or vendor claim needs
`vendor_execution` evidence. These scoped Python checks complement the ledger
validator; they do not judge the quality of a future receipt.
`python -m unittest test_live_inventory` exercises a later evidence-linked
assessment, a false vendor claim, missing IDs/references and a swapped formula
policy citation.

Historical preservation is a revision comparison: the original 789 at
`9509cf41`, 797 at `2bd2311d`, and 811 at `703901ad` remain available as Git
objects. Compare an inventory change against its actual parent; compare the
ledger from `git show <revision>:conformance/mfd-2026r2.json` at those exact
revisions to reproduce the historical checks.
Do not compare a digest of today's assessments to an old release-review digest.
