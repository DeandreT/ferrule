#!/usr/bin/env python3
"""Exercise real cache paths, leases, redirected entries and live file users."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "reclaim", Path(__file__).with_name("reclaim-build-cache.py"))
reclaim = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reclaim)


class CacheSpaceTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="ferrule-cache-space-")
        self.addCleanup(self.scratch.cleanup)
        self.home = Path(self.scratch.name)
        self.cache = self.home / "ci-cache"
        self.cache.mkdir()
        self.markers = []
        for name in reclaim.TARGETS:
            profile = self.cache / name / "debug"
            profile.mkdir(parents=True)
            marker = profile / "deps" / "compiled-artifact"
            marker.parent.mkdir()
            marker.write_bytes(b"reproducible build artifact")
            self.markers.append(marker)
        self.evidence = self.home / "ci-tmp" / "failed-job" / "original.log"
        self.evidence.parent.mkdir(parents=True)
        self.evidence.write_bytes(b"complete retained failure")
        self.compiler = self.cache / "compiler" / "cached-object"
        self.compiler.parent.mkdir()
        self.compiler.write_bytes(b"bounded compiler cache")

    def run_reclaim(self, reserve=16, **options):
        # The finite idle model excludes unrelated protected desktop processes;
        # the live-user test below observes a real child through production /proc reads.
        options.setdefault("active", lambda roots: reclaim.live_cache_users(roots, pids=[]))
        return reclaim.reclaim(self.home, reserve, **options)

    def capacity(self, _home):
        # Each disposable target restores ten GiB in this finite capacity model.
        return (4 + 10 * sum(not p.exists() for p in self.markers)) * reclaim.GIB

    def preserve(self):
        self.assertEqual(self.evidence.read_bytes(), b"complete retained failure")
        self.assertEqual(self.compiler.read_bytes(), b"bounded compiler cache")

    def test_sufficient_headroom_keeps_all_cache_bytes(self):
        result = self.run_reclaim(available=lambda _: 20 * reclaim.GIB)
        self.assertEqual(result["reclaimed_targets"], [])
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_pressure_reclaims_targets_in_order_and_preserves_evidence(self):
        result = self.run_reclaim(available=self.capacity)
        self.assertEqual(result["reclaimed_targets"], list(reclaim.TARGETS))
        self.assertEqual(result["after_bytes"], 24 * reclaim.GIB)
        for marker in self.markers:
            self.assertFalse(marker.exists())
            self.assertTrue((marker.parent.parent / ".cargo-lock").is_file())
        self.preserve()

    def test_codegen_stops_as_soon_as_headroom_is_restored(self):
        result = self.run_reclaim(8, available=self.capacity)
        self.assertEqual(result["reclaimed_targets"], ["generated-host-target"])
        self.assertFalse(self.markers[0].exists())
        self.assertTrue(self.markers[1].exists())
        self.preserve()

    def test_insufficient_space_refuses_without_retiring_evidence(self):
        with self.assertRaisesRegex(ValueError, "Insufficient space"):
            self.run_reclaim(available=lambda _: 4 * reclaim.GIB)
        self.preserve()

    def test_redirected_target_refuses_before_any_target_is_trimmed(self):
        root = self.cache / "workspace-target"
        parked = self.home / "parked-target"
        root.rename(parked)
        root.symlink_to(parked, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "redirected"):
            self.run_reclaim(available=self.capacity)
        self.assertTrue(self.markers[0].exists())
        self.assertEqual((parked / "debug/deps/compiled-artifact").read_bytes(),
                         b"reproducible build artifact")
        self.preserve()

    def test_symlink_inside_disposable_profile_never_deletes_its_target(self):
        (self.markers[0].parent / "outside").symlink_to(self.evidence.parent,
                                                       target_is_directory=True)
        self.run_reclaim(available=self.capacity)
        self.preserve()

    def test_unrecognized_target_entries_are_preserved(self):
        retained = self.cache / "workspace-target/private-evidence/original"
        retained.parent.mkdir()
        retained.write_bytes(b"retained target-root evidence")
        self.run_reclaim(available=self.capacity)
        self.assertEqual(retained.read_bytes(), b"retained target-root evidence")
        self.preserve()

    def test_special_file_refuses_before_either_target_is_trimmed(self):
        os.mkfifo(self.markers[1].parent / "unexpected-fifo")
        with self.assertRaisesRegex(ValueError, "Special cache file"):
            self.run_reclaim(available=self.capacity)
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def held_process(self, code, argument):
        child = subprocess.Popen([sys.executable, "-c", code, str(argument)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 text=True)
        self.addCleanup(lambda: child.poll() is None and child.kill())
        self.assertEqual(child.stdout.readline(), "ready\n")
        return child

    def finish(self, child):
        child.communicate("release\n", timeout=5)
        self.assertEqual(child.returncode, 0)

    def test_live_cargo_lease_refuses_both_targets(self):
        lock = self.markers[1].parent.parent / ".cargo-lock"
        child = self.held_process(
            "import fcntl,sys; f=open(sys.argv[1],'a'); "
            "fcntl.flock(f,fcntl.LOCK_EX); print('ready',flush=True); input()", lock)
        try:
            with self.assertRaises(BlockingIOError):
                self.run_reclaim(available=self.capacity)
            self.assertTrue(all(p.exists() for p in self.markers))
        finally:
            self.finish(child)
        self.preserve()

    def test_live_host_file_refuses_cleanup_even_without_cargo_lease(self):
        child = self.held_process(
            "import sys; f=open(sys.argv[1],'rb'); print('ready',flush=True); input()",
            self.markers[0])
        try:
            with self.assertRaisesRegex(ValueError, "live users"):
                self.run_reclaim(available=self.capacity,
                                 active=lambda roots: reclaim.live_cache_users(roots,
                                                                              pids=[child.pid]))
            self.assertTrue(all(p.exists() for p in self.markers))
        finally:
            self.finish(child)
        self.preserve()

    def test_unreadable_live_user_refuses_before_deletion(self):
        def unreadable(roots):
            raise PermissionError("live process cannot be inspected")
        with self.assertRaises(PermissionError):
            self.run_reclaim(available=self.capacity, active=unreadable)
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_wrong_reserve_and_noncanonical_home_refuse_without_mutation(self):
        with self.assertRaisesRegex(ValueError, "Unknown storage reserve"):
            self.run_reclaim(4, available=self.capacity)
        redirected = self.home / "redirected-home"
        redirected.symlink_to(self.home, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "canonical"):
            reclaim.reclaim(redirected, 16, available=self.capacity)
        self.assertTrue(all(p.exists() for p in self.markers))


if __name__ == "__main__":
    unittest.main()
