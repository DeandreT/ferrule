"""Reconcile output signing only; a signature mention is not signed-input verification."""

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

BASE = "b3e3186e7d7c3a0fc23b18f538e7f133c9822c72"
INTRODUCTION = "c811449d64c91d51f3ee6d79764791012bec446b"
LEDGER_PATH = "conformance/mfd-2026r2.json"
RECEIPT = "output-signing-source.json"
CAPABILITY = "formats.sign-xml-xbrl-output"
TOPICS = {82: "4.3.1", 89: "4.3.8", 90: "4.3.8.1", 91: "4.3.8.2", 278: "4.8.6.2",
          1085: "20.1.1", 1086: "20.1.2"}
FACTS = {
    "xml_target": (82, 155), "generation": (89, 168), "preview": (89, 168), "server": (89, 168),
    "component_settings": (90, 169), "persistence": (90, 169),
    "certificate_sign": (90, 170), "certificate_verify": (90, 170),
    "password": (90, 171), "last_child": (90, 171), "xbrl_detached": (90, 172),
    "reference": (90, 172), "verify_role": (90, 172), "key_info": (90, 172), "included_public_key": (90, 172),
    "cli_failure": (90, 172), "preview_pair": (91, 172), "save_pair": (91, 173),
    "schema": (91, 173), "xbrl_target": (278, 542), "xml_format_note": (1085, 1604),
    "xbrl_format_note": (1085, 1604), "feature_label_start": (1086, 1605),
    "feature_label_end": (1086, 1606),
}
KEY_LOCATORS = {
    "certificate_sign": "signature is generated using the private key",
    "certificate_verify": "public key of the certificate is used to verify the signature",
    "xbrl_detached": "for XBRL files can be created only as external signature files",
    "xml_format_note": "XML with digital signatures processing is supported only in BUILT-IN",
    "xbrl_format_note": "XBRL with digital signatures processing is supported only in BUILT-IN",
}
IMAGES = {"detached_preview_document": (173, 2204), "detached_preview_signature": (173, 2206)}
MATRIX = {
    "feature": ["20.1.2", 1605, 16, "xml-xbrl-signatures"],
    "label_pages": [1605, 1606],
    "marks": ["built_in"],
    "links_to_pdf_page": 168,
    "format_context": [
        ["20.1.1", 1602, 1, "xml", "xml-signatures", "xml_format_note"],
        ["20.1.1", 1602, 5, "xbrl", "xbrl-signatures", "xbrl_format_note"],
    ],
}


def verify(receipt, matrix, ledger, pages, companion, workflows):
    require(receipt.keys() == {"release", "executable_version", "installer_pdf_sha256", "kind",
                                "runtime_proof", "canonical_id", "scope", "case_columns", "cases",
                                "matrix", "anchor_columns", "anchors", "image_columns", "images"},
            "source manifest fields")
    require((receipt["release"], receipt["executable_version"], receipt["installer_pdf_sha256"])
            == ("MapForce Enterprise 2026r2 x64", "28.2.0.0", PDF_HASH), "installer identity")
    require(receipt["kind"] == "documentation_only" and receipt["runtime_proof"] is False,
            "source manifest cannot establish runtime proof")
    require(receipt["canonical_id"] == CAPABILITY and receipt["scope"] == dict(
        operation="create_output_signature", product_lane="desktop_built_in_preview",
        signed_input_verification="not_established", schema_validation="separate",
        mapforce_server="explicitly_excluded", command_line="only_invalid_settings_behavior_documented_here",
    ), "scope: desktop output signing only; verification is not established")
    require(receipt["case_columns"] == "target placement output_contract facts".split()
            and receipt["cases"] == [
                ["xml", "detached", "separate_signature_references_document",
                 ["xml_target", "reference", "preview_pair", "save_pair"]],
                ["xml", "enveloped", "signature_last_child_of_document_element",
                 ["xml_target", "last_child", "schema"]],
                ["xbrl", "detached", "separate_signature_references_document",
                 ["xbrl_target", "xbrl_detached", "reference"]],
                ["xbrl", "enveloped", "excluded_by_placement_rule", ["xbrl_target", "xbrl_detached"]],
            ], "distinct format and placement cases")

    require(receipt["matrix"] == MATRIX, "matrix: one BUILT-IN signing row with a continuation")
    validate(matrix)
    by_surface = {r["surface"]: r for r in matrix}
    signing = by_surface["xml-xbrl-signatures"]
    require([signing["section"], int(signing["page"]), int(signing["row"]), signing["surface"]]
            == MATRIX["feature"] and signing["ledger_context"] == "formats.xml"
            and [k for k in COLUMNS if signing[k] == "1"] == MATRIX["marks"], "signing row")
    for section, page, row, surface, qualifier, _ in MATRIX["format_context"]:
        ordinary = by_surface[surface]
        require([ordinary["section"], int(ordinary["page"]), int(ordinary["row"]),
                 ordinary["ledger_context"], ordinary["qualifier"]]
                == [section, page, row, f"formats.{surface}", qualifier]
                and any(ordinary[k] == "1" for k in COLUMNS if k != "built_in"),
                f"ordinary format row with signature qualification: {surface}")

    require(receipt["anchor_columns"] == "fact topic_index pdf_page locator".split(), "text columns")
    anchors = receipt["anchors"]
    require(all(isinstance(a, list) and len(a) == 4 for a in anchors)
            and [a[0] for a in anchors] == list(FACTS), "text anchor membership")
    citations = []
    for fact, i, n, locator in anchors:
        require((i, n) == FACTS[fact] and isinstance(locator, str) and locator.strip()
                and pages[i]["index"] == i and pages[i]["section"] == TOPICS[i], f"text anchor: {fact}")
        require(fact not in KEY_LOCATORS or locator == KEY_LOCATORS[fact], f"distinct locator: {fact}")
        citations.append((i, n, locator))
    require(len(citations) == len(set(citations)), "duplicate text locator")

    require(receipt["image_columns"] == "id pdf_page image_xref raw_stream_sha256".split(), "image columns")
    images = receipt["images"]
    require(all(isinstance(a, list) and len(a) == 4 for a in images)
            and [a[0] for a in images] == list(IMAGES), "preview image membership")
    for name, page, xref, digest in images:
        require((page, xref) == IMAGES[name] and isinstance(digest, str)
                and re.fullmatch(r"[0-9a-f]{64}", digest), f"preview image: {name}")

    for topic, index, owner in [(90, 54, "formats.xml"), (91, 55, "formats.xml"),
                                (278, 211, "formats.xbrl")]:
        page = pages[topic]
        ctx = companion["research_base_contexts"][index]
        require(page["joins"] == [dict(registry="contracts", kind="research_context",
                                         pointer=f"/research_base_contexts/{index}")]
                and ctx["source_file"] == page["source_file"] and ctx["canonical_id"] is None
                and ctx["broad_canonical_context"] == owner, f"prior format context: {topic}")
    require({(r["page_index"], r["pdf_page"], r["locator"]) for r in workflows
             if r["family"] == "XML signatures" or
             (r["page_index"], r["locator"]) == ("278", "Create digital signature")} == {
                 ("90", "170", "must have a private key"), ("90", "172", "signature file active"),
                 ("91", "173", "appropriate element declarations"), ("278", "542", "Create digital signature"),
             }, "prior signing and verification-example anchors")

    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(len(caps) == len(ledger["capabilities"]) and ledger["inventory_complete"] is False,
            "ledger IDs/completeness")
    require({CAPABILITY, "formats.xml", "formats.xbrl"} <= caps.keys(), "signing and ordinary format IDs")
    cap = caps[CAPABILITY]
    require(cap.keys() == {"id", "title", "category", "profiles", "depends_on", "support"}
            and cap["title"] == "Create digital signatures for XML and XBRL mapping outputs"
            and cap["category"] == "formats" and cap["profiles"] == ["mfd-2026r2-enterprise"]
            and cap["depends_on"] == [], "signing ledger scope")
    evidence = {e["id"]: e["kind"] for e in ledger["evidence"]}
    require(len(evidence) == len(ledger["evidence"]), "duplicate evidence IDs")
    check_cells(cap, evidence)
    return citations


