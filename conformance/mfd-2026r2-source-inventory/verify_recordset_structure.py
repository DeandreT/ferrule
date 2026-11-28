"""Verify two design-time recordset actions without inferring runtime support."""

import argparse
import csv
import json
import subprocess
from pathlib import Path

from verify_context_leaf_review import HERE, PDF_HASH, read_cases, require
from verify_installer_matrices import COLUMNS, compare_pdf, validate
from verify_live_cells import check_cells
from verify_workflow_owners import check_pdf, load

BASE = "72c6a0668301feb04ae5602ffab9ad0f579f60b4"  # Original public revision: 2fc817398a3cefbe7176e312bc5ddf8ccd9ec3bc
INTRODUCTION = "151325badc0a8403e790d617e667d7ef01de4595"  # Original public revision: b9a847ab76ddb4406705d1bc38f70bab602ebc95
LEDGER_PATH = "conformance/mfd-2026r2.json"
RECEIPT = "recordset-structure-source.json"
DEFINE = "authoring.define-procedure-recordset"
DISCOVER = "authoring.discover-procedure-recordset"
CASES = {
    "manual-source": (DEFINE, "source", "edit-structure-scope,source-manual"),
    "manual-call": (DEFINE, "parameterized_call", "edit-structure-choice,call-manual"),
    "discover-source": (DISCOVER, "source", "edit-structure-invocation,source-no-inputs,source-execute,source-result"),
    "discover-call": (DISCOVER, "parameterized_call",
                      "edit-structure-invocation,call-side-effects,call-execute,call-result,call-mapping-connections"),
}
PAGES = {
    173: ("4.4.6", {"driver-api": 324, "fixed-shape": 324, "postgresql-function-metadata": 325}),
    174: ("4.4.6.1", {"edit-structure-scope": 327, "edit-structure-invocation": 327, "edit-structure-choice": 327}),
    175: ("4.4.6.2", {"source-no-inputs": 328, "source-manual": 329, "source-execute": 330, "source-result": 330}),
    176: ("4.4.6.3", {"call-side-effects": 333, "call-manual": 333, "call-execute": 334,
                      "call-result": 334, "call-mapping-connections": 334}),
    1086: ("20.1.2", {"runtime-matrix-context": 1606}),
}
MATRIX_CONTEXT = {"section": "20.1.2", "pdf_page": 1606, "row": 4, "surface": "stored-procedure"}


