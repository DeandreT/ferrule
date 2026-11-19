"""Check the frozen context cohort, manager citations, and optional PDF bookmarks."""

import argparse
import csv
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
PDF_HASH = "9cf5bfa17ec38140941566dd15b19a9c32185339b04fb7383674f50452f43a09"
MANAGERS = set(range(93, 97)) | set(range(98, 107)) | set(range(258, 264)) | set(range(265, 276))
FIELDS = "page_index section first_pdf_page last_pdf_page body_facet review_depth source_observation".split()


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def mapping_cases():
    with (HERE / "mapping-contracts.tsv").open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def verify(rows, pages, prose, cases):
    cohort = [p for p in pages if p["review_level"] == "compared_context_or_control_only"]
    require(len(cohort) == 462 and all(p["leaf"] for p in cohort), "unexpected baseline cohort")
    require([r["page_index"] for r in rows] == [str(p["index"]) for p in cohort], "page partition")
    require(Counter(r["body_facet"] for r in rows) == {"operation": 290, "reference": 172},
            "review classifications")
    citations = {}
    for i, section, start, end in re.findall(
        r"^\| (\d+) \| ([\d.]+) · (\d+)(?:–(\d+))? \|", prose, re.M
    ):
        require(int(i) not in citations, f"duplicate contract {i}")
        citations[int(i)] = (section, start, end or start)
    require(citations.keys() == MANAGERS, "manager contract partition")
    mapping = {int(c["page_index"]) for c in cases}
    require(len(cases) == 112 and len(mapping) == 93 and not mapping & MANAGERS, "mapping cohort")
    require(len({c["case_id"] for c in cases}) == len(cases), "duplicate case")
    anchors = {int(r["page_index"]): r for r in rows}
    for c in cases:
        require(list(c) == ["page_index", "pdf_page", "locator", "case_id", "rule",
                            "synthetic_input", "expected"] and all(v and v.strip() for v in c.values()),
                "case fields")
        i = int(c["page_index"])
        require(i in anchors, f"case outside historical cohort: {i}")
        require(int(anchors[i]["first_pdf_page"]) <= int(c["pdf_page"]) <=
                int(anchors[i]["last_pdf_page"]), f"case page: {c['case_id']}")
    for r, page in zip(rows, cohort, strict=True):
        i = page["index"]
        require(list(r) == FIELDS and r["section"] == page["section"], f"schema/section: {i}")
        require(1 <= int(r["first_pdf_page"]) <= int(r["last_pdf_page"]) <= 1750, f"pages: {i}")
        require(bool(r["source_observation"].strip()), f"empty facet: {i}")
        depth = "manager-contract" if i in MANAGERS else "mapping-contract" if i in mapping else "triage"
        require(r["review_depth"] == depth, f"depth: {i}")
        if i in MANAGERS:
            require(citations[i] == (r["section"], r["first_pdf_page"], r["last_pdf_page"]), f"citation: {i}")


def verify_pdf(rows, pages, path, mapping):
    require(hashlib.sha256(path.read_bytes()).hexdigest() == PDF_HASH, "wrong edition or PDF")
    import pymupdf

    norm = lambda s: " ".join(s.split()).casefold()
    cases = {}
    for case in mapping:
        cases.setdefault((int(case["page_index"]), int(case["pdf_page"])), []).append(case)
    with pymupdf.open(path) as doc:
        toc = doc.get_toc(simple=False)
        # Two initial bookmarks precede Introduction; Index follows the last topic.
        require(len(toc) == len(pages) + 3 and doc.page_count == 1750, "unexpected contents")
        for r in rows:
            i = int(r["page_index"])
            start, stop = toc[i + 2:i + 4]
            require(norm(start[1]) == norm(pages[i]["manual_path"][-1]), f"title: {i}")
            require(start[2] == int(r["first_pdf_page"]), f"first page: {i}")
            # Use the next bookmark to exclude the next topic; trim running
            # heads/feet so an otherwise empty final page is not attributed here.
            last = None
            for n in range(start[2], stop[2] + 1):
                top = max(75, start[3]["to"].y) if n == start[2] else 75
                bottom = min(730, stop[3]["to"].y) if n == stop[2] else 730
                text = doc[n - 1].get_text(clip=(45, top, 560, bottom), sort=True) if top < bottom else ""
                if text.strip():
                    last = n
                for case in cases.get((i, n), []):
                    require(norm(case["locator"]) in norm(text), f"source locator: {case['case_id']}")
            require(last == int(r["last_pdf_page"]), f"last page: {i}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path)
    args = parser.parse_args()
    pages = json.loads((HERE / "documentation-pages.json").read_text(encoding="utf-8"))["pages"]
    with (HERE / "context-leaf-review.csv").open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    cases = mapping_cases()
    verify(rows, pages, (HERE / "context-leaf-review.md").read_text(encoding="utf-8"), cases)
    if args.pdf:
        verify_pdf(rows, pages, args.pdf, cases)
    depths = Counter(r["review_depth"] for r in rows)
    print(f"{len(rows)} leaves: {depths['manager-contract']} manager, {depths['mapping-contract']} mapping, "
          f"{depths['triage']} triage"
          + ("; PDF anchors verified" if args.pdf else ""))
