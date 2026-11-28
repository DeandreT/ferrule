"""Live source-review joins: validate assessment structure without freezing outcomes."""

from verify_context_leaf_review import require
from verify_installer_matrices import COLUMNS


def check_cells(c, evidence):
    support = c["support"]
    local = set("import interpreter native_export ferrule_roundtrip gui debug rust csharp".split())
    require(support.keys() == local | {"vendor_backends"}
            and support["vendor_backends"].keys() == set(COLUMNS), f"axes: {c['id']}")
    cells = [(k, support[k], k == "native_export") for k in local]
    cells += [(k, v, True) for k, v in support["vendor_backends"].items()]
    for axis, cell, needs_vendor in cells:
        where = f"{c['id']}: {axis}"
        require(cell.keys() == {"status", "detail", "evidence"} and cell["detail"].strip(), where)
        status, refs = cell["status"], cell["evidence"]
        require(status in {"unassessed", "unverified", "unsupported", "partial", "supported",
                           "blocked", "not_applicable"}, where)
        require(isinstance(refs, list) and len(refs) == len(set(refs)) and set(refs) <= evidence.keys(), where)
        if status in {"partial", "supported"}:
            require(refs and (not needs_vendor or any(evidence[r] == "vendor_execution" for r in refs)), where)
