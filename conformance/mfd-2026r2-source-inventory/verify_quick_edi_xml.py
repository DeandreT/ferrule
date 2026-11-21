"""Validate source-only itemization of the Quick EDI-to-XML conversion."""

import argparse
import csv
import hashlib
import json
import re
import subprocess
from pathlib import Path

from verify_context_leaf_review import HERE, PDF_HASH, read_cases, require
from verify_installer_matrices import COLUMNS, compare_pdf, validate
from verify_live_cells import check_cells
from verify_workflow_owners import check_pdf, load

BASE = "151325badc0a8403e790d617e667d7ef01de4595"  # Original public revision: b9a847ab76ddb4406705d1bc38f70bab602ebc95
LEDGER_PATH = "conformance/mfd-2026r2.json"
RECEIPT = "quick-edi-xml-source.json"
CAPABILITY = "formats.quick-edi-to-xml"
TITLE = "Quick non-schema EDI-to-XML conversion in source or target mode"
RELATED = {"formats.edi", "formats.xml", "mapping.parse-edi-string", "mapping.serialize-edi-string"}

ANCHORS = {
    "target_only": (235, "4.7.2", 473),
    "schema": (251, "4.7.5", 505),
    "omission": (251, "4.7.5", 505),
    "shape": (251, "4.7.5", 505),
    "language": (251, "4.7.5", 505),
    "source_graph": (251, "4.7.5", 506),
    "source_xml": (251, "4.7.5", 506),
    "target_xml": (251, "4.7.5", 506),
    "target_example": (251, "4.7.5", 507),
    "matrix_row": (1086, "20.1.2", 1606),
}
IMAGES = {"source_graph": (506, 3976), "source_xml": (506, 3978), "target_xml": (507, 3983)}
ROLE_LOCATORS = {
    "source_graph": "This creates a target XML component with the copy-all",
    "target_xml": "produces XML output instead of EDI output",
}
MATRIX = dict(surface="quick-edi-to-xml", section="20.1.2", pdf_page=1606, row=7,
              marked=["built_in"], links_to_pdf_page=505)


def verify(receipt, matrix, ledger, pages, companion, mapping_cases):
    require(receipt.keys() == {"release", "executable_version", "installer_pdf_sha256", "kind",
                                "runtime_proof", "canonical_id", "matrix", "shared", "modes",
                                "anchor_columns", "anchors", "illustration_columns", "illustrations"},
            "source receipt fields")
    require((receipt["release"], receipt["executable_version"], receipt["installer_pdf_sha256"])
            == ("MapForce Enterprise 2026r2 x64", "28.2.0.0", PDF_HASH), "installer identity")
    require(receipt["kind"] == "documentation_only" and receipt["runtime_proof"] is False,
            "source receipt cannot establish runtime proof")
    require(receipt["canonical_id"] == CAPABILITY, "conversion ID")
    require(receipt["shared"] == dict(language="built_in", xml_schema="neither_required_nor_provided",
                                      empty_fields="omitted", shape="source_dependent_not_customizable"),
            "shared schema, empty-field and shape contract")
    require(receipt["modes"] == [
        dict(edi_role="source", trigger="create_mapping_to_xml",
             mapping_effect="adds_xml_output_target_with_copy_all", output="xml",
             illustration_ids=["source_graph", "source_xml"]),
        dict(edi_role="target", trigger="write_content_in_xml_format",
             mapping_effect="switches_existing_edi_target_output_to_xml", output="xml",
             illustration_ids=["target_xml"]),
    ], "distinct source and target mode contracts")

    require(receipt["matrix"] == MATRIX, "conversion matrix claim")
    validate(matrix)
    row = next(r for r in matrix if r["surface"] == MATRIX["surface"])
    require((row["section"], int(row["page"]), int(row["row"]), row["ledger_context"])
            == (MATRIX["section"], MATRIX["pdf_page"], MATRIX["row"], "formats.edi")
            and [k for k in COLUMNS if row[k] == "1"] == MATRIX["marked"], "conversion matrix row")

    require(receipt["anchor_columns"] == "fact topic_index section pdf_page locator".split(),
            "text anchor columns")
    anchors = receipt["anchors"]
    require(all(isinstance(a, list) and len(a) == 5 for a in anchors)
            and [a[0] for a in anchors] == list(ANCHORS), "text anchor membership")
    citations = []
    for fact, i, section, n, locator in anchors:
        require((i, section, n) == ANCHORS[fact] and isinstance(locator, str) and locator.strip()
                and pages[i]["index"] == i and pages[i]["section"] == section, f"text anchor: {fact}")
        if fact in ROLE_LOCATORS:
            require(locator == ROLE_LOCATORS[fact], f"role-specific text anchor: {fact}")
        citations.append((i, n, locator))
    require(len(set(citations)) == len(citations), "duplicate text citation")

    require(receipt["illustration_columns"] == "id pdf_page image_xref raw_stream_sha256".split(),
            "illustration columns")
    images = receipt["illustrations"]
    require(all(isinstance(a, list) and len(a) == 4 for a in images)
            and [a[0] for a in images] == list(IMAGES), "illustration membership")
    for name, page, xref, digest in images:
        require((page, xref) == IMAGES[name] and isinstance(digest, str)
                and re.fullmatch(r"[0-9a-f]{64}", digest), f"illustration anchor: {name}")

    topic = pages[251]
    require(topic["joins"] == [dict(registry="contracts", kind="research_context",
                                  pointer="/research_base_contexts/188")], "original topic join")
    context = companion["research_base_contexts"][188]
    require(context["source_file"] == topic["source_file"] == "mf_structcomp_edi_editoxml.html"
            and context["canonical_id"] is None and context["broad_canonical_context"] == "formats.edi",
            "original broad EDI context")
    require([(r["page_index"], r["pdf_page"], r["locator"])
             for r in mapping_cases if r["case_id"] == "quick-edi-shape"]
            == [("251", "505", "do not contain data")], "earlier omission case")

    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(len(caps) == len(ledger["capabilities"]) and ledger["inventory_complete"] is False,
            "ledger IDs/completeness")
    require({CAPABILITY, *RELATED} <= caps.keys(), "conversion and related IDs")
    c = caps[CAPABILITY]
    require(c.keys() == {"id", "title", "category", "profiles", "depends_on", "support"}
            and c["category"] == "formats" and c["profiles"] == ["mfd-2026r2-enterprise"]
            and c["title"] == TITLE and c["depends_on"] == [], "conversion scope")
    evidence = {e["id"]: e["kind"] for e in ledger["evidence"]}
    require(len(evidence) == len(ledger["evidence"]), "duplicate evidence IDs")
    check_cells(c, evidence)
    return citations


