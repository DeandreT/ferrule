"""Validate the catalog; optionally compare every cell with the pinned installer PDF."""

import argparse
import csv
import hashlib
import json
from pathlib import Path


HERE = Path(__file__).resolve().parent
COLUMNS = "xslt1 xslt2 xslt3 xquery1 cpp csharp java built_in".split()
FIELDS = ["section", "page", "row", "surface", "ledger_context", "qualifier", *COLUMNS]
PAGES = {1602: 5, 1603: 14, 1604: 11, 1605: 16, 1606: 12}
PDF_HASH = "9cf5bfa17ec38140941566dd15b19a9c32185339b04fb7383674f50452f43a09"
# Regression pin for the reviewed row/mark associations. This does not replace
# comparing against the PDF when the catalog is changed.
REVIEWED_ROWS = "b23975d4537205bf50834e01767da67caaf07301bd872edd944b54842c91850e"
QUALIFIERS = {
    "xml-signatures": {"xml"},
    "xbrl-signatures": {"xbrl"},
    "strict-xlsx-input": {"xlsx-read", "xlsx-create", "xlsx-update"},
    "driver-environment": {"ado", "ado-net", "odbc", "jdbc", "sqlite-native", "postgresql-native"},
    "call-only": {"http-api-call"},
    "soap-call-security": {"soap-call"},
    "collection-engine": {"wildcard-input-filename"},
    "json-string-adapters": {"string-parse-serialize"},
    "driver-language": {"db-exceptions"},
}


def check(ok, message):
    if not ok:
        raise ValueError(message)


def validate(rows):
    anchors = [(str(pg), str(n)) for pg, count in PAGES.items() for n in range(1, count + 1)]
    check([(r["page"], r["row"]) for r in rows] == anchors, "missing, duplicate or reordered row")
    check(len({r["surface"] for r in rows}) == len(rows), "duplicate surface")
    ledger = json.loads((HERE.parent / "mfd-2026r2.json").read_text(encoding="utf-8"))
    contexts = {c["id"] for c in ledger["capabilities"] if "mfd-2026r2-enterprise" in c["profiles"]}
    qualifiers = {surface: key for key, surfaces in QUALIFIERS.items() for surface in surfaces}
    check(qualifiers.keys() <= {r["surface"] for r in rows}, "missing qualified surface")
    for r in rows:
        check(list(r) == FIELDS and all(v is not None for v in r.values()), "wrong CSV fields")
        check(r["section"] == ("20.1.1" if int(r["page"]) < 1605 else "20.1.2"), "wrong section")
        check(r["ledger_context"] in contexts or not r["ledger_context"], "unknown native context")
        check(r["qualifier"] == qualifiers.get(r["surface"], ""), f"wrong qualifier: {r['surface']}")
        check(all(r[k] in ("0", "1") for k in COLUMNS), "invalid mark")
    payload = "\n".join("\t".join(r[k] for k in FIELDS) for r in rows)
    check(hashlib.sha256(payload.encode()).hexdigest() == REVIEWED_ROWS,
          "reviewed row associations changed; compare the original PDF")


def compare_pdf(rows, pdf):
    check(hashlib.sha256(pdf.read_bytes()).hexdigest() == PDF_HASH, "wrong manual or release")
    import pymupdf  # Optional: needed only for physical PDF verification.

    # Centers are in PDF points. Locate the eight cells from their printed
    # columns and use the vertical placement of the repeated dot on each row.
    with pymupdf.open(pdf) as doc:
        for pg, count in PAGES.items():
            marks = {}
            for image in doc[pg - 1].get_image_info(xrefs=True):
                if image["xref"] != 9170:
                    continue
                x, y, right, bottom = image["bbox"]
                col = round((x - 159) / 48)
                check(0 <= col < 8 and x == 159 + col * 48, f"unrecognized mark column on {pg}")
                check((right - x, bottom - y) == (12.75, 12.75), "unexpected dot size")
                group = marks.setdefault(y, set())
                check(col not in group, "duplicate dot")
                group.add(col)
            check(len(marks) == count, f"wrong number of marked rows on {pg}")
            page_rows = [r for r in rows if int(r["page"]) == pg]
            for row, (_, cols) in zip(page_rows, sorted(marks.items()), strict=True):
                check(cols == {i for i, k in enumerate(COLUMNS) if row[k] == "1"},
                      f"mark mismatch: page {pg}, {row['surface']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, help="English manual from the pinned installer")
    args = parser.parse_args()
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    validate(rows)
    if args.pdf:
        compare_pdf(rows, args.pdf)
    print(f"{len(rows)} rows, {len(rows) * len(COLUMNS)} cells"
          + ("; matched pinned PDF" if args.pdf else "; catalog checks passed"))
