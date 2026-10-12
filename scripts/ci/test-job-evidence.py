#!/usr/bin/env python3
"""Focused preparation/refusal tests; no provider, compiler, or runner process."""
import importlib.util
import json
import os
from pathlib import Path
import socket
import stat
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location("job_evidence", Path(__file__).with_name("job-evidence.py"))
evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(evidence)


class JobEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="ferrule-job-evidence-")
        self.addCleanup(self.scratch.cleanup)
        self.home = Path(self.scratch.name) / "runner"
        self.home.mkdir(mode=0o700)
        self.environment = {"GITHUB_REPOSITORY": "DeandreT/ferrule", "GITHUB_RUN_ID": "123",
                            "GITHUB_RUN_ATTEMPT": "1", "GITHUB_JOB": "test",
                            "GITHUB_SHA": "a" * 40, "FERRULE_CI_SUITE": "native"}

    def begin(self, nonce="0" * 32, **changes):
        return evidence.begin(self.home, self.environment | changes, nonce=nonce)

    def snapshot(self):
        return {str(p.relative_to(self.home)): (p.lstat().st_mode,
                p.read_bytes() if p.is_file() and not p.is_symlink() else None)
                for p in self.home.rglob("*")}

    def observer_environment(self, settings, **changes):
        return self.environment | settings | {"FERRULE_CI_OBSERVED_JOB_STATE": "success"} | changes

    def test_private_job_layout_and_immutable_unarchived_records(self):
        settings = self.begin()
        root = Path(settings["FERRULE_CI_EVIDENCE_ROOT"])
        for folder in (root.parent, root, root / "tmp", root / "receipts"):
            self.assertEqual(stat.S_IMODE(folder.stat().st_mode), 0o700)
        initial = json.loads((root / "receipts/UNARCHIVED.json").read_bytes())
        self.assertEqual((initial["state"], initial["closure"], initial["archive_verified"]),
                         ("UNARCHIVED", "UNQUALIFIED", False))
        self.assertEqual(stat.S_IMODE((root / "receipts/UNARCHIVED.json").stat().st_mode), 0o444)
        self.assertEqual(settings["TMPDIR"], str(root / "tmp"))

    def test_later_disjoint_job_does_not_require_archive_of_prior_job(self):
        first = self.begin()
        original = self.snapshot()
        second = self.begin("1" * 32)
        self.assertNotEqual(first["TMPDIR"], second["TMPDIR"])
        for name, retained in original.items():
            p = self.home / name
            self.assertEqual((p.lstat().st_mode, p.read_bytes() if p.is_file() else None), retained)

    def test_exact_identity_collision_refuses_without_overwrite(self):
        self.begin()
        before = self.snapshot()
        with self.assertRaises(FileExistsError):
            self.begin()
        self.assertEqual(self.snapshot(), before)

    def test_invalid_identity_refuses_before_directory_creation(self):
        for changes in ({"GITHUB_REPOSITORY": "elsewhere/repo"}, {"GITHUB_RUN_ID": "../1"},
                        {"GITHUB_RUN_ATTEMPT": "0"}, {"GITHUB_JOB": "job\nTMPDIR=/"},
                        {"GITHUB_SHA": "A" * 40}, {"FERRULE_CI_SUITE": "unknown"},
                        {"FERRULE_CI_EVIDENCE_SUITE": "codegen-crates"}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.begin(**changes)
            self.assertFalse((self.home / "ci-tmp").exists())

    def test_invalid_nonce_refuses_before_directory_creation(self):
        for nonce in ("../x", "1" * 31, "G" * 32):
            with self.subTest(nonce=nonce), self.assertRaises(ValueError):
                self.begin(nonce)
        self.assertFalse((self.home / "ci-tmp").exists())

    def test_codegen_matrix_identity_is_separate(self):
        roots = []
        for number, suite in enumerate(("codegen-crates", "codegen-mappings")):
            settings = self.begin(str(number) * 32, FERRULE_CI_SUITE="codegen",
                                  FERRULE_CI_EVIDENCE_SUITE=suite)
            record = json.loads(Path(settings["FERRULE_CI_EVIDENCE_CANARY"]).read_bytes())
            self.assertEqual(record["identity"]["suite"], suite)
            self.assertEqual(settings["FERRULE_CI_EVIDENCE_ID"], str(number) * 32)
            roots.append(Path(settings["FERRULE_CI_EVIDENCE_ROOT"]))
        self.assertNotEqual(*roots)
        with self.assertRaises(ValueError):
            self.begin("2" * 32, FERRULE_CI_SUITE="codegen")

    def test_maximum_identity_keeps_real_unix_socket_usable(self):
        environment = self.environment | {"GITHUB_RUN_ID": "9" * 20,
            "GITHUB_RUN_ATTEMPT": "9" * 20, "GITHUB_JOB": "g" * 80,
            "FERRULE_CI_SUITE": "codegen", "FERRULE_CI_EVIDENCE_SUITE": "codegen-mappings"}
        with tempfile.TemporaryDirectory(prefix="f-") as directory:
            home = Path(directory) / "r"
            home.mkdir(mode=0o700)
            settings = evidence.begin(home, environment, nonce="a" * 32)
            configured = Path("/home/ferrule-runner/ci-tmp/jobs") / settings["FERRULE_CI_EVIDENCE_ID"] / "tmp/sccacheABC123/sock"
            self.assertLess(len(os.fsencode(configured)), 108)
            startup = Path(settings["TMPDIR"]) / "sccacheABC123"
            startup.mkdir()
            endpoint = str(startup / "sock")
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
                server.bind(endpoint)
                server.listen(1)
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                    client.connect(endpoint)
                    accepted, _ = server.accept()
                    with accepted:
                        client.sendall(b"complete socket exchange")
                        self.assertEqual(accepted.recv(64), b"complete socket exchange")
            evidence.observe(home, environment | settings | {"FERRULE_CI_OBSERVED_JOB_STATE": "success"})

    def test_short_identity_still_refuses_another_matrix_job(self):
        settings = self.begin(FERRULE_CI_SUITE="codegen", FERRULE_CI_EVIDENCE_SUITE="codegen-crates")
        before = self.snapshot()
        with self.assertRaises(ValueError):
            evidence.observe(self.home, self.observer_environment(settings,
                FERRULE_CI_SUITE="codegen", FERRULE_CI_EVIDENCE_SUITE="codegen-mappings"))
        self.assertEqual(self.snapshot(), before)

    def test_existing_long_identity_record_can_still_be_observed(self):
        settings = self.begin()
        root = Path(settings["FERRULE_CI_EVIDENCE_ROOT"])
        legacy_id = "123-1-test-native-" + settings["FERRULE_CI_EVIDENCE_ID"]
        legacy_root = root.with_name(legacy_id)
        root.rename(legacy_root)
        # Author the historical record literally; the production observer must
        # validate it rather than rebuilding or renaming it.
        record_path = legacy_root / "receipts/UNARCHIVED.json"
        record = json.loads(record_path.read_bytes())
        record["job_id"] = legacy_id
        record_path.chmod(0o600)
        record_path.write_text(json.dumps(record))
        record_path.chmod(0o444)
        original = record_path.read_bytes()
        settings |= {"FERRULE_CI_EVIDENCE_ID": legacy_id,
                     "FERRULE_CI_EVIDENCE_ROOT": str(legacy_root)}
        evidence.observe(self.home, self.observer_environment(settings))
        self.assertEqual(record_path.read_bytes(), original)
        self.assertEqual(len(list(record_path.parent.glob("observation-*.json"))), 1)

    def test_symlinked_parent_refuses_and_preserves_outside(self):
        outside = self.home.parent / "outside"
        outside.mkdir()
        (outside / "marker").write_bytes(b"protected")
        (self.home / "ci-tmp").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(OSError):
            self.begin()
        self.assertEqual(list(outside.iterdir()), [outside / "marker"])
        self.assertEqual((outside / "marker").read_bytes(), b"protected")

    def test_writable_jobs_parent_refuses(self):
        parent = self.home / "ci-tmp/jobs"
        parent.mkdir(parents=True, mode=0o700)
        parent.chmod(0o777)
        with self.assertRaises(ValueError):
            self.begin()
        self.assertEqual(list(parent.iterdir()), [])

    def test_metadata_publication_fsync_and_readonly_order(self):
        events = []
        fsync, fchmod = evidence.os.fsync, evidence.os.fchmod
        def sync(fd):
            events.append(("sync", stat.S_ISDIR(os.fstat(fd).st_mode)))
            return fsync(fd)
        def readonly(fd, mode):
            events.append(("readonly", mode))
            return fchmod(fd, mode)
        with mock.patch.object(evidence.os, "fsync", side_effect=sync), \
                mock.patch.object(evidence.os, "fchmod", side_effect=readonly):
            self.begin()
        positions = [i for i, event in enumerate(events) if event == ("readonly", 0o444)]
        self.assertEqual(len(positions), 2)
        for position in positions:
            self.assertEqual(events[position - 1], ("sync", False))
            self.assertEqual(events[position + 1:position + 3], [("sync", False), ("sync", True)])

    def test_failure_before_export_retains_created_nodes(self):
        fsync = evidence.os.fsync
        def refuse_file_sync(fd):
            if stat.S_ISREG(os.fstat(fd).st_mode):
                raise OSError("fixture fsync refusal")
            return fsync(fd)
        with mock.patch.object(evidence.os, "fsync", side_effect=refuse_file_sync):
            with self.assertRaises(OSError):
                self.begin()
        jobs = list((self.home / "ci-tmp/jobs").iterdir())
        self.assertEqual(len(jobs), 1)
        self.assertTrue((jobs[0] / "tmp").is_dir())
        self.assertTrue((jobs[0] / "receipts").is_dir())
        retained = jobs[0] / "receipts/UNARCHIVED.json"
        self.assertTrue(retained.exists())
        self.assertEqual(stat.S_IMODE(retained.stat().st_mode), 0o600)
        self.assertEqual(json.loads(retained.read_bytes())["state"], "UNARCHIVED")

    def test_canary_is_metadata_only_and_excludes_environment_secrets(self):
        settings = self.begin(GITHUB_TOKEN="secret-token", SECRET="secret-value")
        data = Path(settings["FERRULE_CI_EVIDENCE_CANARY"]).read_bytes()
        record = json.loads(data)
        self.assertEqual(set(record), {"schema", "state", "closure", "archive_verified",
                         "created_ns", "identity", "job_id", "scope", "producer_tmpdir_compliance"})
        for excluded in (b"secret-token", b"secret-value", str(self.home).encode()):
            self.assertNotIn(excluded, data)
        self.assertIn("not complete job evidence", record["scope"])

    def test_observation_never_promotes_closure_or_archive(self):
        settings = self.begin()
        root = Path(settings["FERRULE_CI_EVIDENCE_ROOT"])
        initial = (root / "receipts/UNARCHIVED.json").read_bytes()
        evidence.observe(self.home, self.observer_environment(settings, FERRULE_CI_CANARY_OUTCOME="success"))
        record = json.loads(next((root / "receipts").glob("observation-*.json")).read_bytes())
        self.assertEqual((record["state"], record["closure"], record["archive_verified"]),
                         ("UNARCHIVED", "UNQUALIFIED", False))
        self.assertEqual((record["job_state"], record["canary_outcome"]), ("success", "success"))
        self.assertEqual((root / "receipts/UNARCHIVED.json").read_bytes(), initial)

    def test_canary_failure_is_separate_from_ordinary_success(self):
        settings = self.begin()
        evidence.observe(self.home, self.observer_environment(settings, FERRULE_CI_CANARY_OUTCOME="failure"))
        receipts = Path(settings["FERRULE_CI_EVIDENCE_ROOT"]) / "receipts"
        record = json.loads(next(receipts.glob("observation-*.json")).read_bytes())
        self.assertEqual((record["job_state"], record["canary_outcome"]), ("success", "failure"))
        self.assertFalse(record["archive_verified"])

    def test_wrong_root_identity_and_state_refuse_without_mutation(self):
        settings = self.begin()
        before = self.snapshot()
        for changes in ({"FERRULE_CI_EVIDENCE_ROOT": str(self.home)},
                        {"FERRULE_CI_EVIDENCE_ID": "wrong"},
                        {"FERRULE_CI_OBSERVED_JOB_STATE": "closed"}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                evidence.observe(self.home, self.observer_environment(settings, **changes))
        self.assertEqual(self.snapshot(), before)

    def test_missing_observation_root_is_not_recreated(self):
        job_id = "123-1-test-native-" + "0" * 32
        settings = {"FERRULE_CI_EVIDENCE_ID": job_id,
                    "FERRULE_CI_EVIDENCE_ROOT": str(self.home / "ci-tmp/jobs" / job_id)}
        with self.assertRaises(FileNotFoundError):
            evidence.observe(self.home, self.observer_environment(settings))
        self.assertFalse((self.home / "ci-tmp").exists())

    def test_fifo_metadata_refuses_without_blocking_or_mutating_outside(self):
        settings = self.begin()
        record = Path(settings["FERRULE_CI_EVIDENCE_ROOT"]) / "receipts/UNARCHIVED.json"
        record.unlink()  # Fixture substitution only; production helper has no unlink.
        os.mkfifo(record, 0o600)
        sentinel = self.home.parent / "outside-sentinel"
        sentinel.write_bytes(b"protected")
        with self.assertRaises(ValueError):
            evidence.observe(self.home, self.observer_environment(settings))
        self.assertTrue(stat.S_ISFIFO(record.lstat().st_mode))
        self.assertEqual(sentinel.read_bytes(), b"protected")

    def test_changed_record_refuses_observation(self):
        settings = self.begin()
        initial = Path(settings["FERRULE_CI_EVIDENCE_ROOT"]) / "receipts/UNARCHIVED.json"
        record = json.loads(initial.read_bytes())
        record["archive_verified"] = True
        initial.chmod(0o600)
        initial.write_text(json.dumps(record))
        initial.chmod(0o444)
        with self.assertRaises(ValueError):
            evidence.observe(self.home, self.observer_environment(settings))
        self.assertEqual(len(list(initial.parent.glob("observation-*.json"))), 0)

    def test_record_read_allows_only_access_time_change(self):
        settings = self.begin()
        record_path = Path(settings["FERRULE_CI_EVIDENCE_ROOT"]) / "receipts/UNARCHIVED.json"
        expected = json.loads(record_path.read_bytes())
        fields = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
                  "st_size", "st_mtime_ns", "st_ctime_ns")
        original_fstat = os.fstat
        observations = []
        def access_time_only(fd):
            row = original_fstat(fd)
            current = SimpleNamespace(**{field: getattr(row, field) for field in fields},
                                      st_atime_ns=1000000000 + len(observations),
                                      st_atime=1.0 + len(observations))
            observations.append(current)
            return current
        directory = os.open(record_path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            with mock.patch.object(evidence.os, "fstat", side_effect=access_time_only):
                actual = evidence.read_record(directory, "UNARCHIVED.json")
        finally:
            os.close(directory)
        self.assertEqual(actual, expected)
        self.assertEqual(len(observations), 2)
        self.assertEqual(observations[1].st_atime_ns, observations[0].st_atime_ns + 1)
        self.assertNotEqual(observations[0], observations[1])
        self.assertTrue(all(getattr(observations[0], field) == getattr(observations[1], field)
                            for field in fields))


if __name__ == "__main__":
    unittest.main()
