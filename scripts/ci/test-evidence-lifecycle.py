#!/usr/bin/env python3
"""Offline lifecycle qualification; all mutations stay in a private test root."""
import contextlib
import copy
import fcntl
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import time
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("lifecycle", Path(__file__).with_name("evidence-lifecycle.py"))
life = importlib.util.module_from_spec(spec)
spec.loader.exec_module(life)


class TestContainment(life.Containment):
    """Test-only authority: no processes are spawned in these fixtures."""
    def __init__(self):
        self.closed = True
        self.compliant = True
        self.held = False
        self.identities = []

    @contextlib.contextmanager
    def lease(self, preparation):
        life.require(self.closed and self.compliant, "Unproven containment")
        self.identities.append(preparation)
        self.held = True
        try:
            yield
        finally:
            self.held = False


class MemoryProvider(life.Provider):
    """Not a real durable provider; qualified only as an isolated test double."""
    def __init__(self, authority):
        self.authority = authority
        self.objects = {}
        self.fault = None
        self.until = time.time_ns() + 10**15
        self.reads = 0

    def put(self, key, source, digest, size):
        assert self.authority.held
        if self.fault == "upload":
            raise OSError("Interrupted upload")
        data = source.read()
        if key in self.objects:
            assert self.objects[key] == data
        self.objects[key] = data
        return life.ObjectRef("memory-test", "isolated-test", key,
                              "version-1" if self.fault != "version" else "",
                              digest, size)

    def read(self, ref, destination):
        assert self.authority.held
        self.reads += 1
        data = self.objects[ref.key]
        if self.fault == "download":
            destination.write(data[:3])
            raise OSError("Interrupted download")
        if self.fault == "truncated":
            data = data[:-1]
        if self.fault == "extra":
            data += b"unexpected"
        if self.fault == "corrupt":
            data = b"!" + data[1:]
        destination.write(data)

    def retention(self, ref):
        assert self.authority.held
        if self.fault == "retention-error":
            raise OSError("Retention lookup failed")
        if self.fault == "wrong-retention-version":
            ref = life.ObjectRef(ref.provider, ref.destination, ref.key, "other", ref.sha256, ref.size)
        return life.Retention(ref, self.until, self.fault != "unenforced")


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        # The caller can put this suite under its private on-disk cache via TMPDIR.
        self.temporary = tempfile.TemporaryDirectory(prefix="ferrule-lifecycle-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.home = self.base / "runner"
        self.home.mkdir(mode=0o700)
        self.state = self.base / "controller"
        self.state.mkdir(mode=0o700)
        self.job_id = "a" * 32
        self.env = {"GITHUB_REPOSITORY": "DeandreT/ferrule", "GITHUB_RUN_ID": "123",
                    "GITHUB_RUN_ATTEMPT": "1", "GITHUB_JOB": "test", "GITHUB_SHA": "b" * 40}
        settings = life.jobs.begin(self.home, self.env, nonce=self.job_id)
        self.root = Path(settings["FERRULE_CI_EVIDENCE_ROOT"])
        self.body = self.root / "tmp" / "diagnostic.txt"
        self.body.write_bytes(b"complete original evidence\x00\xff")
        (self.root / "tmp" / "empty").mkdir()
        self.authority = TestContainment()
        self.provider = MemoryProvider(self.authority)
        self.controller = life.Lifecycle(self.home, self.state, containment=self.authority,
            provider=self.provider, destination=("memory-test", "isolated-test"),
            retain_until_ns=time.time_ns() + 10**14, available=lambda: 100 * life.GIB)

    def run_job(self, retire=False):
        return self.controller.run(self.job_id, retire=retire)

    def record(self):
        return json.loads((self.state / (self.job_id + ".json")).read_bytes())

    def unchanged_on_refusal(self, error=(ValueError, OSError)):
        before = self.body.read_bytes()
        with self.assertRaises(error):
            self.run_job(retire=True)
        self.assertEqual(self.body.read_bytes(), before)
        self.assertFalse(self.authority.held)

    def test_preservation_then_exact_finite_retirement(self):
        other = self.home / "ci-cache" / "active"
        other.mkdir(parents=True)
        (other / "cache").write_bytes(b"active cache")
        second = life.jobs.begin(self.home, self.env, nonce="c" * 32)
        self.assertEqual(self.run_job(), "PRESERVED")
        self.assertTrue(self.body.exists())
        self.assertEqual(len(self.provider.objects), 2)
        self.assertEqual(self.run_job(retire=True), "RETIRED")
        self.assertFalse(self.root.exists())
        self.assertEqual((other / "cache").read_bytes(), b"active cache")
        self.assertTrue(Path(second["TMPDIR"]).exists())
        self.assertEqual(self.run_job(retire=True), "RETIRED")
        self.assertEqual(self.provider.reads, 4)

    def test_unconfigured_authority_never_creates_closed_record(self):
        self.controller.containment = life.Containment()
        self.unchanged_on_refusal()
        self.assertFalse((self.state / (self.job_id + ".json")).exists())

    def test_unproved_closure_or_tmp_compliance(self):
        for field in ("closed", "compliant"):
            with self.subTest(field=field):
                setattr(self.authority, field, False)
                self.unchanged_on_refusal()
                setattr(self.authority, field, True)

    def test_cancelled_and_failed_observations_do_not_prove_closure(self):
        for status in ("cancelled", "failure", "success", "skipped"):
            life.jobs.observe(self.home, self.env | {
                "FERRULE_CI_EVIDENCE_ID": self.job_id,
                "FERRULE_CI_EVIDENCE_ROOT": str(self.root),
                "FERRULE_CI_OBSERVED_JOB_STATE": status})
            self.authority.closed = False
            self.unchanged_on_refusal()
        self.authority.closed = True
        self.assertEqual(self.run_job(), "PRESERVED")

    def test_all_transfer_faults_preserve_originals_and_retry(self):
        for fault in ("upload", "download", "truncated", "extra", "corrupt", "version",
                      "retention-error", "wrong-retention-version", "unenforced"):
            with self.subTest(fault=fault):
                self.provider.fault = fault
                self.unchanged_on_refusal()
        self.provider.fault = None
        self.assertEqual(self.run_job(retire=True), "RETIRED")

    def test_unapproved_destination_and_expiring_retention_refuse(self):
        self.controller.destination = None
        self.unchanged_on_refusal()
        self.controller.destination = ("different", "destination")
        self.unchanged_on_refusal()
        self.controller.destination = ("memory-test", "isolated-test")
        self.provider.until = time.time_ns()
        self.unchanged_on_refusal()
        self.controller.retain_until_ns = time.time_ns() - 1
        self.unchanged_on_refusal()

    def test_no_real_provider_is_installed(self):
        self.controller.provider = life.Provider()
        self.unchanged_on_refusal()
        self.assertEqual(self.record()["state"], "ARCHIVED_LOCAL")

    def test_reserve_floors_cannot_be_disabled(self):
        for reserve in (8, 16):
            self.controller.reserve_gib = reserve
            self.controller.available = lambda: (reserve + 4) * life.GIB
            self.unchanged_on_refusal()
            self.assertEqual(self.record()["state"], "CLOSED_UNARCHIVED")
        self.controller.available = lambda: 100 * life.GIB
        self.assertEqual(self.run_job(), "PRESERVED")
        with self.assertRaises(ValueError):
            life.Lifecycle(self.home, self.state, reserve_gib=0)

    def test_symlink_and_hardlink_sources_refuse(self):
        for symlink in (True, False):
            link = self.root / "tmp" / "link"
            if symlink:
                link.symlink_to(self.body)
            else:
                os.link(self.body, link)
            self.unchanged_on_refusal()
            link.unlink()

    def test_fifo_source_refuses(self):
        os.mkfifo(self.root / "tmp" / "fifo")
        self.unchanged_on_refusal()

    def test_unowned_job_and_metadata_refuse(self):
        receipt = self.root / "receipts" / "UNARCHIVED.json"
        receipt.chmod(0o600)
        self.unchanged_on_refusal()
        receipt.chmod(0o444)
        data = json.loads(receipt.read_bytes())
        data["job_directory"]["inode"] += 1
        receipt.chmod(0o600)
        receipt.write_text(json.dumps(data))
        receipt.chmod(0o444)
        self.unchanged_on_refusal()

    def test_configured_source_owner_is_independent_of_controller_uid(self):
        owner = os.getuid()
        root = os.open(self.root, life.jobs.DIRECTORY_FLAGS)
        self.addCleanup(os.close, root)
        with mock.patch.object(os, "getuid", return_value=owner + 1):
            with self.assertRaises(ValueError):
                life.jobs.retained_job(root, self.job_id)
            self.assertEqual(life.jobs.retained_job(root, self.job_id, owner=owner)["job_id"], self.job_id)
            with self.assertRaises(ValueError):
                life.compact.inventory(root)
            self.assertIn("tmp/diagnostic.txt", life.compact.inventory(root, owner=owner))

    def test_unexpected_source_owner_refuses(self):
        self.controller.runner_uid = os.getuid() + 1
        self.unchanged_on_refusal()

    def test_readback_reserve_gate_also_applies_to_retries(self):
        self.run_job()
        self.controller.available = lambda: 20 * life.GIB
        self.unchanged_on_refusal()

    def test_unowned_state_refuses(self):
        self.run_job()
        path = self.state / (self.job_id + ".json")
        record = self.record()
        record["job_id"] = "d" * 32
        path.write_text(json.dumps(record))
        self.unchanged_on_refusal()

    def test_state_directory_may_not_be_inside_runner_home(self):
        self.controller.state_root = self.home
        self.unchanged_on_refusal()

    def test_state_fifo_and_concurrent_controller_refuse(self):
        journal = self.state / (self.job_id + ".json")
        os.mkfifo(journal, 0o600)
        self.unchanged_on_refusal()
        journal.unlink()
        with (self.state / ".lock").open("r+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.unchanged_on_refusal()

    def test_mount_inventory_failure_refuses(self):
        with mock.patch.object(life.compact, "reject_mounts", side_effect=ValueError("mounted")):
            self.unchanged_on_refusal()

    def test_readback_is_bounded_before_writing_excess(self):
        output = io.BytesIO()
        writer = life.BoundedWriter(output, 4)
        writer.write(b"123")
        with self.assertRaises(ValueError):
            writer.write(b"45")
        self.assertEqual(output.getvalue(), b"123")

    def test_second_object_failure_never_retires(self):
        original = self.provider.read
        def fail(ref, output):
            if ref.key.endswith(".manifest.json"):
                raise OSError("manifest incomplete")
            original(ref, output)
        with mock.patch.object(self.provider, "read", side_effect=fail):
            self.unchanged_on_refusal()
        self.assertEqual(self.record()["state"], "ARCHIVED_LOCAL")

    def test_archive_source_mutation_refuses(self):
        original = life.compact.write_archive
        def mutate(*args):
            records = original(*args)
            self.body.write_bytes(b"updated during archive")
            return records
        with mock.patch.object(life.compact, "write_archive", side_effect=mutate):
            with self.assertRaises(ValueError):
                self.run_job(retire=True)
        self.assertEqual(self.body.read_bytes(), b"updated during archive")
        self.assertEqual(self.record()["state"], "CLOSED_UNARCHIVED")

    def test_unsafe_state_permissions_and_symlinks_refuse(self):
        self.state.chmod(0o755)
        self.unchanged_on_refusal()
        self.state.chmod(0o700)
        alias = self.base / "state-link"
        alias.symlink_to(self.state, target_is_directory=True)
        self.controller.state_root = alias
        self.unchanged_on_refusal()

    def test_safe_member_and_job_names(self):
        for path in ("../escape", "/escape", "a//b", "a/../b", "a\\b", "a/./b", "a\x00b"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                life.safe_member(path)
        for job in ("../cache", ".", "..", "b" * 241, "/tmp"):
            with self.subTest(job=job), self.assertRaises(ValueError):
                self.controller.run(job, retire=True)

    def test_legacy_job_identity_uses_shared_preparation_validation(self):
        legacy = "123-1-test-native-" + self.job_id
        renamed = self.root.with_name(legacy)
        self.root.rename(renamed)
        self.root = renamed
        receipt = renamed / "receipts" / "UNARCHIVED.json"
        data = json.loads(receipt.read_bytes())
        data["job_id"] = legacy
        receipt.chmod(0o600)
        receipt.write_text(json.dumps(data))
        receipt.chmod(0o444)
        self.job_id = legacy
        self.assertEqual(self.run_job(retire=True), "RETIRED")

    def test_modified_source_or_extra_file_refuses_without_unlink(self):
        self.run_job()
        self.body.write_bytes(b"changed")
        self.unchanged_on_refusal()
        (self.root / "new-file").write_bytes(b"extra")
        self.unchanged_on_refusal()

    def test_local_archive_or_manifest_tampering_refuses(self):
        self.run_job()
        for suffix in (".tar.gz", ".manifest.json"):
            path = self.state / (self.job_id + suffix)
            original = path.read_bytes()
            path.write_bytes(original + b"extra")
            self.unchanged_on_refusal()
            path.write_bytes(original)

    def test_crash_after_closed_record_retries(self):
        with mock.patch.object(self.controller, "_archive", side_effect=OSError("crash")):
            self.unchanged_on_refusal()
        self.assertEqual(self.record()["state"], "CLOSED_UNARCHIVED")
        self.assertEqual(self.run_job(), "PRESERVED")

    def test_crash_before_archive_journal_commit_retries(self):
        original = self.controller._save
        def fail(directory, name, record):
            if record.get("state") == "ARCHIVED_LOCAL":
                raise OSError("crash")
            return original(directory, name, record)
        with mock.patch.object(self.controller, "_save", side_effect=fail):
            self.unchanged_on_refusal()
        self.assertEqual(self.record()["state"], "CLOSED_UNARCHIVED")
        self.assertEqual(self.run_job(retire=True), "RETIRED")

    def test_partial_retirement_recovery_rechecks_provider(self):
        self.run_job()
        original = life.os.unlink
        calls = 0
        def fail(path, *args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("crash between unlinks")
            return original(path, *args, **kwargs)
        with mock.patch.object(life.os, "unlink", side_effect=fail), self.assertRaises(OSError):
            self.run_job(retire=True)
        self.assertEqual(self.record()["state"], "RETIRING")
        self.provider.fault = "download"
        with self.assertRaises(OSError):
            self.run_job(retire=True)
        self.assertTrue(self.root.exists())
        self.provider.fault = None
        self.assertEqual(self.run_job(retire=True), "RETIRED")

    def test_partial_retirement_refuses_new_or_replaced_files(self):
        self.run_job()
        with mock.patch.object(life.os, "unlink", side_effect=OSError("crash")):
            with self.assertRaises(OSError):
                self.run_job(retire=True)
        extra = self.root / "new-file"
        extra.write_bytes(b"must remain")
        self.unchanged_on_refusal()
        self.assertEqual(extra.read_bytes(), b"must remain")
        extra.unlink()
        replacement = self.root / "replacement"
        replacement.write_bytes(self.body.read_bytes())
        os.replace(replacement, self.body)
        self.unchanged_on_refusal()

    def test_replaced_root_refuses_before_retirement(self):
        self.run_job()
        saved = self.root.with_name("saved")
        self.root.rename(saved)
        life.jobs.begin(self.home, self.env, nonce=self.job_id)
        with self.assertRaises(ValueError):
            self.run_job(retire=True)
        self.assertTrue((saved / "tmp" / "diagnostic.txt").exists())

    def test_crash_after_root_removal_before_journal_commit(self):
        original = self.controller._save
        def fail(directory, name, record):
            if record.get("state") == "RETIRED":
                raise OSError("crash")
            return original(directory, name, record)
        with mock.patch.object(self.controller, "_save", side_effect=fail), self.assertRaises(OSError):
            self.run_job(retire=True)
        self.assertFalse(self.root.exists())
        self.assertEqual(self.record()["state"], "RETIRING")
        self.assertEqual(self.run_job(retire=True), "RETIRED")

    def test_missing_root_before_retirement_refuses(self):
        self.run_job()
        self.root.rename(self.root.with_name("saved"))
        with self.assertRaises(ValueError):
            self.run_job(retire=True)

    def test_retired_root_recreation_refuses(self):
        self.run_job(retire=True)
        life.jobs.begin(self.home, self.env, nonce=self.job_id)
        with self.assertRaises(ValueError):
            self.run_job(retire=True)

    def test_member_readback_rejects_missing_duplicate_metadata_and_body_changes(self):
        self.run_job()
        records = self.record()["manifest"]["members"]
        source = self.state / (self.job_id + ".tar.gz")
        with tarfile.open(source, "r:gz") as archive:
            originals = [(member, archive.extractfile(member).read() if member.isfile() else None)
                         for member in archive]
        for fault in ("missing", "duplicate", "mode", "owner", "mtime", "body", "extra"):
            with self.subTest(fault=fault):
                output = io.BytesIO()
                with tarfile.open(fileobj=output, mode="w:gz") as archive:
                    for index, (member, data) in enumerate(originals):
                        member = copy.copy(member)
                        if fault == "missing" and index == 0:
                            continue
                        if index == 0:
                            if fault == "mode":
                                member.mode ^= 0o100
                            if fault == "owner":
                                member.uid += 1
                            if fault == "mtime":
                                member.mtime += 1
                                member.pax_headers = {}
                        if fault == "body" and data:
                            data = b"!" + data[1:]
                        archive.addfile(member, io.BytesIO(data) if data is not None else None)
                        if fault == "duplicate" and index == 0:
                            archive.addfile(member)
                    if fault == "extra":
                        archive.addfile(tarfile.TarInfo("unexpected"))
                output.seek(0)
                with self.assertRaises(ValueError):
                    life.compact.verify_archive(output, records)


if __name__ == "__main__":
    unittest.main()
