"""Regression controls for independent qualification after an inventory-only revision."""

import csv
import unittest

from verify_adapter_operations import verify as adapters
from verify_context_leaf_review import HERE, read_cases
from verify_select_workbook import verify as select_workbook
from verify_workflow_owners import load, owners


class LiveInventory(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pages = load(HERE / "documentation-pages.json")["pages"]
        cls.companion = load(HERE.parent / "mfd-2026r2.source-contracts.json")
        with (HERE / "installer-support-matrices.csv").open(encoding="utf-8", newline="") as f:
            cls.matrix = list(csv.DictReader(f))

    def setUp(self):
        self.ledger = load(HERE.parent / "mfd-2026r2.json")
        self.caps = {c["id"]: c for c in self.ledger["capabilities"]}

    def verify_owners(self):
        owners(read_cases("workflow-owners.tsv"), read_cases("workflow-contracts.tsv"),
               self.pages, self.companion, self.ledger)

    def verify_adapters(self):
        adapters(read_cases("adapter-operations.tsv"), self.matrix, self.ledger, self.pages)

    def verify_writes(self):
        select_workbook(read_cases("select-workbook.tsv"), self.matrix, self.ledger)

    def test_later_reviews_can_change_status_and_evidence(self):
        # Simulated metadata changes only: no runtime claims about the real file.
        self.ledger["evidence"].append(dict(id="future-review", kind="repository", location="ROADMAP.md",
                                            description="Synthetic reference for this control."))
        for k in ["formats.xlsx", "deployment.credential-transfer",
                  "functions.read-binary-file", "formats.xlsx-update"]:
            self.caps[k]["support"]["gui"] = dict(status="unverified", evidence=["future-review"],
                                                  detail="A later review requires a behavioral receipt.")
        self.verify_owners()
        self.verify_adapters()
        self.verify_writes()

    def test_matrix_or_local_test_is_not_vendor_execution(self):
        for k, check in [("deployment.credential-transfer", self.verify_owners),
                         ("functions.read-binary-file", self.verify_adapters),
                         ("formats.xlsx-update", self.verify_writes)]:
            cell = self.caps[k]["support"]["vendor_backends"]["built_in"]
            with self.subTest(k=k), self.assertRaises(ValueError):
                cell.update(status="partial", evidence=["vendor-formats", "xlsx-update-style-test"])
                check()

    def test_missing_operation_and_orphan_reference_fail(self):
        self.caps["formats.xlsx-create"]["support"]["import"]["evidence"] = ["unknown-evidence"]
        with self.assertRaises(ValueError):
            self.verify_writes()
        self.ledger["capabilities"].remove(self.caps["formats.xlsx-create"])
        with self.assertRaises(ValueError):
            self.verify_writes()

    def test_formula_policies_cannot_replace_each_other(self):
        rows = read_cases("select-workbook.tsv")
        altered = [dict(r, locator="Stop on error") if r["case"] == "formula-skip" else r for r in rows]
        with self.assertRaises(ValueError):
            select_workbook(altered, self.matrix, self.ledger)


if __name__ == "__main__":
    unittest.main()
