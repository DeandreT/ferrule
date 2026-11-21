"""Regression controls for independent qualification after an inventory-only revision."""

import csv
import unittest
from copy import deepcopy

from verify_adapter_operations import verify as adapters
from verify_context_leaf_review import HERE, read_cases
from verify_database_operations import verify as database_operations
from verify_quick_edi_xml import CAPABILITY as QUICK_EDI_XML
from verify_quick_edi_xml import RECEIPT as QUICK_EDI_RECEIPT
from verify_quick_edi_xml import verify as quick_edi_xml
from verify_quick_edi_xml import verify_introduction as quick_edi_introduction
from verify_recordset_structure import DEFINE, DISCOVER, RECEIPT, verify as recordset_structure
from verify_recordset_structure import verify_introduction
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

    def verify_database(self, rows=None):
        if rows is None:
            rows = read_cases("database-operations.tsv")
        database_operations(rows, self.matrix, self.ledger, self.pages, self.companion)

    def verify_recordsets(self, receipt=None, rows=None):
        if receipt is None:
            receipt = load(HERE / "oracles" / RECEIPT)
        if rows is None:
            rows = read_cases("recordset-structure.tsv")
        return recordset_structure(rows, receipt, self.matrix, self.ledger, self.pages, self.companion)

    def verify_quick_edi_xml(self, receipt=None):
        if receipt is None:
            receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
        return quick_edi_xml(receipt, self.matrix, self.ledger, self.pages, self.companion,
                             read_cases("mapping-contracts.tsv"))

    def test_later_reviews_can_change_status_and_evidence(self):
        # Simulated metadata changes only: no runtime claims about the real file.
        self.ledger["evidence"].append(dict(id="future-review", kind="repository", location="ROADMAP.md",
                                            description="Synthetic reference for this control."))
        for k in ["formats.xlsx", "deployment.credential-transfer",
                  "functions.read-binary-file", "formats.xlsx-update", "connectors.db-action-trace",
                  DEFINE, QUICK_EDI_XML]:
            self.caps[k]["support"]["gui"] = dict(status="unverified", evidence=["future-review"],
                                                  detail="A later review requires a behavioral receipt.")
        self.verify_owners()
        self.verify_adapters()
        self.verify_writes()
        self.verify_database()
        self.verify_recordsets()
        self.verify_quick_edi_xml()

    def test_matrix_or_local_test_is_not_vendor_execution(self):
        for k, check in [("deployment.credential-transfer", self.verify_owners),
                         ("functions.read-binary-file", self.verify_adapters),
                         ("formats.xlsx-update", self.verify_writes),
                         ("connectors.db-bulk-insert", self.verify_database)]:
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

    def test_local_operation_oracles_do_not_qualify_native_export(self):
        for cap, evidence in [("connectors.select-with-inputs", "sqlite-select-cli-oracle"),
                              ("formats.xlsx-update", "xlsx-write-cli-oracle")]:
            with self.subTest(cap=cap):
                cell = self.caps[cap]["support"]["native_export"]
                original = cell.copy()
                cell.update(status="partial", evidence=[evidence])
                with self.assertRaises(ValueError):
                    self.verify_writes()
                cell.update(original)

    def test_formula_policies_cannot_replace_each_other(self):
        rows = read_cases("select-workbook.tsv")
        altered = [dict(r, locator="Stop on error") if r["case"] == "formula-skip" else r for r in rows]
        with self.assertRaises(ValueError):
            select_workbook(altered, self.matrix, self.ledger)

    def test_database_procedure_modes_cannot_be_swapped(self):
        rows = read_cases("database-operations.tsv")
        ids = {"procedure-source": "connectors.stored-procedure-call",
               "procedure-call": "connectors.stored-procedure-source"}
        # Counts, IDs and matrix marks still match after this relabeling.
        altered = [dict(r, canonical_id=ids.get(r["case"], r["canonical_id"])) for r in rows]
        with self.assertRaisesRegex(ValueError, "operation/source identity"):
            self.verify_database(altered)

    def test_database_rollback_driver_qualification_cannot_be_dropped(self):
        altered = [dict(r, qualifier="") if r["surface"] == "db-exceptions" else r
                   for r in self.matrix]
        with self.assertRaisesRegex(ValueError, "qualifier"):
            database_operations(read_cases("database-operations.tsv"), altered, self.ledger,
                                self.pages, self.companion)

    def test_database_operation_must_have_its_own_ledger_slot(self):
        self.ledger["capabilities"].remove(self.caps["connectors.stored-procedure-target"])
        with self.assertRaisesRegex(ValueError, "database operation IDs"):
            self.verify_database()

    def test_recordset_source_receipt_is_not_runtime_proof(self):
        receipt = load(HERE / "oracles" / RECEIPT)
        with self.assertRaisesRegex(ValueError, "cannot establish runtime proof"):
            self.verify_recordsets(receipt=dict(receipt, runtime_proof=True))

    def test_recordset_authoring_cannot_inherit_runtime_matrix(self):
        receipt = load(HERE / "oracles" / RECEIPT)
        receipt["matrix"]["design_time_operations"] = "inherits_stored_procedure_marks"
        with self.assertRaisesRegex(ValueError, "matrix is context only"):
            self.verify_recordsets(receipt=receipt)
        cell = self.caps[DISCOVER]["support"]["vendor_backends"]["built_in"]
        cell.update(status="supported", evidence=["vendor-formats"])
        with self.assertRaisesRegex(ValueError, "built_in"):
            self.verify_recordsets()

    def test_recordset_trial_call_cannot_replace_manual_structure(self):
        rows = read_cases("recordset-structure.tsv")
        altered = [dict(r, facts="edit-structure-choice,call-execute")
                   if r["case"] == "manual-call" else r for r in rows]
        with self.assertRaisesRegex(ValueError, "operation/source join: manual-call"):
            self.verify_recordsets(rows=altered)

    def test_recordset_introduction_rejects_excluded_blank_and_changed_prior_row(self):
        # Simulate the historic transition, independently checked by --check-introduction.
        introduction = dict(self.ledger, capabilities=self.ledger["capabilities"][:823])
        base = deepcopy(dict(introduction, capabilities=introduction["capabilities"][:821]))
        verify_introduction(introduction, base)
        cell = self.caps[DEFINE]["support"]["vendor_backends"]["cpp"]
        original = cell.copy()
        cell.update(status="not_applicable")
        with self.assertRaisesRegex(ValueError, "introduction not source-only"):
            verify_introduction(introduction, base)
        cell.update(original)
        self.caps["connectors.stored-procedure-source"]["title"] += " (changed)"
        with self.assertRaisesRegex(ValueError, "prior ledger or evidence changed"):
            verify_introduction(introduction, base)

    def test_quick_edi_roles_cannot_exchange_their_triggers_and_effects(self):
        receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
        source, target = receipt["modes"]
        receipt["modes"] = [dict(target, edi_role="source"), dict(source, edi_role="target")]
        with self.assertRaisesRegex(ValueError, "distinct source and target"):
            self.verify_quick_edi_xml(receipt)

    def test_quick_edi_graph_is_not_the_xml_output_illustration(self):
        receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
        graph, xml, _ = receipt["illustrations"]
        # Same physical page and valid hashes; an image of XML is not the wired graph.
        graph[2:], xml[2:] = xml[2:], graph[2:]
        with self.assertRaisesRegex(ValueError, "illustration anchor: source_graph"):
            self.verify_quick_edi_xml(receipt)

    def test_quick_edi_role_passages_cannot_be_swapped_on_the_same_page(self):
        receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
        anchors = {a[0]: a for a in receipt["anchors"]}
        source, target = anchors["source_graph"], anchors["target_xml"]
        source[-1], target[-1] = target[-1], source[-1]
        with self.assertRaisesRegex(ValueError, "role-specific text anchor: source_graph"):
            self.verify_quick_edi_xml(receipt)

    def test_quick_edi_cannot_be_reclassified_as_schema_driven_or_preserve_empty_fields(self):
        for facet, value in (("xml_schema", "required"), ("empty_fields", "preserved")):
            receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
            receipt["shared"][facet] = value
            with self.subTest(facet=facet), self.assertRaisesRegex(ValueError, "shared schema"):
                self.verify_quick_edi_xml(receipt)

    def test_quick_edi_matrix_is_not_execution_evidence(self):
        receipt = load(HERE / "oracles" / QUICK_EDI_RECEIPT)
        with self.assertRaisesRegex(ValueError, "cannot establish runtime proof"):
            self.verify_quick_edi_xml(dict(receipt, runtime_proof=True))
        self.caps[QUICK_EDI_XML]["support"]["vendor_backends"]["built_in"].update(
            status="supported", evidence=["vendor-formats"])
        with self.assertRaisesRegex(ValueError, "built_in"):
            self.verify_quick_edi_xml()

    def test_quick_edi_introduction_preserves_prior_ids_and_does_not_assess_blanks(self):
        # CLI --check-introduction supplies the independent exact Git parent.
        base = deepcopy(dict(self.ledger, capabilities=self.ledger["capabilities"][:823]))
        quick_edi_introduction(self.ledger, base)
        cell = self.caps[QUICK_EDI_XML]["support"]["vendor_backends"]["csharp"]
        cell.update(status="not_applicable")
        with self.assertRaisesRegex(ValueError, "introduction not source-only"):
            quick_edi_introduction(self.ledger, base)
        cell.update(status="unassessed")
        self.caps["formats.edi"]["title"] += " (changed)"
        with self.assertRaisesRegex(ValueError, "prior ledger or evidence changed"):
            quick_edi_introduction(self.ledger, base)


if __name__ == "__main__":
    unittest.main()
