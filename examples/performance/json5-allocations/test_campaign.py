"""Checks for the study's evidence and owned-process failure paths."""
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("campaign", Path(__file__).with_name("campaign.py"))
campaign = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(campaign)


@unittest.skipUnless(sys.platform == "linux", "Linux wait4/process groups required")
class EvidenceAndClosure(unittest.TestCase):
    def test_exact_child_failure_is_retained(self):
        with tempfile.TemporaryDirectory() as root:
            result = Path(root) / "measurement.json"
            observed = subprocess.run([sys.executable, campaign.__file__, "--child", str(result),
                sys.executable, "-c", "print('original stdout'); raise SystemExit(17)"], capture_output=True)
            self.assertEqual(observed.returncode, 17)
            self.assertEqual(observed.stdout, b"original stdout\n")
            row = json.loads(result.read_text())
            self.assertEqual(row["exit"], 17)
            self.assertGreater(row["whole_host_peak_rss_kib"], 0)
            self.assertFalse(row["profiled"])

    def test_timeout_reaps_owned_grandchild(self):
        self.assertEqual(ctypes.CDLL(None).prctl(36, 1, 0, 0, 0), 0)
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / "commands").mkdir()
            runner = campaign.Campaign(root, os.environ.copy())
            # Unit test the closure path independently of machine admission.
            runner.headroom = lambda: None
            command = [sys.executable, "-c",
                "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); time.sleep(60)"]
            with self.assertRaises(TimeoutError):
                runner.command("timeout", command, root, seconds=0.02)
            row = json.loads((root / "commands/timeout/result.json").read_text())
            self.assertEqual(row["remaining_members"], [])
            self.assertGreaterEqual(len(row["members_at_cleanup"]), 2)
            self.assertNotEqual(row["exit"], 0)
            self.assertIn("TimeoutError", row["original_failure"])

    def test_modified_source_guard_refuses(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "source"
            path.write_bytes(b"original")
            record = campaign.identity(path)
            path.write_bytes(b"modified")
            with self.assertRaisesRegex(RuntimeError, "guard changed"):
                campaign.guard([record])

    def test_failed_evidence_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "failure.json"
            campaign.save(path, {"original": "failure"})
            with self.assertRaises(FileExistsError):
                campaign.save(path, {"replacement": "success"})
            self.assertEqual(json.loads(path.read_text()), {"original": "failure"})


if __name__ == "__main__":
    unittest.main()
