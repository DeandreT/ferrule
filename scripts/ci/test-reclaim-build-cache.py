#!/usr/bin/env python3
"""Exercise real cache paths, leases, redirected entries and live file users."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

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

    @staticmethod
    def mount_record(point):
        escaped = (str(point).replace("\\", r"\134").replace(" ", r"\040")
                   .replace("\t", r"\011").replace("\n", r"\012"))
        return f"42 1 0:1 / {escaped} rw shared:7 - ext4 device rw\n"

    def test_synthetic_mounts_refuse_before_either_target_is_trimmed(self):
        target = self.cache / "workspace-target"
        release = target / "release"
        release.mkdir()
        release_marker = release / "compiled-release"
        release_marker.write_bytes(b"reproducible release artifact")
        points = (target, target / "debug", self.markers[1].parent,
                  self.markers[1], release, release_marker)
        for point in points:
            with self.subTest(point=point):
                self.assertEqual(point.stat().st_dev, self.cache.stat().st_dev)
                text = self.mount_record(point)
                with self.assertRaisesRegex(ValueError, "Mounted cache tree"):
                    self.run_reclaim(available=self.capacity,
                                     mounts=lambda fd, **options: reclaim.reject_mounts(
                                         fd, mountinfo=text, **options))
                self.assertTrue(all(p.exists() for p in self.markers))
                self.assertEqual(release_marker.read_bytes(), b"reproducible release artifact")
                self.preserve()

    def test_escaped_mountpoint_refuses_before_deletion(self):
        point = self.markers[1].parent / "space tab\tline\nslash\\leaf"
        point.mkdir()
        text = self.mount_record(point)
        with self.assertRaisesRegex(ValueError, "Mounted cache tree"):
            self.run_reclaim(available=self.capacity,
                             mounts=lambda fd, **options: reclaim.reject_mounts(
                                 fd, mountinfo=text, **options))
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_sibling_mount_is_not_a_cache_descendant(self):
        sibling = self.cache / "generated-host-target-other" / "debug"
        sibling.mkdir(parents=True)
        marker = sibling / "original"
        marker.write_bytes(b"foreign sibling remains")
        text = self.mount_record(sibling)
        result = self.run_reclaim(available=self.capacity,
                                  mounts=lambda fd, **options: reclaim.reject_mounts(
                                      fd, mountinfo=text, **options))
        self.assertEqual(result["reclaimed_targets"], list(reclaim.TARGETS))
        self.assertEqual(marker.read_bytes(), b"foreign sibling remains")
        self.preserve()

    def test_malformed_mount_inventory_preserves_all_targets(self):
        invalid = ("", "\n", "incomplete\n", "42 1 0:1 / relative rw - ext4 device rw\n",
                   "42 1 0:1 / /unknown\\bad rw - ext4 device rw\n",
                   "42 1 0:1 / /other rw - ext4 device rw extra\n",
                   "42 1 0:1 /unknown\\bad /other rw - ext4 device rw\n",
                   "42 1 0:1 /unknown\\000 /other rw - ext4 device rw\n",
                   "42 1 0:1 / /other\\777 rw - ext4 device rw\n",
                   "42 1 0:1 /\x00 /other rw - ext4 device rw\n")
        for text in invalid + tuple(self.mount_record("/unrelated") + row
                                    for row in invalid if row):
            with self.subTest(inventory=text):
                with self.assertRaisesRegex(ValueError, "Mount inventory"):
                    self.run_reclaim(available=self.capacity,
                                     mounts=lambda fd, **options: reclaim.reject_mounts(
                                         fd, mountinfo=text, **options))
                self.assertTrue(all(p.exists() for p in self.markers))
                self.preserve()

    def test_unreadable_mount_inventory_preserves_all_targets(self):
        read_text = reclaim.Path.read_text

        def unavailable(path, *args, **options):
            if path == Path("/proc/self/mountinfo"):
                raise PermissionError("mount inventory")
            return read_text(path, *args, **options)

        with patch.object(reclaim.Path, "read_text", new=unavailable):
            with self.assertRaises(PermissionError):
                self.run_reclaim(available=self.capacity)
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_mount_after_process_census_refuses_before_any_trim(self):
        mounted = False

        def mount_admission(fd, **options):
            point = self.cache / "workspace-target" if mounted else Path("/unrelated")
            reclaim.reject_mounts(fd, mountinfo=self.mount_record(point), **options)

        def census(_roots):
            nonlocal mounted
            mounted = True

        with self.assertRaisesRegex(ValueError, "Mounted cache tree"):
            self.run_reclaim(available=self.capacity, active=census, mounts=mount_admission)
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_exact_cache_mount_refuses_before_creating_maintenance_lock(self):
        text = self.mount_record(self.cache)
        with self.assertRaisesRegex(ValueError, "Mounted cache tree"):
            self.run_reclaim(available=self.capacity,
                             mounts=lambda fd, **options: reclaim.reject_mounts(
                                 fd, mountinfo=text, **options))
        self.assertFalse((self.cache / ".reclaim-lock").exists())
        self.assertTrue(all(p.exists() for p in self.markers))
        self.preserve()

    def test_containing_home_mount_does_not_veto_normal_cache(self):
        text = self.mount_record(self.home)
        result = self.run_reclaim(available=self.capacity,
                                  mounts=lambda fd, **options: reclaim.reject_mounts(
                                      fd, mountinfo=text, **options))
        self.assertEqual(result["reclaimed_targets"], list(reclaim.TARGETS))
        self.preserve()

    def test_unavailable_descriptor_mount_identity_preserves_all_targets(self):
        read_text = reclaim.Path.read_text
        invalid = (None, "", "mnt_id: invalid\n", "mnt_id: 1\nmnt_id: 2\n")
        for text in invalid:
            with self.subTest(fdinfo=text):
                def unavailable(path, *args, **options):
                    if str(path).startswith("/proc/self/fdinfo/"):
                        if text is None:
                            raise PermissionError("descriptor mount identity")
                        return text
                    return read_text(path, *args, **options)

                with patch.object(reclaim.Path, "read_text", new=unavailable):
                    with self.assertRaises((PermissionError, ValueError)):
                        self.run_reclaim(available=self.capacity)
                self.assertFalse((self.cache / ".reclaim-lock").exists())
                self.assertTrue(all(p.exists() for p in self.markers))
                self.preserve()

    def test_late_descendant_mount_identity_refuses_before_traversal(self):
        original = {path: path.read_bytes() for path in self.markers}
        identity = reclaim.mount_identity
        admitted = False

        def census(_roots):
            nonlocal admitted
            admitted = True

        def mounted_child(fd):
            current = identity(fd)
            if admitted and os.readlink(f"/proc/self/fd/{fd}") == str(self.markers[0].parent):
                return current + 1
            return current

        with patch.object(reclaim, "mount_identity", new=mounted_child):
            with self.assertRaisesRegex(ValueError, "Mounted cache tree"):
                self.run_reclaim(available=self.capacity, active=census)
        self.assertEqual({path: path.read_bytes() for path in self.markers}, original)
        self.preserve()

    def test_opened_descendant_owner_change_refuses_before_any_trim(self):
        original = {path: path.read_bytes() for path in self.markers}
        fstat = reclaim.os.fstat

        def changed(fd):
            current = fstat(fd)
            if os.readlink(f"/proc/self/fd/{fd}") == str(self.markers[1].parent):
                fields = list(current)
                fields[4] = os.getuid() + 1
                return os.stat_result(fields)
            return current

        with patch.object(reclaim.os, "fstat", new=changed):
            with self.assertRaisesRegex(ValueError, "changed during admission"):
                self.run_reclaim(available=self.capacity)
        self.assertEqual({path: path.read_bytes() for path in self.markers}, original)
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
