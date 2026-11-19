"""Check post-1471 ownership and the bounded optional-input and wrapper cases."""

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

from verify_context_leaf_review import HERE, PDF_HASH, read_cases, require


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def owners(rows, workflows, pages, companion, ledger):
    contexts = companion["research_base_contexts"]
    cohort = set()
    for i in {int(w["page_index"]) for w in workflows}:
        joined = [contexts[int(j["pointer"].split("/")[-1])] for j in pages[i]["joins"]]
        if all(c.get("broad_canonical_context") is None for c in joined):
            require(len(joined) == 1 and joined[0]["source_file"] == pages[i]["source_file"],
                    f"ownerless join: {i}")
            cohort.add(i)
    require([int(r["page_index"]) for r in rows] == sorted(cohort) and len(cohort) == 64,
            "owner partition")
    original = {m["canonical_id"] for m in companion["identity_policy"]["canonical_identity_map"]}
    caps = {c["id"]: c for c in ledger["capabilities"]}
    require(len(caps) == len(ledger["capabilities"]) and original <= caps.keys(), "canonical IDs")
    added = caps.keys() - original
    require(len(added) == 8 and added == {
        r["canonical_id"] for r in rows if r["original_disposition"] == "missing"
    }, "new capabilities")
    for r in rows:
        owner, disposition = r["canonical_id"], r["original_disposition"]
        require(r["reason"].strip() and (
            (disposition == "external" and not owner)
            or (disposition == "match" and owner in original
                and caps[owner]["profiles"] == ["mfd-2026r2-enterprise"])
            or (disposition == "missing" and owner in added)
        ), f"disposition: {r['page_index']}")
    require(Counter(r["original_disposition"] for r in rows) ==
            {"match": 30, "external": 18, "missing": 16}, "decision counts")
    by_page = {int(r["page_index"]): r for r in rows}
    for i, owner in {803: "deployment.host-api", 823: "deployment.credential-transfer",
                     824: "deployment.credential-transfer", 857: "interop.configured-resource-alias",
                     861: ""}.items():
        require(by_page[i]["canonical_id"] == owner, f"reviewed primary owner: {i}")
    require(Counter(by_page[i]["original_disposition"] for w in workflows
                    if (i := int(w["page_index"])) in cohort) ==
            {"match": 31, "external": 18, "missing": 17}, "example partition")
    for owner in added:
        c = caps[owner]
        require(c["profiles"] == ["mfd-2026r2-enterprise"] and not c["depends_on"], owner)
        support = c["support"]
        cells = [v for k, v in support.items() if k != "vendor_backends"]
        cells.extend(support["vendor_backends"].values())
        require(support.keys() == ledger["capabilities"][0]["support"].keys() and
                support["vendor_backends"].keys() ==
                ledger["capabilities"][0]["support"]["vendor_backends"].keys(), owner)
        require(len(cells) == 16 and all(v["status"] == "unassessed" and
                v["evidence"] == [] and v["detail"].strip() for v in cells), owner)
    # Protect the entire published baseline, including ordering, assessments,
    # dependencies, source references and profiles (normalized JSON, not CRLF).
    baseline = dict(ledger, capabilities=[c for c in ledger["capabilities"] if c["id"] in original])
    data = json.dumps(baseline, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    require(hashlib.sha256(data).hexdigest() ==
            "ff82f2572af6e6c29b8edb2e24eb4da6909127d5a29507ab16b81aea3e2a5483",
            "published ledger changed; review any subsequent qualification independently")


def bounded(defaults, wrappers, pages, functions, companion):
    by_file = {p["source_file"]: p["index"] for p in pages}
    optional = {(by_file[u["source_identity"]["source_file"]], p["name"]): p["default_state"]
                for u in functions["source_units"] for p in u["contract"]["parameters"]
                if p["optionality"] == "explicit_optional"}
    covered = set()
    citations = []
    # These revisited ports have different strengths of evidence. In
    # particular, a diagram with three disconnected ports is not three
    # independently specified defaults.
    revisited = {
        (504, "flags"): "manual-modes",
        **{(i, "language"): "standard" for i in (730, 731, 732, 733)},
        (762, "nodeset"): "standard",
        **{(547, k): "illustrated" for k in ("second", "millisecond", "timezone")},
        **{(547, k): "open" for k in ("hour", "minute")},
        **{(551, k): "open" for k in ("hour", "minute", "second", "millisecond", "negative")},
        (609, "substitute"): "partial",
    }
    for r in defaults:
        require(all(r.values()) and r["disposition"] in {
            "documented", "context", "linked", "open", "manual-modes", "standard", "illustrated", "partial"},
                "optional case")
        for pair in r["topic_pages"].split(","):
            i, n = map(int, pair.split(":"))
            citations.append((i, n, r["locator"]))
            for param in r["parameters"].split(","):
                key = (i, param)
                require(key in optional and key not in covered, f"optional slot: {key}")
                if key in revisited:
                    require(r["disposition"] == revisited[key], f"omission evidence: {key}")
                require((r["disposition"] == "documented") ==
                        (optional[key] == "explicit_source_default_or_omission_behavior"),
                        f"source default state: {key}")
                covered.add(key)
    require(covered == optional.keys() and len(covered) == 55, "optional slot coverage")
    api = {u["canonical_id"]: u["data"] for u in companion["source_units"]["API"]}
    cohort = set(range(905, 909)) | set(range(917, 921)) | set(range(928, 932))
    files = {pages[i]["source_file"] for i in cohort}
    expected = {k for k, m in api.items() if m["source"].get("source_file") in files}
    require(len(wrappers) == len(expected) == 120 and
            {r["canonical_id"] for r in wrappers} == expected, "wrapper member partition")
    for r in wrappers:
        i = int(r["page_index"])
        member = api.get(r["canonical_id"])
        require(all(r.values()) and member and
                member["source"]["source_file"] == pages[i]["source_file"], "wrapper member/topic")
        citations.append((i, int(r["pdf_page"]), r["locator"]))
    return citations


def review_supplement(leads, facets, pages, functions):
    by_file = {p["source_file"]: p["index"] for p in pages}
    params = {}
    for u in functions["source_units"]:
        i = by_file[u["source_identity"]["source_file"]]
        for p in u["contract"]["parameters"]:
            key = (i, p["name"], p["direction"])
            require(key not in params, f"repeated parameter: {key}")
            params[key] = p["optionality"]
    require(Counter((direction, status) for (_, _, direction), status in params.items()) ==
            {("input", "explicit_optional"): 55, ("input", "explicit_mandatory"): 105,
             ("input", "not_explicitly_marked"): 304, ("output", "not_explicitly_marked"): 1},
            "parameter directions")
    expected = {(i, name, "input"): "inherited" for i in (730, 731, 733) for name in ("calendar", "place")}
    expected.update({(632, "default-encoding", "input"): "conditional", (625, "data", "output"): "output-only"})
    found = {}
    citations = []
    for r in leads:
        require(all(r.values()), "incomplete supplementary lead")
        for pair in r["topic_pages"].split(","):
            i, n = map(int, pair.split(":"))
            citations.append((i, n, r["locator"]))
            for name in r["parameters"].split(","):
                key = (i, name, r["direction"])
                require(key not in found and params.get(key) == "not_explicitly_marked", f"lead: {key}")
                found[key] = r["disposition"]
    require(found == expected, "inherited/conditional/output partition")
    require(Counter(int(r["page_index"]) for r in facets) == {803: 1, 823: 2, 824: 1, 857: 3},
            "mixed workflow facets")
    for r in facets:
        require(all(r.values()), "incomplete workflow facet")
        citations.append((int(r["page_index"]), int(r["pdf_page"]), r["locator"]))
    require(len(citations) == len(set(citations)), "duplicate supplement citation")
    return citations


def check_wrapper_tables(doc, wrappers, pages, companion):
    """Compare exact signature cells, including overloads with the same method name."""
    api = {u["canonical_id"]: u["data"]["member_identity"] for u in companion["source_units"]["API"]
           if "member_identity" in u["data"]}
    toc = doc.get_toc(simple=False)[2:]
    compact = lambda s: "".join(s.split())
    for i in {int(r["page_index"]) for r in wrappers}:
        a, b = toc[i:i+2]
        rows = []
        for n in range(a[2], b[2]+1):
            top = max(75, a[3]["to"].y) if n == a[2] else 75
            bottom = min(730, b[3]["to"].y) if n == b[2] else 730
            page = doc[n-1]
            # Ruled rows have a Name cell and a Description cell; C# and Java
            # also have a leading icon cell. Do not interleave their columns.
            borders = {}
            for drawing in page.get_drawings():
                x, y, right, end = drawing["rect"]
                if x == right and 45 <= x <= 560 and top-1 <= y < end <= bottom+1:
                    borders.setdefault((y, end), set()).add(x)
            for (y, end), xs in sorted(borders.items()):
                if len(xs) not in (3, 4):
                    continue
                left, mid, right = sorted(xs)[-3:]
                signature = page.get_text(clip=(left, y, mid, end), sort=True)
                description = page.get_text(clip=(mid, y, right, end), sort=True)
                if signature.strip() not in ("", "Name") and description.strip():
                    rows.append((compact(signature), n, " ".join(description.split())))
        for r in (r for r in wrappers if int(r["page_index"]) == i):
            identity = api[r["canonical_id"]]
            language = "C++" if i <= 908 else "C#" if i <= 920 else "Java"
            require(identity["owner"] == pages[i]["manual_path"][-1] and
                    identity["language"] == language, f"wrapper source type: {i}")
            signature = compact(identity["displayed_signature"])
            hits = [x for x in rows if x[0] == signature]
            require(len(hits) == 1, f"wrapper declaration cell: {r['canonical_id']}")
            rows.remove(hits[0])
            # ElementType namespace descriptions continue across page breaks;
            # their prior citations are checked by the bookmark locator test.
            if i not in {906, 918, 929}:
                require(hits[0][1] == int(r["pdf_page"]) and
                        r["locator"].casefold() in hits[0][2].casefold(), f"wrapper row locator: {signature}")
        extra = {compact(s) for s in [
            "void SaveToFile(const string_type & fileName, bool omitXmlDecl)",
            "void SaveToFile(const string_type & fileName, bool prettyPrint, const string_type & encoding)",
        ]} if i == 905 else set()
        require({sig for sig, _, _ in rows} == extra and len(rows) == len(extra), f"unaccounted declaration: {i}")


def check_pdf(path, citations, pages):
    require(hashlib.sha256(path.read_bytes()).hexdigest() == PDF_HASH, "installer PDF hash")
    import pymupdf

    with pymupdf.open(path) as doc:
        toc = doc.get_toc(simple=False)[2:]
        for i, n, locator in citations:
            a, b = toc[i:i + 2]
            require(a[1].strip() == pages[i]["manual_path"][-1].strip() and a[2] <= n <= b[2],
                    f"citation bookmark: {i}")
            top = max(75, a[3]["to"].y) if n == a[2] else 75
            bottom = min(730, b[3]["to"].y) if n == b[2] else 730
            text = doc[n-1].get_text(clip=(45, top, 560, bottom), sort=True) if top < bottom else ""
            require(locator.casefold() in " ".join(text.split()).casefold(), f"locator: {i}, {locator}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path)
    args = parser.parse_args()
    pages = load(HERE / "documentation-pages.json")["pages"]
    companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
    owners(read_cases("workflow-owners.tsv"), read_cases("workflow-contracts.tsv"), pages,
           companion, load(HERE.parent / "mfd-2026r2.json"))
    wrappers = read_cases("wrapper-cases.tsv")
    functions = load(HERE / "functions.json")
    citations = bounded(read_cases("optional-inputs.tsv"), wrappers, pages, functions, companion)
    citations += review_supplement(read_cases("optional-leads.tsv"), read_cases("reviewed-workflow-facets.tsv"),
                                   pages, functions)
    if args.pdf:
        check_pdf(args.pdf, citations, pages)
        import pymupdf

        with pymupdf.open(args.pdf) as doc:
            check_wrapper_tables(doc, wrappers, pages, companion)
    print("64 ownership decisions; 8 native IDs; 55 optional slots + 7 unmarked input leads; "
          "1 unmarked output; 120 existing wrapper members"
          + (f"; {len(citations)} in-section citations verified" if args.pdf else ""))
