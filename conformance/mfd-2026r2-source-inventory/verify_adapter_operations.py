"""Check the operation-level reconciliation of the four unassigned matrix rows."""

import argparse
import csv
import hashlib
import json
from pathlib import Path

from verify_context_leaf_review import HERE, read_cases, require
from verify_installer_matrices import COLUMNS, validate
from verify_workflow_owners import check_pdf, load


def verify(rows, matrix, ledger, pages):
    validate(matrix)
    blank = {r["surface"]: r for r in matrix if not r["ledger_context"]}
    require(blank.keys() == {"binary-file", "binary-input", "binary-output", "string-parse-serialize"},
            "original matrix partition")
    require(all([k for k in COLUMNS if r[k] == "1"] == ["built_in"] for r in blank.values())
            and blank["string-parse-serialize"]["qualifier"] == "json-string-adapters",
            "documented matrix marks/footnote")

    formats = {"csv": "flat-files", "fixed-text": "flat-files", "edi": "edi",
               "flextext": "flextext", "json": "json", "xml": "xml"}
    expected = {f"mapping.{direction}-{fmt}-string": ("string-parse-serialize", f"formats.{owner}")
                for fmt, owner in formats.items() for direction in ("parse", "serialize")}
    expected.update({"functions.read-binary-file": ("binary-file,binary-input", "functions.builtins"),
                     "functions.write-binary-file": ("binary-file,binary-output", "functions.builtins")})
    require(len(rows) == len(expected) and {r["canonical_id"] for r in rows} == expected.keys(),
            "direction/format membership")
    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(expected.keys() <= caps.keys() and len(caps) == len(ledger["capabilities"]), "ledger IDs")
    # Preserve the draft head including its eight workflow additions and earlier assessments.
    baseline = dict(ledger, capabilities=[c for c in ledger["capabilities"] if c["id"] not in expected])
    payload = json.dumps(baseline, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    require(hashlib.sha256(payload).hexdigest() ==
            "228bf4803ae1fdfbfdd8a77105468c9b88e1b3e5da16ba0af2cb802e92a269aa", "draft ledger changed")

    signatures = {u["title"]: u for u in load(HERE / "functions.json")["source_units"]}
    citations = [(814, 1224, "Text (CSV, fixed-length field text, EDI, and MapForce FlexText templates)"),
                 (814, 1224, "XML schema files"), (815, 1226, "either an incoming connector"),
                 (817, 1234, "a new record for each"), (1085, 1604, "Raw binary files"),
                 (1086, 1606, "For JSON, parsing and serialization")]
    for r in rows:
        require(list(r) == "canonical_id matrix_surfaces format_context page_index pdf_page locator "
                "synthetic_input expected".split() and all(v and v.strip() for v in r.values()), "case fields")
        k = r["canonical_id"]
        require(expected[k] == (r["matrix_surfaces"], r["format_context"]) and r["format_context"] in caps,
                f"matrix/format join: {k}")
        i, n = int(r["page_index"]), int(r["pdf_page"])
        require(0 <= i < len(pages) and 1 <= n <= 1750, f"citation: {k}")
        citations.append((i, n, r["locator"]))
        if k.startswith("functions."):
            u = signatures[k.split(".")[1]]
            require(pages[i]["source_file"] == u["source_identity"]["source_file"] and
                    u["canonical_id"] is None and u["broad_canonical_context"] == r["format_context"],
                    f"binary function identity: {k}")
        c = caps[k]
        require(c["category"] == k.split(".")[0] and c["profiles"] == ["mfd-2026r2-enterprise"]
                and not c["depends_on"] and c["title"].strip(), f"owner: {k}")
        support = dict(c["support"])
        vendor = support.pop("vendor_backends")
        require(support.keys() == set("import interpreter native_export ferrule_roundtrip gui debug rust csharp".split())
                and vendor.keys() == set(COLUMNS), f"axes: {k}")
        require(all(cell.keys() == {"status", "detail", "evidence"} and cell["status"] == "unassessed"
                    and cell["evidence"] == [] and cell["detail"].strip()
                    for cell in [*support.values(), *vendor.values()]), f"assessment: {k}")
    return citations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path)
    args = parser.parse_args()
    pages = load(HERE / "documentation-pages.json")["pages"]
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        citations = verify(read_cases("adapter-operations.tsv"), list(csv.DictReader(f)),
                           load(HERE.parent / "mfd-2026r2.json"), pages)
    if args.pdf:
        check_pdf(args.pdf, citations, pages)
    print("4 matrix rows; 14 native operations; 224 unassessed cells; draft ledger preserved"
          + (f"; {len(citations)} in-section citations verified" if args.pdf else ""))