def verify_introduction(ledger, base):
    prior = base["capabilities"]
    current = ledger["capabilities"]
    require(len(prior) == 823 and len(current) == 824, "introduction counts")
    require(dict(ledger, capabilities=current[:823]) == base, "prior ledger or evidence changed")
    cap = current[-1]
    require(cap["id"] == CAPABILITY, "append exactly one new conversion ID")
    check_cells(cap, {e["id"]: e["kind"] for e in ledger["evidence"]})
    support = dict(cap["support"])
    vendors = support.pop("vendor_backends")
    require(all(cell["status"] == "unassessed" and cell["evidence"] == []
                for cell in [*support.values(), *vendors.values()]), "introduction not source-only")


def verify_pdf(path, receipt, citations, matrix, pages):
    check_pdf(path, citations, pages)
    compare_pdf(matrix, path)
    import pymupdf

    with pymupdf.open(path) as doc:
        for name, page, xref, digest in receipt["illustrations"]:
            require(sum(i["xref"] == xref for i in doc[page - 1].get_image_info(xrefs=True)) == 1
                    and hashlib.sha256(doc.xref_stream_raw(xref)).hexdigest() == digest,
                    f"installer illustration: {name}")
        spec = receipt["matrix"]
        page = doc[spec["pdf_page"] - 1]
        topic = doc.get_toc(simple=False)[2:][251]
        # This row's printed label carries a link to the conversion's bookmark.
        label, = page.search_for("Quick")
        require(topic[2] == spec["links_to_pdf_page"] and any(
            link.get("page") == topic[2] - 1 and link["from"].intersects(label)
            and link.get("to") == topic[3]["to"]
            for link in page.get_links()
        ), "matrix label does not link to the conversion topic")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, help="English manual from the pinned installer")
    parser.add_argument("--check-introduction", action="store_true", help="compare against exact Git parent")
    args = parser.parse_args()
    ledger = load(HERE.parent / "mfd-2026r2.json")
    pages = load(HERE / "documentation-pages.json")["pages"]
    companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
    receipt = load(HERE / "oracles" / RECEIPT)
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        matrix = list(csv.DictReader(f))
    citations = verify(receipt, matrix, ledger, pages, companion, read_cases("mapping-contracts.tsv"))
    if args.pdf:
        verify_pdf(args.pdf, receipt, citations, matrix, pages)
    if args.check_introduction:
        base = json.loads(subprocess.check_output(
            ["git", "show", f"{BASE}:{LEDGER_PATH}"], cwd=HERE.parent.parent
        ))
        verify_introduction(ledger, base)
    print("1 conversion, 2 distinct modes; 16 live assessment cells checked"
          + ("; text, illustrations, graphical matrix and topic link verified" if args.pdf else "")
          + ("; prior 823 unchanged and 16 new cells unassessed" if args.check_introduction else ""))