def verify(rows, receipt, matrix, ledger, pages, companion):
    require(receipt.keys() == {"release", "executable_version", "installer_pdf_sha256",
                               "kind", "runtime_proof", "matrix", "columns", "anchors"},
            "source receipt fields")
    require((receipt["release"], receipt["executable_version"], receipt["installer_pdf_sha256"])
            == ("MapForce Enterprise 2026r2 x64", "28.2.0.0", PDF_HASH), "installer identity")
    require(receipt["kind"] == "documentation_only" and receipt["runtime_proof"] is False,
            "source receipt cannot establish runtime proof")
    require(receipt["matrix"] == {"design_time_operations": "not_listed_separately",
                                  "context_only": MATRIX_CONTEXT}, "runtime matrix is context only")
    validate(matrix)
    context = next(m for m in matrix if m["surface"] == MATRIX_CONTEXT["surface"])
    require((context["section"], int(context["page"]), int(context["row"]))
            == (MATRIX_CONTEXT["section"], MATRIX_CONTEXT["pdf_page"], MATRIX_CONTEXT["row"]),
            "runtime matrix row")
    require({env for env in COLUMNS if context[env] == "1"} == {"built_in"}, "runtime matrix marks")

    require(receipt["columns"] == "fact page_index section pdf_page locator".split(), "source columns")
    expected = {name: (i, section, n) for i, (section, facts) in PAGES.items() for name, n in facts.items()}
    anchors = receipt["anchors"]
    require(all(isinstance(a, list) and len(a) == 5 for a in anchors), "source anchor shape")
    require(len(anchors) == len(expected) and {a[0] for a in anchors} == expected.keys(), "source fact membership")
    contexts = companion["research_base_contexts"]
    citations = []
    for name, i, section, n, locator in anchors:
        require((i, section, n) == expected[name] and isinstance(locator, str) and locator.strip(),
                f"source locator: {name}")
        topic = pages[i]
        require(topic["index"] == i and topic["section"] == section, f"topic identity: {name}")
        if i in (174, 175, 176):
            joined = [contexts[int(j["pointer"].removeprefix("/research_base_contexts/"))]
                      for j in topic["joins"] if j["registry"] == "contracts" and j["kind"] == "research_context"]
            require(len(joined) == 1 and joined[0]["source_file"] == topic["source_file"] and
                    joined[0]["canonical_id"] is None and
                    joined[0]["broad_canonical_context"] == "connectors.enterprise-databases",
                    f"original broad connector context: {i}")
        citations.append((i, n, locator))
    require(len(citations) == len(set(citations)), "duplicate source anchor")

    fields = "canonical_id case procedure_role input expected facts".split()
    require(all(list(r) == fields and all(v and v.strip() for v in r.values()) for r in rows), "case fields")
    require(len(rows) == len(CASES) and {r["case"] for r in rows} == CASES.keys(), "case membership")
    require(len({(r["input"], r["expected"]) for r in rows}) == len(rows), "duplicate example")
    for r in rows:
        require((r["canonical_id"], r["procedure_role"], r["facts"]) == CASES[r["case"]],
                f"operation/source join: {r['case']}")

    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(len(caps) == len(ledger["capabilities"]) and ledger["inventory_complete"] is False,
            "ledger IDs/completeness")
    require({DEFINE, DISCOVER, "connectors.stored-procedure-source",
             "connectors.stored-procedure-call"} <= caps.keys(), "design-time or runtime ID missing")
    evidence = {e["id"]: e["kind"] for e in ledger["evidence"]}
    require(len(evidence) == len(ledger["evidence"]), "duplicate evidence IDs")
    for k in (DEFINE, DISCOVER):
        c = caps[k]
        require(c["profiles"] == ["mfd-2026r2-enterprise"] and c["category"] == "authoring", f"profile: {k}")
        check_cells(c, evidence)
    return citations


def verify_introduction(ledger, base):
    original = base["capabilities"]
    current = ledger["capabilities"]
    require(len(original) == 821 and len(current) == 823, "introduction counts")
    require(dict(ledger, capabilities=current[:821]) == base, "prior ledger or evidence changed")
    require({c["id"] for c in current[821:]} == {DEFINE, DISCOVER}, "new recordset IDs")
    for c in current[821:]:
        axes = dict(c["support"])
        vendors = axes.pop("vendor_backends")
        require(len(axes) == len(vendors) == 8 and
                all(cell["status"] == "unassessed" and cell["evidence"] == []
                    for cell in [*axes.values(), *vendors.values()]), f"introduction not source-only: {c['id']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, help="English manual from the pinned installer")
    parser.add_argument("--check-introduction", action="store_true",
                        help="compare the historical recordset introduction with its exact Git parent")
    args = parser.parse_args()
    ledger = load(HERE.parent / "mfd-2026r2.json")
    pages = load(HERE / "documentation-pages.json")["pages"]
    companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        matrix = list(csv.DictReader(f))
    citations = verify(read_cases("recordset-structure.tsv"), load(HERE / "oracles" / RECEIPT),
                       matrix, ledger, pages, companion)
    if args.pdf:
        check_pdf(args.pdf, citations, pages)
        compare_pdf(matrix, args.pdf)
    if args.check_introduction:
        root = HERE.parent.parent
        base, introduction = (json.loads(subprocess.check_output(
            ["git", "show", f"{ref}:{LEDGER_PATH}"], cwd=root
        )) for ref in (BASE, INTRODUCTION))
        verify_introduction(introduction, base)
    print("2 design-time operations; 4 cases; 32 live assessment cells checked"
          + (f"; {len(citations)} in-section citations and {len(matrix) * len(COLUMNS)} matrix cells verified" if args.pdf else "")
          + ("; prior 821 unchanged and 32 new cells unassessed" if args.check_introduction else ""))