def verify_introduction(ledger, base):
    current, prior = ledger["capabilities"], base["capabilities"]
    require(len(prior) == 824 and len(current) == 825, "introduction counts")
    require(dict(ledger, capabilities=current[:824]) == base, "prior ledger, versions or evidence changed")
    cap = current[-1]
    require(cap["id"] == CAPABILITY, "append exactly one signing ID")
    check_cells(cap, {e["id"]: e["kind"] for e in ledger["evidence"]})
    support = dict(cap["support"])
    vendor = support.pop("vendor_backends")
    require(all(cell["status"] == "unassessed" and cell["evidence"] == []
                for cell in [*support.values(), *vendor.values()]), "introduction not source-only")


def verify_pdf(path, receipt, citations, matrix, pages):
    check_pdf(path, citations, pages)
    compare_pdf(matrix, path)
    import pymupdf

    with pymupdf.open(path) as doc:
        for name, page, xref, digest in receipt["images"]:
            require(sum(i["xref"] == xref for i in doc[page - 1].get_image_info(xrefs=True)) == 1
                    and hashlib.sha256(doc.xref_stream_raw(xref)).hexdigest() == digest,
                    f"installer preview image: {name}")
        signing = doc.get_toc(simple=False)[2:][89]
        require(signing[2] == receipt["matrix"]["links_to_pdf_page"], "signing bookmark page")
        # Both format settings point to the same signing topic. Each printed
        # part of the split matrix label also points to that bookmark.
        for n, label in [(155, None), (542, None), (1605, "XML and XBRL"), (1606, "signatures")]:
            page = doc[n - 1]
            rect = page.search_for(label)[0] if label else None
            require(any(link.get("page") == signing[2] - 1 and link.get("to") == signing[3]["to"]
                        and (rect is None or link["from"].intersects(rect))
                        for link in page.get_links()), f"signing bookmark link on p. {n}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, help="English manual from the pinned installer")
    parser.add_argument("--check-introduction", action="store_true",
                        help="compare the historical signing introduction with its exact Git parent")
    args = parser.parse_args()
    ledger = load(HERE.parent / "mfd-2026r2.json")
    pages = load(HERE / "documentation-pages.json")["pages"]
    companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
    receipt = load(HERE / "oracles" / RECEIPT)
    with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
        matrix = list(csv.DictReader(f))
    citations = verify(receipt, matrix, ledger, pages, companion, read_cases("workflow-contracts.tsv"))
    if args.pdf:
        verify_pdf(args.pdf, receipt, citations, matrix, pages)
    if args.check_introduction:
        base, introduction = (json.loads(subprocess.check_output(
            ["git", "show", f"{ref}:{LEDGER_PATH}"], cwd=HERE.parent.parent
        )) for ref in (BASE, INTRODUCTION))
        verify_introduction(introduction, base)
    print("1 signing operation, 4 format/placement cases; 16 live assessment cells checked"
          + ("; installer text, previews, graphical matrix and links verified" if args.pdf else "")
          + ("; prior 824 and versions unchanged, 16 new cells unassessed" if args.check_introduction else ""))
