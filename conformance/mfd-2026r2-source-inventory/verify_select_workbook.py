"""Verify the SELECT and workbook write contracts against their release matrix rows."""

import argparse
import csv
from pathlib import Path

from verify_context_leaf_review import HERE, require, read_cases
from verify_installer_matrices import COLUMNS, validate
from verify_live_cells import check_cells
from verify_workflow_owners import check_pdf, load


def verify(rows, matrix, ledger):
    validate(matrix)
    by_surface = {r["surface"]: r for r in matrix}
    cases = {
        "fixed-predicate": ("connectors.select-without-inputs", "sql-select-no-inputs", 154),
        "mapped-predicate": ("connectors.select-with-inputs", "sql-select-with-inputs", 154),
        "replace-target": ("formats.xlsx-create", "xlsx-create", 228),
        "selective-update": ("formats.xlsx-update", "xlsx-update", 232),
        "formula-stop": ("formats.xlsx-update", "xlsx-update", 228),
        "formula-skip": ("formats.xlsx-update", "xlsx-update", 228),
    }
    require(len(rows) == len(cases) and {r["case"] for r in rows} == cases.keys(), "case membership")
    require(len({(r["input"], r["expected"]) for r in rows}) == len(rows), "duplicate example")
    envs = {"sql-select-no-inputs": {"cpp", "csharp", "java", "built_in"},
            "sql-select-with-inputs": {"built_in"}, "xlsx-create": {"csharp", "java", "built_in"},
            "xlsx-update": {"built_in"}}
    policies = {"formula-stop": "Stop on error", "formula-skip": "Ignore (with warning in preview)"}
    caps = {c["id"]: c for c in ledger["capabilities"]}
    evidence = {e["id"]: e["kind"] for e in ledger["evidence"]}
    require(len(caps) == len(ledger["capabilities"]) and not ledger["inventory_complete"], "ledger IDs/completeness")
    citations = [(154, 269, "table-like structures"), (221, 435, "Strict Open XML Spreadsheet format"),
                 (228, 451, "will be updated selectively"), (232, 461, "as many rows as there are items"),
                 (1085, 1604, "supported only as a data source")]
    for r in rows:
        require(list(r) == "canonical_id matrix_surface page_index pdf_page locator case input expected".split()
                and all(v and v.strip() for v in r.values()), "case fields")
        k, surface, i = cases[r["case"]]
        require((r["canonical_id"], r["matrix_surface"], int(r["page_index"])) == (k, surface, i),
                f"source identity: {r['case']}")
        if r["case"] in policies:
            require(r["locator"] == policies[r["case"]] and r["pdf_page"] == "451", "formula policy anchor")
        matrix_row = by_surface[surface]
        require({e for e in COLUMNS if matrix_row[e] == "1"} == envs[surface], f"vendor marks: {surface}")
        family = "formats.xlsx" if surface.startswith("xlsx") else "connectors.enterprise-databases"
        require(matrix_row["ledger_context"] == family and family in caps and k in caps, f"ledger join: {k}")
        require(caps[k]["profiles"] == ["mfd-2026r2-enterprise"] and caps[k]["category"] == k.split(".")[0],
                f"profile: {k}")
        check_cells(caps[k], evidence)
        citations.append((i, int(r["pdf_page"]), r["locator"]))
    return citations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path)
    args = parser.parse_args()
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        citations = verify(read_cases("select-workbook.tsv"), list(csv.DictReader(f)),
                           load(HERE.parent / "mfd-2026r2.json"))
    if args.pdf:
        check_pdf(args.pdf, citations, load(HERE / "documentation-pages.json")["pages"])
    print("4 native operations; 6 cases; 64 live assessment cells checked"
          + (f"; {len(citations)} in-section citations verified" if args.pdf else ""))
