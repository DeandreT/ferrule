#!/usr/bin/env python3
"""Verify lossless space recovery, producer closure, and refusal before deletion."""
import ast
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
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
        with patch.object(evidence, "WORKER", "/no-administrator-job-worker"):
            with self.assertRaisesRegex(ValueError, "actual pre-job hook"):
                evidence.producer_closure(processes=[])

    def test_foreign_init_ancestor_does_not_require_executable_access(self):
        root_process = Mock()
        root_process.stat.return_value.st_uid = os.getuid() + 1
        root_process.__truediv__ = Mock(return_value=Mock())
        root_process.__truediv__.return_value.read_text.return_value = "1 (init) S 0"
        with patch.object(evidence.os, "getpid", return_value=1), \
             patch.object(evidence, "Path", return_value=Mock(__truediv__=Mock(return_value=root_process))), \
             patch.object(evidence.os.path, "realpath", side_effect=PermissionError("root exe")):
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
