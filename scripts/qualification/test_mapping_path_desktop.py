"""Standard-library tests; no X server or user desktop is accessed."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "mapping_path_desktop", Path(__file__).with_name("mapping-path-desktop.py")
)
desktop = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(desktop)


class GuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="desktop108-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.run = self.root / "run"
        self.run.mkdir()
        self.proc = self.root / "proc"
        self.process = self.proc / "123"
        (self.process / "fd").mkdir(parents=True)
        boot = self.proc / "sys/kernel/random/boot_id"
        boot.parent.mkdir(parents=True)
        boot.write_text("test-boot\n")
        self.executable = self.root / "Xvfb"
        self.executable.write_bytes(b"fake executable identity, never executed")
        (self.process / "exe").symlink_to(self.executable)
        self.start(100)
        self.sockets = self.root / "x"
        self.sockets.mkdir()
        self.listener = socket.socket(socket.AF_UNIX)
        self.addCleanup(self.listener.close)
        self.socket_path = self.sockets / "X19"
        self.listener.bind(str(self.socket_path))
        (self.proc / "net").mkdir()
        (self.proc / "net/unix").write_text(
            "Num RefCount Protocol Flags Type St Inode Path\n"
            f"0: 00000002 00000000 00010000 0001 01 901 {self.socket_path}\n"
        )
        (self.process / "fd/7").symlink_to("socket:[901]")
        self.owner = {
            "xvfb": 123, "display": ":19", "session": 124,
            "xvfb_identity": desktop.process_identity(123, self.proc),
            "display_binding": desktop.display_binding(123, ":19", self.proc, self.sockets),
        }
        (self.run / "owner.json").write_text(json.dumps(self.owner))

    def start(self, ticks):
        fields = ["S"] + ["0"] * 49
        fields[19] = str(ticks)
        (self.process / "stat").write_text("123 (Xvfb test) " + " ".join(fields))

    def validate(self, owner=None):
        desktop.validate_owner(owner or self.owner, self.proc, self.sockets)

    def action(self, arguments):
        validate = desktop.validate_owner
        with mock.patch.object(
            desktop, "validate_owner",
            side_effect=lambda owner: validate(owner, self.proc, self.sockets),
        ):
            desktop.action(self.run, arguments)

    def test_valid_process_and_socket_identity(self):
        self.validate()

    def test_stale_and_reused_pid_rejected(self):
        self.start(200)  # Same PID, executable and socket, different process birth.
        with self.assertRaisesRegex(RuntimeError, "identity changed"):
            self.validate()
        (self.process / "stat").unlink()
        with self.assertRaises(RuntimeError):
            self.validate()

    def test_boot_executable_and_socket_reuse_rejected(self):
        cases = (
            ("xvfb_identity", "boot_id", "previous-boot"),
            ("xvfb_identity", "executable_inode", -1),
            ("display_binding", "inode", -1),
            ("display_binding", "listener", "902"),
        )
        for section, field, value in cases:
            with self.subTest(field=field):
                owner = json.loads(json.dumps(self.owner))
                owner[section][field] = value
                with self.assertRaises(RuntimeError):
                    self.validate(owner)

    def test_process_must_be_xvfb_and_own_local_display(self):
        other = self.root / "other"
        other.write_bytes(b"not Xvfb")
        (self.process / "exe").unlink()
        (self.process / "exe").symlink_to(other)
        owner = dict(self.owner, xvfb_identity=desktop.process_identity(123, self.proc))
        with self.assertRaises(RuntimeError):
            self.validate(owner)
        (self.process / "exe").unlink()
        (self.process / "exe").symlink_to(self.executable)
        (self.process / "fd/7").unlink()
        with self.assertRaisesRegex(RuntimeError, "does not own"):
            self.validate()
        for display in (":19.0", "localhost:19", ":../19"):
            with self.subTest(display=display), self.assertRaises(RuntimeError):
                self.validate(dict(self.owner, display=display))

    def test_legacy_owner_is_rejected_without_rewriting_evidence(self):
        path = self.run / "owner.json"
        path.write_text('{"xvfb":123,"display":":19"}')
        original = path.read_bytes()
        with self.assertRaises(RuntimeError):
            self.action(["key", "a"])
        self.assertEqual(path.read_bytes(), original)

    def test_every_action_rejects_reused_pid_before_dispatch(self):
        self.start(200)
        with mock.patch.object(desktop.subprocess, "run") as run, \
                mock.patch.object(desktop.subprocess, "check_output") as output, \
                mock.patch.object(desktop, "close_window") as close:
            for arguments in (["key", "a"], ["capture", "shot"],
                              ["clipboard", "out.json"], ["snapshot", "saved-copy"],
                              ["close", "123"]):
                with self.subTest(arguments=arguments), self.assertRaises(RuntimeError):
                    self.action(arguments)
            run.assert_not_called()
            output.assert_not_called()
            close.assert_not_called()
        self.assertFalse((self.run / "actions.jsonl").exists())

    def test_invalid_names_rejected_for_each_artifact_action(self):
        invalid = ("", ".", "..", "../escape", "/tmp/escape", "sub/file",
                   "C:\\escape", "sub\\file", " trailing ", "null\0name")
        with mock.patch.object(desktop.subprocess, "run") as run, \
                mock.patch.object(desktop.subprocess, "check_output") as output:
            for operation in ("snapshot", "capture", "clipboard"):
                for name in invalid:
                    with self.subTest(operation=operation, name=name):
                        with self.assertRaises(ValueError):
                            self.action([operation, name])
            run.assert_not_called()
            output.assert_not_called()

    def test_existing_and_dangling_symlinks_are_rejected(self):
        outside = self.root / "outside"
        inside = self.run / "inside"
        outside.write_bytes(b"outside unchanged")
        inside.write_bytes(b"inside unchanged")
        for operation, suffix in (("snapshot", ".png"), ("capture", ".png"),
                                  ("clipboard", "")):
            for target in (outside, inside, self.root / "missing"):
                link = self.run / ("escape" + suffix)
                link.symlink_to(target)
                with self.subTest(operation=operation, target=target):
                    with self.assertRaises(ValueError):
                        self.action([operation, "escape"])
                link.unlink()
        self.assertEqual(outside.read_bytes(), b"outside unchanged")
        self.assertEqual(inside.read_bytes(), b"inside unchanged")

    def test_snapshot_preflights_all_destinations(self):
        (self.run / "shot.json").write_bytes(b"preserve existing snapshot")
        (self.run / "shot.layout.json").symlink_to(self.root / "outside")
        with self.assertRaises(ValueError):
            self.action(["snapshot", "shot"])
        self.assertEqual((self.run / "shot.json").read_bytes(), b"preserve existing snapshot")

    def test_no_follow_open_refuses_a_late_symlink(self):
        path = desktop.destination(self.run, "late")
        outside = self.root / "outside"
        outside.write_bytes(b"unchanged")
        path.symlink_to(outside)
        with self.assertRaises(OSError):
            desktop.output_file(path)
        self.assertEqual(outside.read_bytes(), b"unchanged")

    def test_snapshot_saved_preserves_both_originals(self):
        originals = {"saved.json": b"original project", "saved.layout.json": b"original layout"}
        for name, data in originals.items():
            (self.run / name).write_bytes(data)
        with mock.patch.object(desktop.subprocess, "run") as run:
            with self.assertRaises(shutil.SameFileError):
                self.action(["snapshot", "saved"])
            run.assert_not_called()
        for name, data in originals.items():
            self.assertEqual((self.run / name).read_bytes(), data)
        self.assertFalse((self.run / "saved.png").exists())

    def test_snapshot_hardlinks_preserve_both_originals_and_earlier_outputs(self):
        originals = {"saved.json": b"original project", "saved.layout.json": b"original layout"}
        for name, data in originals.items():
            (self.run / name).write_bytes(data)
        # Include a late layout alias: JSON must not have been overwritten first.
        for output, source in (("shot.json", "saved.json"),
                               ("shot.layout.json", "saved.layout.json"),
                               ("shot.json", "saved.layout.json"),
                               ("shot.layout.json", "saved.json"),
                               ("shot.png", "saved.json"),
                               ("shot.png", "saved.layout.json")):
            with self.subTest(output=output, source=source):
                outputs = {name: b"previous snapshot " + name.encode()
                           for name in ("shot.json", "shot.layout.json", "shot.png")}
                for name, data in outputs.items():
                    (self.run / name).write_bytes(data)
                (self.run / output).unlink()
                os.link(self.run / source, self.run / output)
                outputs[output] = originals[source]
                with mock.patch.object(desktop.subprocess, "run") as run:
                    with self.assertRaises(shutil.SameFileError):
                        self.action(["snapshot", "shot"])
                    run.assert_not_called()
                for name, data in originals.items():
                    self.assertEqual((self.run / name).read_bytes(), data)
                for name, data in outputs.items():
                    self.assertEqual((self.run / name).read_bytes(), data)
                    (self.run / name).unlink()

    def test_valid_private_paths_and_artifact_actions(self):
        def screenshot(*_args, **kwargs):
            kwargs["stdout"].write(b"png screenshot")

        (self.run / "saved.json").write_bytes(b"project")
        (self.run / "saved.layout.json").write_bytes(b"layout")
        with mock.patch.object(desktop.subprocess, "run", side_effect=screenshot), \
                mock.patch.object(desktop.subprocess, "check_output", return_value=b"output"):
            self.action(["snapshot", "history-main-before"])
            self.action(["capture", "screen"])
            self.action(["clipboard", "main-preview.json"])
        for name, content in (("history-main-before.json", b"project"),
                              ("history-main-before.layout.json", b"layout"),
                              ("history-main-before.png", b"png screenshot"),
                              ("screen.png", b"png screenshot"),
                              ("main-preview.json", b"output")):
            self.assertEqual((self.run / name).read_bytes(), content)
        self.assertEqual(desktop.destination(self.run, "main-preview.json"),
                         self.run / "main-preview.json")
        self.assertEqual((self.run / "saved.json").read_bytes(), b"project")
        self.assertEqual((self.run / "saved.layout.json").read_bytes(), b"layout")
        self.assertEqual(len((self.run / "actions.jsonl").read_text().splitlines()), 3)


if __name__ == "__main__":
    unittest.main()
