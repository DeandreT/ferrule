"""Verify the bounded database operations and their installer-manual citations."""

import argparse
import csv
from pathlib import Path

from verify_context_leaf_review import HERE, read_cases, require
from verify_installer_matrices import COLUMNS, validate
from verify_live_cells import check_cells
from verify_workflow_owners import check_pdf, load

FAMILY = "connectors.enterprise-databases"
PROFILE = ["mfd-2026r2-enterprise"]
MATRIX_ROWS = {
    "db-bulk-insert": ("1", "", {"built_in"}),
    "stored-procedure": ("4", "", {"built_in"}),
    "db-exceptions": ("5", "driver-language", {"cpp", "csharp", "java", "built_in"}),
    "db-tracing": ("6", "", {"built_in"}),
}
# Each mode has its own topic and observable check, even when its backend
# documentation is shared. These anchors are from the installer, not the web PDF.
CASES = {
    "server-batches": ("db-bulk-insert", "db-bulk-insert", 160, "4.4.3.1", 292,
                       "batch size defines the number of records"),
    "native-sqlite-excluded": ("db-bulk-insert", "db-bulk-insert", 160, "4.4.3.1", 291,
                               "SQLite n/a n/a n/a n/a No"),
    "procedure-source": ("stored-procedure-source", "stored-procedure", 175, "4.4.6.2", 328,
                         "takes no input parameters"),
    "procedure-call": ("stored-procedure-call", "stored-procedure", 176, "4.4.6.3", 332,
                       "right side contains the return and the output parameters"),
    "procedure-target": ("stored-procedure-target", "stored-procedure", 177, "4.4.6.4", 335,
                         "executed before the procedure is called"),
    "procedure-target-key": ("stored-procedure-target", "stored-procedure", 180, "4.4.6.7", 345,
                             "key and inserting the record occur during the same transaction"),
    "rollback-current": ("db-transaction-rollback", "db-exceptions", 162, "4.4.3.3", 302,
                         "only the faulty Book record"),
    "rollback-top": ("db-transaction-rollback", "db-exceptions", 162, "4.4.3.3", 302,
                     "rolled back together with the other two Book records"),
    "trace-actions": ("db-action-trace", "db-tracing", 153, "4.4.2.1", 267,
                      "rows-affected attribute"),
    "trace-errors": ("db-action-trace", "db-tracing", 153, "4.4.2.1", 266,
                     "not possible to use full tracing at table and stored-procedure level"),
}
QUALIFICATION_CITATIONS = [
    (1086, 1606, "Database bulk inserts 285"),
    (1086, 1606, "Database stored procedures 323"),
    (1086, 1606, "Database exception handling 297"),
    (1086, 1606, "Database tracing and error logging 261"),
    (1086, 1607, "connected database driver, as indicated in the previous table"),
    (160, 291, "multiple INSERT statements are executed as one query"),
    (160, 291, "actual bulk transfer of data occurs when the mapping is run by MapForce Server"),
    (160, 291, "not have any related tables, views, or stored procedures referencing the table"),
    (160, 292, "rewriteBatchedStatements=true"),
    (160, 292, "Use transactions option are mutually exclusive"),
    (173, 324, "Stored procedures can be used only in the BUILT-IN execution engine"),
    (173, 324, "cannot omit input parameters in the mapping to use the default value"),
    (173, 324, "fixed column structure are supported"),
    (173, 324, "Transactions are not supported for stored"),
    (173, 325, "SQLite does not use stored procedures"),
    (174, 327, "you can define the recordset structure manually"),
    (153, 265, "Tracing is compatible only with the Built-In transformation language"),
    (153, 267, "supplied by the database driver"),
    (177, 337, "only hints (as comments)"),
    (180, 345, "does not have any input or output connectors"),
]


def verify(rows, matrix, ledger, pages, companion):
    validate(matrix)
    by_surface = {r["surface"]: r for r in matrix}
    for surface, (row, qualifier, envs) in MATRIX_ROWS.items():
        m = by_surface[surface]
        require((m["section"], m["page"], m["row"], m["ledger_context"], m["qualifier"])
                == ("20.1.2", "1606", row, FAMILY, qualifier), f"matrix join: {surface}")
        require({env for env in COLUMNS if m[env] == "1"} == envs, f"documented marks: {surface}")

    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(len(caps) == len(ledger["capabilities"]) and ledger["inventory_complete"] is False,
            "ledger IDs/completeness")
    evidence = {e["id"]: e["kind"] for e in ledger["evidence"]}
    require(len(evidence) == len(ledger["evidence"]), "duplicate evidence IDs")
    require(FAMILY in caps and caps[FAMILY]["profiles"] == PROFILE, "broad database context")
    original = {m["canonical_id"] for m in companion["identity_policy"]["canonical_identity_map"]}
    added = {f"connectors.{c[0]}" for c in CASES.values()}
    require(len(added) == 6 and added <= caps.keys() and not added & original, "database operation IDs")
    for k in added:
        require(caps[k]["profiles"] == PROFILE and caps[k]["category"] == "connectors", f"profile: {k}")
        check_cells(caps[k], evidence)

    fields = "canonical_id matrix_surface case page_index section pdf_page locator input expected".split()
    require(all(list(r) == fields and all(v and v.strip() for v in r.values()) for r in rows), "case fields")
    require(len(rows) == len(CASES) and {r["case"] for r in rows} == CASES.keys(), "database case membership")
    require(len({(r["input"], r["expected"]) for r in rows}) == len(rows), "duplicate example")
    citations = list(QUALIFICATION_CITATIONS)
    contexts = companion["research_base_contexts"]
    for r in rows:
        demo = r["case"]
        name, surface, i, section, n, locator = CASES[demo]
        require((r["canonical_id"], r["matrix_surface"], r["page_index"], r["section"],
                 r["pdf_page"], r["locator"]) == (f"connectors.{name}", surface, str(i), section, str(n), locator),
                f"operation/source identity: {demo}")
        p = pages[i]
        require(p["index"] == i and p["section"] == section, f"topic section: {demo}")
        joins = [contexts[int(j["pointer"].removeprefix("/research_base_contexts/"))]
                 for j in p["joins"] if j["registry"] == "contracts" and j["kind"] == "research_context"]
        require(joins and all(c["source_file"] == p["source_file"] and c["canonical_id"] is None
                              and c["broad_canonical_context"] == FAMILY for c in joins),
                f"original broad context: {demo}")
        citations.append((i, n, locator))
    return citations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, help="English manual from the pinned installer")
    args = parser.parse_args()
    pages = load(HERE / "documentation-pages.json")["pages"]
    companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        citations = verify(read_cases("database-operations.tsv"), list(csv.DictReader(f)),
                           load(HERE.parent / "mfd-2026r2.json"), pages, companion)
    if args.pdf:
        check_pdf(args.pdf, citations, pages)
    print("6 native operations; 10 cases; 96 live assessment cells checked"
          + (f"; {len(citations)} in-section citations verified" if args.pdf else ""))
