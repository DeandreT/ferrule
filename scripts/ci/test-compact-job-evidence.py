#!/usr/bin/env python3
"""Verify lossless space recovery, producer closure, and refusal before deletion."""
import ast
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


evidence = load("compact-job-evidence")
prepare = load("prepare-ovh-job")
job_evidence = load("job-evidence")


class HookAncestryTests(unittest.TestCase):
    """Resolve real fixture symlinks against an entirely synthetic process tree."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ferrule-hook-ancestry-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.uid = 1000
        self.configured_worker = self.root / "actions-runner/bin/Runner.Worker"
        self.processes = {}
        self.proc = Mock()
        self.proc.__truediv__ = Mock(side_effect=self.processes.__getitem__)
        self.proc.iterdir.side_effect = AssertionError("Supply a synthetic process census")
        self.getpid = Mock(return_value=40)
        patches = (
            patch.object(evidence, "Path", side_effect=self.proc_root),
            patch.object(evidence, "WORKER", str(self.configured_worker)),
            patch.object(evidence.os, "getpid", self.getpid),
            patch.object(evidence.os, "getuid", return_value=self.uid),
        )
        for replacement in patches:
            replacement.start()
            self.addCleanup(replacement.stop)

    def proc_root(self, path):
        self.assertEqual(path, "/proc")
        return self.proc

    def marker(self, name):
        # Ordinary identity markers, not executable programs or live runner files.
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("self-authored path identity fixture\n")
        return path

    def process(self, pid, parent, *, executable=None, owner=None):
        fields = Mock()
        fields.read_text.return_value = f"{pid} (fixture ) process) S {parent}"
        entries = {"stat": fields}
        if executable is not None:
            directory = self.root / "proc" / str(pid)
            directory.mkdir(parents=True)
            link = directory / "exe"
            link.symlink_to(executable)
            entries["exe"] = link
        process = Mock()
        process.name = str(pid)
        process.stat.return_value.st_uid = self.uid if owner is None else owner
        # Foreign ancestors can omit exe entirely: inspecting it fails the test.
        process.__truediv__ = Mock(side_effect=entries.__getitem__)
        self.processes[str(pid)] = process
        return process

    def hook(self, *, versioned=True):
        directory = "bin.2.337.0" if versioned else "bin"
        worker = self.marker(f"actions-runner/{directory}/Runner.Worker")
        if versioned:
            self.configured_worker.parent.symlink_to(directory, target_is_directory=True)
        self.process(40, 30, executable=self.marker("tools/python3"))
        self.process(30, 20, executable=worker)
        self.process(20, 1, executable=self.marker("tools/launcher"))
        self.process(1, 0, owner=self.uid + 1)
        return worker

    def test_unversioned_worker_returns_complete_ancestry(self):
        self.hook(versioned=False)
        self.assertEqual(evidence.hook_ancestry(), {40, 30, 20, 1})

    def test_configured_symlink_accepts_exact_versioned_worker_and_full_ancestry(self):
        worker = self.hook()
        self.assertNotEqual(worker, self.configured_worker)
        self.assertTrue(self.configured_worker.parent.is_symlink())
        self.assertEqual(evidence.hook_ancestry(), {40, 30, 20, 1})

    def test_same_basename_other_installation_and_unselected_version_refuse(self):
        self.hook()
        executable = self.processes["30"] / "exe"
        for name in ("unrelated/Runner.Worker",
                     "other-actions-runner/bin/Runner.Worker",
                     "actions-runner/bin.2.336.0/Runner.Worker"):
            with self.subTest(executable=name):
                target = self.marker(name)
                executable.unlink()
                executable.symlink_to(target)
                with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
                    evidence.hook_ancestry()

    def test_configured_symlink_cannot_select_a_worker_outside_the_installation(self):
        self.hook()
        worker = self.marker("actions-runner-other/bin/Runner.Worker")
        bin_link = self.configured_worker.parent
        bin_link.unlink()
        bin_link.symlink_to(worker.parent, target_is_directory=True)
        executable = self.processes["30"] / "exe"
        executable.unlink()
        executable.symlink_to(worker)
        with self.assertRaisesRegex(ValueError, "outside its runner installation"):
            evidence.hook_ancestry()

    def test_foreign_owned_worker_ancestor_does_not_authorize_compaction(self):
        self.hook()
        worker = self.processes["30"]
        worker.stat.return_value.st_uid = self.uid + 1
        with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
            evidence.hook_ancestry()
        worker.__truediv__.assert_called_once_with("stat")

    def test_foreign_init_is_traversed_without_inspecting_its_executable(self):
        self.hook()
        self.assertEqual(evidence.hook_ancestry(), {40, 30, 20, 1})
        self.processes["1"].__truediv__.assert_called_once_with("stat")

    def test_missing_or_unreadable_owner_or_parent_metadata_after_worker_refuses(self):
        self.hook()
        ancestor = self.processes["20"]
        for metadata, read in (("owner", ancestor.stat),
                               ("parent", (ancestor / "stat").read_text)):
            for error in (FileNotFoundError, PermissionError):
                with self.subTest(metadata=metadata, error=error.__name__):
                    read.side_effect = error("required synthetic ancestor metadata")
                    try:
                        with self.assertRaises(error):
                            evidence.hook_ancestry()
                    finally:
                        read.side_effect = None
                    self.processes["30"].__truediv__.assert_any_call("exe")

    def test_worker_outside_ancestry_cannot_authorize_an_administrator(self):
        self.hook()
        self.process(90, 1, executable=self.marker("tools/administrator"))
        self.getpid.return_value = 90
        with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
            evidence.producer_closure(processes=[self.processes["30"]])
        self.processes["30"].stat.assert_not_called()
        self.proc.iterdir.assert_not_called()

    def test_same_account_nonancestor_refuses_even_when_it_is_an_exact_worker(self):
        worker = self.hook()
        for pid, executable in ((60, self.marker("tools/earlier-producer")), (70, worker)):
            with self.subTest(pid=pid, executable=executable):
                producer = self.process(pid, 1, executable=executable)
                with self.assertRaisesRegex(ValueError, f"still alive: {pid}"):
                    evidence.producer_closure(processes=[producer])
                producer.__truediv__.assert_not_called()
        self.proc.iterdir.assert_not_called()

    def test_vanished_nonancestor_is_ignored(self):
        self.hook()
        vanished = self.process(60, 1)
        vanished.stat.side_effect = FileNotFoundError("synthetic producer exited")
        evidence.producer_closure(processes=[vanished])
        vanished.stat.assert_called_once_with()
        self.proc.iterdir.assert_not_called()

    def test_foreign_nonancestor_is_not_a_runner_producer(self):
        self.hook()
        foreign = self.process(60, 1, owner=self.uid + 1)
        evidence.producer_closure(processes=[foreign])
        foreign.stat.assert_called_once_with()
        foreign.__truediv__.assert_not_called()
        self.proc.iterdir.assert_not_called()

    def test_unreadable_nonancestor_metadata_refuses(self):
        self.hook()
        unknown = self.process(60, 1)
        unknown.stat.side_effect = PermissionError("synthetic producer unreadable")
        with self.assertRaises(PermissionError):
            evidence.producer_closure(processes=[unknown])
        self.proc.iterdir.assert_not_called()


class EvidenceCompactionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ferrule-evidence-compaction-")
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.scratch = self.home / "ci-tmp"
        self.archives = self.home / "ci-evidence"
        self.scratch.mkdir()
        self.archives.mkdir()
        self.name = "ferrule-codegen-123-456"
        self.original = self.scratch / self.name
        self.original.mkdir()
        (self.original / "empty").mkdir()
        (self.original / "original.log").write_bytes(b"complete failure\x00\xff\n")
        (self.original / "original.log").chmod(0o600)
        (self.original / "large-record.txt").write_bytes(b"complete repeated record\n" * 800000)
        self.outside = self.home / "protected.txt"
        self.outside.write_bytes(b"unrelated original")
        (self.original / "external-link").symlink_to(self.outside)

    def archive_one(self, **options):
        options.setdefault("closed", lambda: None)
        scratch = evidence.owned_directory(self.scratch)
        archives = evidence.owned_directory(self.archives)
        try:
            return evidence.compact_directory(scratch, archives, self.name, **options)
        finally:
            os.close(scratch)
            os.close(archives)

    def assert_preserved(self):
        self.assertEqual((self.original / "original.log").read_bytes(),
                         b"complete failure\x00\xff\n")
        self.assertEqual(self.outside.read_bytes(), b"unrelated original")

    def test_roundtrip_keeps_every_body_empty_directory_link_and_metadata(self):
        result = self.archive_one()
        self.assertFalse(self.original.exists())
        archive = self.archives / result["archive"]
        with archive.open("rb") as source:
            self.assertEqual(hashlib.file_digest(source, "sha256").hexdigest(),
                             result["archive_sha256"])
        with archive.open("rb") as source:
            evidence.verify_archive(source, result["members"])
        with tarfile.open(archive) as source:
            self.assertEqual(source.extractfile("original.log").read(),
                             b"complete failure\x00\xff\n")
            self.assertEqual(source.getmember("original.log").mode, 0o600)
            self.assertTrue(source.getmember("empty").isdir())
            self.assertEqual(source.getmember("external-link").linkname, str(self.outside))
        manifest = next(self.archives.glob("*.json"))
        self.assertEqual(json.loads(manifest.read_text()), result)
        self.assertLess(archive.stat().st_size, 1024**2)
        self.assertEqual(self.outside.read_bytes(), b"unrelated original")

    def test_failed_readback_keeps_original_and_removes_only_pending_copy(self):
        def failure(_source, _records):
            raise ValueError("readback failed")
        with self.assertRaisesRegex(ValueError, "readback failed"):
            self.archive_one(verify=failure)
        self.assert_preserved()
        self.assertEqual(list(self.archives.iterdir()), [])

    def test_long_original_names_fit_archive_filesystem_limits(self):
        name = "ferrule-" + "x" * 212
        self.original.rename(self.scratch / name)
        self.name = name
        self.original = self.scratch / name
        manifest = self.archive_one()
        self.assertEqual(manifest["original_directory"], name)
        self.assertTrue((self.archives / manifest["archive"]).is_file())

    def test_same_filesystem_bind_mount_also_refuses(self):
        fd = evidence.owned_directory(self.original)
        try:
            path = str(self.original / "nested mount").replace(" ", "\\040")
            mounts = f"42 1 0:1 / {path} rw - ext4 device rw\n"
            with self.assertRaisesRegex(ValueError, "Mounted evidence"):
                evidence.reject_mounts(fd, mountinfo=mounts)
        finally:
            os.close(fd)
        self.assert_preserved()

    def test_missing_or_malformed_mount_inventory_refuses(self):
        invalid = ("", "\n", "incomplete\n",
                   "42 1 0:1 / relative rw - ext4 device rw\n",
                   "42 1 0:1 / /other\\bad rw - ext4 device rw\n",
                   "42 1 0:1 /unknown\\000 /other rw - ext4 device rw\n",
                   "42 1 0:1 / /other rw - ext4 device rw extra\n",
                   "42 1 0:1 net:[unknown] /other rw - nsfs device rw\n",
                   "42 1 0:1 net:[42] /other rw - ext4 device rw\n")
        valid = "42 1 0:1 / /unrelated rw - ext4 device rw\n"
        fd = evidence.owned_directory(self.original)
        try:
            for text in invalid + tuple(valid + row for row in invalid if row):
                with self.subTest(inventory=text):
                    with self.assertRaisesRegex(ValueError, "Mount inventory"):
                        evidence.reject_mounts(fd, mountinfo=text)
        finally:
            os.close(fd)
        self.assert_preserved()

    def test_producer_reappearing_before_removal_preserves_original(self):
        with self.assertRaisesRegex(ValueError, "producer"):
            self.archive_one(closed=Mock(side_effect=ValueError("producer alive")))
        self.assert_preserved()

    def test_original_change_after_readback_refuses_removal(self):
        def changed(source, records):
            evidence.verify_archive(source, records)
            (self.original / "added-record").write_bytes(b"new evidence")
        with self.assertRaisesRegex(ValueError, "Evidence changed"):
            self.archive_one(verify=changed)
        self.assert_preserved()
        self.assertEqual((self.original / "added-record").read_bytes(), b"new evidence")

    def test_root_replaced_after_readback_never_removes_replacement(self):
        parked = self.scratch / "parked"
        def changed(source, records):
            evidence.verify_archive(source, records)
            self.original.rename(parked)
            self.original.mkdir()
            (self.original / "replacement").write_bytes(b"protected replacement")
        with self.assertRaisesRegex(ValueError, "root was replaced"):
            self.archive_one(verify=changed)
        self.assertEqual((self.original / "replacement").read_bytes(), b"protected replacement")
        self.assertEqual((parked / "original.log").read_bytes(), b"complete failure\x00\xff\n")

    def test_redirected_root_refuses_without_following_link(self):
        parked = self.scratch / "parked"
        self.original.rename(parked)
        self.original.symlink_to(parked, target_is_directory=True)
        with self.assertRaises(OSError):
            self.archive_one()
        self.assert_preserved()

    def test_special_file_hardlink_and_writable_directory_refuse(self):
        for kind in ("fifo", "hardlink", "permissions"):
            with self.subTest(kind=kind):
                leaf = self.original / "unsafe"
                if kind == "fifo":
                    os.mkfifo(leaf)
                elif kind == "hardlink":
                    os.link(self.outside, leaf)
                else:
                    leaf.mkdir(mode=0o777)
                    leaf.chmod(0o777)
                with self.assertRaises(ValueError):
                    self.archive_one()
                self.assert_preserved()
                if leaf.is_dir():
                    leaf.rmdir()
                else:
                    leaf.unlink()

    def test_mismatched_archive_body_inventory_or_metadata_refuses(self):
        for kind in ("body", "extra", "missing", "mode"):
            with self.subTest(kind=kind):
                data = io.BytesIO()
                records = {"file": {"kind": "file", "size": 3, "mode": 0o600,
                                    "uid": 1, "gid": 2, "mtime_ns": 0,
                                    "sha256": hashlib.sha256(b"abc").hexdigest()}}
                with tarfile.open(fileobj=data, mode="w:gz") as output:
                    if kind != "missing":
                        member = tarfile.TarInfo("extra" if kind == "extra" else "file")
                        member.size, member.uid, member.gid = 3, 1, 2
                        member.mode = 0o644 if kind == "mode" else 0o600
                        output.addfile(member, io.BytesIO(b"xyz" if kind == "body" else b"abc"))
                data.seek(0)
                with self.assertRaises(ValueError):
                    evidence.verify_archive(data, records)

    def test_headroom_keeps_originals_and_does_not_require_maintenance(self):
        closed = Mock(side_effect=ValueError("should not run"))
        result = evidence.compact(self.home, 20, available=lambda _: 20 * evidence.GIB,
                                  closed=closed)
        self.assertEqual(result, [])
        closed.assert_not_called()
        self.assert_preserved()

    def test_pressure_stops_at_goal_and_preserves_small_and_unknown_records(self):
        other = self.scratch / "unrecognized"
        other.mkdir()
        (other / "record").write_bytes(b"keep")
        tiny = self.scratch / "ferrule-tiny"
        tiny.mkdir()
        (tiny / "record").write_bytes(b"small original")
        result = evidence.compact(
            self.home, 20, closed=lambda: None,
            available=lambda _: (14 if self.original.exists() else 21) * evidence.GIB)
        self.assertEqual(len(result), 1)
        self.assertEqual((tiny / "record").read_bytes(), b"small original")
        self.assertEqual((other / "record").read_bytes(), b"keep")
        self.assertFalse(self.original.exists())

    def test_process_closure_failure_preserves_every_original(self):
        with self.assertRaisesRegex(ValueError, "alive"):
            evidence.compact(self.home, 20, available=lambda _: 14 * evidence.GIB,
                             closed=Mock(side_effect=ValueError("producer alive")))
        self.assert_preserved()

    def test_special_fixture_stays_while_other_eligible_records_are_compacted(self):
        unsafe = self.scratch / "ferrule-special-fixture"
        unsafe.mkdir()
        os.mkfifo(unsafe / "deliberate-fifo")
        evidence.compact(self.home, 20, closed=lambda: None,
                         available=lambda _: (14 if self.original.exists() else 21) * evidence.GIB)
        self.assertTrue((unsafe / "deliberate-fifo").exists())
        self.assertFalse(self.original.exists())

    def test_redirected_archive_store_and_scratch_parent_refuse(self):
        for directory in (self.archives, self.scratch):
            with self.subTest(directory=directory.name):
                parked = self.home / (directory.name + "-parked")
                directory.rename(parked)
                directory.symlink_to(parked, target_is_directory=True)
                with self.assertRaises(OSError):
                    evidence.compact(self.home, 20, available=lambda _: 14 * evidence.GIB,
                                     closed=lambda: None)
                directory.unlink()
                parked.rename(directory)
                self.assert_preserved()

    def test_direct_admin_invocation_cannot_claim_pre_job_closure(self):
        # Tests themselves can run beneath a real Actions worker. Model an
        # administrator's ancestry independently of the test harness's parent.
        worker = self.home / "runner/bin/Runner.Worker"
        worker.parent.mkdir(parents=True)
        worker.touch()
        with patch.object(evidence, "WORKER", str(worker)):
            with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
                evidence.producer_closure(processes=[])

    def test_foreign_init_ancestor_does_not_require_executable_access(self):
        root_process = Mock()
        root_process.stat.return_value.st_uid = os.getuid() + 1
        root_process.__truediv__ = Mock(return_value=Mock())
        root_process.__truediv__.return_value.read_text.return_value = "1 (init) S 0"
        def realpath(path, *, strict=False):
            if path == evidence.WORKER and strict:
                return path
            raise PermissionError("root exe")

        with patch.object(evidence.os, "getpid", return_value=1), \
             patch.object(evidence, "Path", return_value=Mock(__truediv__=Mock(return_value=root_process))), \
             patch.object(evidence.os.path, "realpath", side_effect=realpath):
            with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
                evidence.hook_ancestry()

    def test_real_live_producer_without_open_evidence_files_vetoes_compaction(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            with patch.object(evidence, "hook_ancestry", return_value={os.getpid()}):
                with self.assertRaisesRegex(ValueError, "still alive"):
                    evidence.producer_closure(processes=[Path("/proc") / str(child.pid)])
        finally:
            child.terminate()
            child.wait(timeout=5)
        self.assert_preserved()


class CurrentJobCompactionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ferrule-current-job-compaction-")
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.environment = {"GITHUB_REPOSITORY": "DeandreT/ferrule", "GITHUB_RUN_ID": "123",
                            "GITHUB_RUN_ATTEMPT": "1", "GITHUB_JOB": "generated-backends",
                            "GITHUB_SHA": "a" * 40, "FERRULE_CI_SUITE": "codegen",
                            "FERRULE_CI_EVIDENCE_SUITE": "codegen-mappings"}
        self.settings = job_evidence.begin(self.home, self.environment, nonce="a" * 32)
        self.original = Path(self.settings["FERRULE_CI_EVIDENCE_ROOT"])
        self.receipt = self.original / "receipts/UNARCHIVED.json"
        self.receipt_bytes = self.receipt.read_bytes()
        self.body = b"complete current-job record\x00\xff\n" * 700000
        self.large = Path(self.settings["TMPDIR"]) / "ferrule-generated" / "complete.bin"
        self.large.parent.mkdir()
        self.large.write_bytes(self.body)
        self.archives = self.home / "ci-evidence"
        self.outside = self.home / "protected-original"
        self.outside.write_bytes(b"unrelated original")

    def compact(self, **options):
        options.setdefault("closed", lambda: None)
        return evidence.compact(self.home, 20,
                                available=lambda _: (14 if self.original.exists() else 21) * evidence.GIB,
                                **options)

    def archive_one(self, **options):
        options.setdefault("closed", lambda: None)
        self.archives.mkdir(exist_ok=True, mode=0o700)
        parent = evidence.owned_directory(self.original.parent)
        archives = evidence.owned_directory(self.archives)
        try:
            return evidence.compact_directory(parent, archives, self.original.name,
                                               jobs=True, **options)
        finally:
            os.close(parent)
            os.close(archives)

    def assert_preserved(self):
        self.assertEqual(self.large.read_bytes(), self.body)
        self.assertEqual(self.receipt.read_bytes(), self.receipt_bytes)
        self.assertEqual(self.outside.read_bytes(), b"unrelated original")

    def test_current_job_roundtrip_retains_complete_scratch_and_immutable_receipts(self):
        job_evidence.observe(self.home, self.environment | self.settings |
                             {"FERRULE_CI_OBSERVED_JOB_STATE": "failure"})
        (self.original / "tmp/empty").mkdir()
        (self.original / "tmp/external-link").symlink_to(self.outside)
        receipts = {str(path.relative_to(self.original)): path.read_bytes()
                    for path in (self.original / "receipts").iterdir()}
        results = self.compact()
        self.assertEqual(len(results), 1)
        result = results[0]
        self.assertEqual(result["original_directory"], "jobs/" + self.original.name)
        self.assertFalse(self.original.exists())
        self.assertTrue(self.original.parent.is_dir())
        archive = self.archives / result["archive"]
        with archive.open("rb") as source:
            self.assertEqual(hashlib.file_digest(source, "sha256").hexdigest(),
                             result["archive_sha256"])
        with archive.open("rb") as source:
            evidence.verify_archive(source, result["members"])
        with tarfile.open(archive) as source:
            self.assertEqual(source.extractfile("tmp/ferrule-generated/complete.bin").read(), self.body)
            self.assertTrue(source.getmember("tmp/empty").isdir())
            self.assertEqual(source.getmember("tmp/external-link").linkname, str(self.outside))
            for name, original in receipts.items():
                self.assertEqual(source.extractfile(name).read(), original)
                self.assertEqual(source.getmember(name).mode, 0o444)
        manifest = next(self.archives.glob("*.json"))
        self.assertEqual(json.loads(manifest.read_bytes()), result)
        self.assertEqual(self.outside.read_bytes(), b"unrelated original")

    def test_current_job_compacts_with_only_installed_neighbor_helpers(self):
        install = self.home / "installed-tools"
        install.mkdir()
        for name in ("compact-job-evidence", "job-evidence"):
            shutil.copyfile(Path(__file__).with_name(name + ".py"), install / (name + ".py"))
        spec = importlib.util.spec_from_file_location("installed_compactor", install / "compact-job-evidence.py")
        installed = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(installed)
        results = installed.compact(self.home, 20, closed=lambda: None,
                                   available=lambda _: (14 if self.original.exists() else 21) * evidence.GIB)
        self.assertEqual(len(results), 1)
        self.assertFalse(self.original.exists())

    def test_small_unknown_and_unprepared_job_trees_are_preserved(self):
        small = job_evidence.begin(self.home, self.environment, nonce="b" * 32)
        small_root = Path(small["FERRULE_CI_EVIDENCE_ROOT"])
        unprepared = self.original.parent / ("c" * 32)
        unprepared.mkdir(mode=0o700)
        (unprepared / "complete.bin").write_bytes(self.body)
        unknown = self.original.parent / "unrecognized"
        unknown.mkdir(mode=0o700)
        (unknown / "complete.bin").write_bytes(self.body)
        results = self.compact()
        self.assertEqual(len(results), 1)
        self.assertTrue(small_root.is_dir())
        self.assertEqual((unprepared / "complete.bin").read_bytes(), self.body)
        self.assertEqual((unknown / "complete.bin").read_bytes(), self.body)

    def test_changed_nonce_pin_state_or_github_identity_never_admits_a_job(self):
        initial = json.loads(self.receipt_bytes)
        cases = ([], initial | {"nonce": "b" * 32}, initial | {"job_id": "b" * 32},
                 initial | {"job_directory": initial["job_directory"] | {"inode": -1}},
                 initial | {"identity": initial["identity"] | {"repository": "elsewhere/repo"}},
                 initial | {"identity": initial["identity"] | {"run_id": "../123"}},
                 initial | {"identity": initial["identity"] | {"suite": "unknown"}},
                 initial | {"schema": "unknown"}, initial | {"archive_verified": True},
                 initial | {"closure": "QUALIFIED"}, initial | {"state": "ARCHIVED"})
        for changed in cases:
            with self.subTest(record=changed):
                self.receipt.chmod(0o600)
                self.receipt.write_text(json.dumps(changed))
                self.receipt.chmod(0o444)
                before = self.receipt.read_bytes()
                self.assertEqual(self.compact(), [])
                self.assertEqual(self.receipt.read_bytes(), before)
                self.assertEqual(self.large.read_bytes(), self.body)
                self.assertEqual(list(self.archives.glob("*.tar.gz")), [])
                self.assertEqual(list(self.archives.glob("*.pending")), [])

    def test_redirected_or_writable_jobs_parent_refuses_without_removal(self):
        parent = self.original.parent
        parked = parent.with_name("parked-jobs")
        parent.rename(parked)
        parent.symlink_to(parked, target_is_directory=True)
        with self.assertRaises(OSError):
            self.compact()
        self.assert_preserved()
        parent.unlink()
        parked.rename(parent)
        parent.chmod(0o777)
        with self.assertRaisesRegex(ValueError, "unsafe ownership|Unsafe jobs"):
            self.compact()
        self.assert_preserved()

    def test_mounts_at_jobs_parent_job_root_or_descendant_preserve_originals(self):
        reject = evidence.reject_mounts
        for point in (self.original.parent, self.original, self.large):
            with self.subTest(mount=point):
                mounts = f"42 1 0:1 / {point} rw - ext4 device rw\n"
                with patch.object(evidence, "reject_mounts",
                                  side_effect=lambda fd, **options: reject(
                                      fd, mountinfo=mounts, **options)):
                    with self.assertRaisesRegex(ValueError, "Mounted evidence"):
                        self.compact()
                self.assert_preserved()

    def assert_mount_inventory_refused(self, text):
        read_text = evidence.Path.read_text

        def inventory(path, *args, **options):
            if path == Path("/proc/self/mountinfo"):
                return text
            return read_text(path, *args, **options)

        with patch.object(evidence.Path, "read_text", new=inventory):
            with self.assertRaisesRegex(ValueError, "Mount inventory|Mounted evidence"):
                self.compact()
        self.assert_preserved()
        self.assertEqual(list(self.archives.glob("*.tar.gz")), [])
        self.assertEqual(list(self.archives.glob("*.pending")), [])

    def test_empty_mount_inventory_preserves_originals(self):
        self.assert_mount_inventory_refused("")

    def test_relative_mountpoint_preserves_originals(self):
        self.assert_mount_inventory_refused(
            "42 1 0:1 / relative rw - ext4 device rw\n")

    def test_exact_scratch_parent_mount_preserves_originals(self):
        self.assert_mount_inventory_refused(
            f"42 1 0:1 / {self.home / 'ci-tmp'} rw - ext4 device rw\n")

    def test_exact_archive_parent_mount_preserves_originals(self):
        self.assert_mount_inventory_refused(
            f"42 1 0:1 / {self.archives} rw - ext4 device rw\n")

    def test_unrelated_namespace_and_containing_home_mounts_allow_compaction(self):
        text = "".join(f"42 1 0:1 {name}:[42] /unrelated/{name} rw - nsfs device rw\n"
                       for name in ("cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"))
        text += f"42 1 0:1 / {self.home} rw - ext4 device rw\n"
        read_text = evidence.Path.read_text

        def inventory(path, *args, **options):
            return text if path == Path("/proc/self/mountinfo") else read_text(path, *args, **options)

        with patch.object(evidence.Path, "read_text", new=inventory):
            results = self.compact()
        self.assertEqual(len(results), 1)
        self.assertFalse(self.original.exists())
        with (self.archives / results[0]["archive"]).open("rb") as source:
            evidence.verify_archive(source, results[0]["members"])
        self.assertEqual(self.outside.read_bytes(), b"unrelated original")

    def test_live_producer_without_open_job_files_vetoes_current_job_compaction(self):
        child = subprocess.Popen([sys.executable, "-c", "print('ready',flush=True); input()"],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(child.stdout.readline(), "ready\n")
            with patch.object(evidence, "hook_ancestry", return_value={os.getpid()}):
                with self.assertRaisesRegex(ValueError, "still alive"):
                    self.compact(closed=lambda: evidence.producer_closure(
                        processes=[Path("/proc") / str(child.pid)]))
        finally:
            child.communicate("release\n", timeout=5)
        self.assertEqual(child.returncode, 0)
        self.assert_preserved()

    def test_failed_current_job_archive_readback_preserves_originals(self):
        with self.assertRaisesRegex(ValueError, "readback failed"):
            self.archive_one(verify=Mock(side_effect=ValueError("readback failed")))
        self.assert_preserved()
        self.assertEqual(list(self.archives.iterdir()), [])

    def test_producer_reappearing_after_current_job_readback_preserves_originals(self):
        closed = Mock(side_effect=[None, ValueError("producer alive")])
        with self.assertRaisesRegex(ValueError, "producer alive"):
            self.archive_one(closed=closed)
        self.assert_preserved()
        self.assertEqual(list(self.archives.iterdir()), [])

    def test_current_job_replaced_after_readback_preserves_both_trees(self):
        parked = self.original.with_name("parked")
        def changed(source, records):
            evidence.verify_archive(source, records)
            self.original.rename(parked)
            self.original.mkdir(mode=0o700)
            (self.original / "replacement").write_bytes(b"protected replacement")
        with self.assertRaisesRegex(ValueError, "root was replaced"):
            self.archive_one(verify=changed)
        self.assertEqual((self.original / "replacement").read_bytes(), b"protected replacement")
        self.assertEqual((parked / "tmp/ferrule-generated/complete.bin").read_bytes(), self.body)
        self.assertEqual((parked / "receipts/UNARCHIVED.json").read_bytes(), self.receipt_bytes)
        self.assertEqual(list(self.archives.iterdir()), [])

    def test_legacy_long_job_identity_compacts_without_changing_its_receipt(self):
        legacy_id = "123-1-generated-backends-codegen-mappings-" + self.original.name
        self.original.rename(self.original.with_name(legacy_id))
        self.original = self.original.with_name(legacy_id)
        receipt = self.original / "receipts/UNARCHIVED.json"
        initial = json.loads(receipt.read_bytes()) | {"job_id": legacy_id}
        receipt.chmod(0o600)
        receipt.write_text(json.dumps(initial))
        receipt.chmod(0o444)
        original = receipt.read_bytes()
        results = self.compact()
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["original_directory"], "jobs/" + legacy_id)
        with tarfile.open(self.archives / results[0]["archive"]) as source:
            self.assertEqual(source.extractfile("receipts/UNARCHIVED.json").read(), original)


class PreJobTests(unittest.TestCase):
    def test_space_ready_skips_archive_and_cache_mutations(self):
        cache, records, stop = Mock(), Mock(), Mock()
        cache.GIB = evidence.GIB
        cache.headroom.return_value = 20 * evidence.GIB
        prepare.prepare(Path("/home/runner"), 16, cache=cache, evidence=records, stop_cache=stop)
        stop.assert_not_called()
        records.compact.assert_not_called()
        cache.reclaim.assert_not_called()

    def test_low_space_requires_producer_closure_before_archive_or_cache_removal(self):
        cache, records, stop = Mock(), Mock(), Mock()
        cache.GIB = evidence.GIB
        cache.headroom.return_value = 14 * evidence.GIB
        records.producer_closure.side_effect = ValueError("old job alive")
        with self.assertRaisesRegex(ValueError, "old job alive"):
            prepare.prepare(Path("/home/runner"), 16, cache=cache, evidence=records,
                            stop_cache=stop)
        stop.assert_called_once_with()
        records.compact.assert_not_called()
        cache.reclaim.assert_not_called()

    def test_both_storage_floors_are_preserved(self):
        for reserve in (8, 16):
            with self.subTest(reserve=reserve):
                cache, records, stop = Mock(), Mock(), Mock()
                cache.GIB = evidence.GIB
                cache.headroom.return_value = 4 * evidence.GIB
                home = Path("/home/runner")
                prepare.prepare(home, reserve, cache=cache, evidence=records, stop_cache=stop)
                records.compact.assert_called_once_with(home, reserve + 4,
                                                       available=cache.headroom)
                cache.reclaim.assert_called_once_with(home, reserve)

    def test_hook_routes_native_and_codegen_only_for_actual_worker(self):
        guard = Path(__file__).with_name("guard-ovh-job.sh").read_text()
        tree = ast.parse(guard.split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0])
        function = next(node for node in tree.body if isinstance(node, ast.FunctionDef)
                        and node.name == "prepare_admitted_worker")
        module = ast.Module(body=[function], type_ignores=[])
        for workflow, reserve in (("CI", "16"), ("Codegen CI", "8"), ("other", "16")):
            process = Mock()
            process.path.realpath.return_value = evidence.WORKER
            process.environ = {"GITHUB_WORKFLOW": workflow}
            runner = Mock()
            namespace = {"os": process, "subprocess": runner, "sys": sys}
            exec(compile(module, "guard", "exec"), namespace)
            namespace["prepare_admitted_worker"]()
            runner.run.assert_called_once_with(
                [sys.executable, "-I", "/opt/ferrule-runner-tools/prepare-ovh-job.py", reserve],
                check=True, timeout=600)


if __name__ == "__main__":
    unittest.main()
