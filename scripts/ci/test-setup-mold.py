"""No network or installed linker is needed for these job-local setup tests."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("setup_mold", Path(__file__).with_name("setup-mold.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def archive(*, duplicate=False, symlink=False, omit_license=False):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as output:
        for name in module.MEMBERS:
            if omit_license and name.endswith("LICENSE"):
                continue
            item = tarfile.TarInfo(f"mold-{module.VERSION}-x86_64-linux/{name}")
            payload = b"test content"
            if symlink and name == "bin/mold":
                item.type = tarfile.SYMTYPE
                item.linkname = "/bin/sh"
                output.addfile(item)
            else:
                item.size = len(payload)
                output.addfile(item, io.BytesIO(payload))
                if duplicate and name == "bin/mold":
                    output.addfile(item, io.BytesIO(payload))
        # Unselected archive paths are never extracted.
        item = tarfile.TarInfo("../../outside")
        item.size = 1
        output.addfile(item, io.BytesIO(b"x"))
    return buffer.getvalue()


@unittest.skipUnless(os.name == "posix", "Linux setup uses a local symlink")
class SetupMoldTests(unittest.TestCase):
    def test_installs_only_selected_files_and_registers_path_after_validation(self):
        data = archive()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "job"
            root.mkdir()
            with patch.object(module.subprocess, "check_output", return_value=f"mold {module.VERSION} (compatible with GNU ld)\n"):
                result = module.install(root, root / "path", data, hashlib.sha256(data).hexdigest())
            self.assertEqual((result / "bin/ld.mold").readlink(), Path("mold"))
            self.assertTrue((result / "share/doc/mold/LICENSE").is_file())
            self.assertEqual((root / "path").read_text().strip(), str(result / "bin"))
            self.assertFalse((Path(directory) / "outside").exists())

    def test_rejects_wrong_checksum_without_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                module.install(root, root / "path", archive(), "0" * 64)
            self.assertEqual(list(root.iterdir()), [])

    def test_rejects_missing_duplicate_and_link_members(self):
        for options in [{"duplicate": True}, {"symlink": True}, {"omit_license": True}]:
            with self.subTest(options=options), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                data = archive(**options)
                with self.assertRaises(ValueError):
                    module.install(root, root / "path", data, hashlib.sha256(data).hexdigest())
                self.assertEqual(list(root.iterdir()), [])

    def test_failed_executable_validation_does_not_publish_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = archive()
            with patch.object(module.subprocess, "check_output", return_value="wrong linker\n"):
                with self.assertRaises(ValueError):
                    module.install(root, root / "path", data, hashlib.sha256(data).hexdigest())
            self.assertEqual(list(root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
