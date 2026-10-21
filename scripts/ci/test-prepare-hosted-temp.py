"""Native unit checks; filesystem probes are injected, never a quota bypass."""
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("hosted_temp", Path(__file__).with_name("prepare-hosted-temp.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class HostedTemporaryDirectoryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.environment_file = self.root / "github-env"
        self.environment_file.write_text("EXISTING=retained\n", encoding="utf8")
        self.environment = {"RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_TEMP": str(self.root),
                            "GITHUB_ENV": str(self.environment_file)}

    def prepare(self, *, filesystem="ext2/ext3", free=MODULE.MIN_FREE_BYTES):
        return MODULE.prepare(self.environment, filesystem=lambda _: filesystem,
                              usage=lambda _: SimpleNamespace(free=free))

    def assert_refused(self, **kwargs):
        before = self.environment_file.read_bytes()
        with self.assertRaises(ValueError):
            self.prepare(**kwargs)
        self.assertEqual(self.environment_file.read_bytes(), before)

    def test_disk_backed_private_directory_and_all_temp_variables(self):
        result = self.prepare()
        directory = Path(result["directory"])
        self.assertEqual(directory.parent, self.root)
        self.assertTrue(directory.is_dir())
        self.assertEqual(self.environment_file.read_text(encoding="utf8"),
                         "EXISTING=retained\n" + "".join(f"{name}={directory}\n" for name in ("TMPDIR", "TMP", "TEMP")))
        self.assertEqual(result["minimum_free_bytes"], 8 * 1024**3)

    def test_ram_backed_and_unknown_filesystems_are_refused(self):
        for kind in ("tmpfs", "ramfs", "unknown"):
            with self.subTest(kind=kind):
                self.assert_refused(filesystem=kind)

    def test_existing_reserve_is_not_lowered(self):
        self.assert_refused(free=MODULE.MIN_FREE_BYTES - 1)

    def test_self_hosted_setup_is_not_changed(self):
        self.environment["RUNNER_ENVIRONMENT"] = "self-hosted"
        self.assert_refused()

    def test_missing_relative_and_multiline_paths_are_refused(self):
        for value in ("", "relative", str(self.root) + "\nINJECTED=yes"):
            with self.subTest(value=value):
                self.environment["RUNNER_TEMP"] = value
                self.assert_refused()

    def test_missing_environment_file_is_not_created(self):
        self.environment_file.unlink()
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.environment_file.exists())

    def test_linked_environment_file_is_refused(self):
        import os
        link = self.root / "linked-env"
        os.link(self.environment_file, link)
        self.assert_refused()

    def test_symlinked_environment_file_is_refused_before_open(self):
        output = self.environment_file
        with patch.object(Path, "is_symlink", lambda path: path == output):
            self.assert_refused()

    def test_repeated_setup_uses_distinct_owned_directories(self):
        first = Path(self.prepare()["directory"])
        second = Path(self.prepare()["directory"])
        self.assertNotEqual(first, second)
        self.assertTrue(first.is_dir())


if __name__ == "__main__":
    unittest.main()
