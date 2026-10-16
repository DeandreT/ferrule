#!/usr/bin/env python3
"""Portable launcher refusal tests; no SDK, campaign, or native workload."""
import argparse
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest import mock


SOURCE = Path(__file__).with_name("unicode_controls.py")


def launcher(optimization=0):
    namespace = {"__file__": str(SOURCE), "__name__": "unicode_controls_test_subject"}
    exec(compile(SOURCE.read_text(encoding="utf-8"), str(SOURCE), "exec",
                 optimize=optimization), namespace)
    return SimpleNamespace(**namespace)


class UnicodeControlTests(unittest.TestCase):
    def test_optimized_execution_refuses_before_argument_parsing_or_side_effects(self):
        for optimization in (1, 2):
            with self.subTest(optimization=optimization):
                controls = launcher(optimization)
                with mock.patch.object(controls.argparse, "ArgumentParser") as parser, \
                     mock.patch.object(Path, "resolve") as resolve, \
                     mock.patch.object(Path, "mkdir") as mkdir, \
                     mock.patch.object(controls.signal, "signal") as signal, \
                     mock.patch.object(controls.importlib.util, "spec_from_file_location") as helper, \
                     mock.patch.object(controls.ctypes, "CDLL") as native:
                    with self.assertRaisesRegex(SystemExit, "optimized execution is not permitted"):
                        controls.main()
                    for operation in (parser, resolve, mkdir, signal, helper, native):
                        operation.assert_not_called()

    def test_fresh_home_child_explicitly_opts_out_without_mutating_caller(self):
        controls = launcher()
        root = Path("unused-evidence-root")
        for inherited_flag in (None, "false", "true"):
            inherited = {"PATH": "unchanged", "UNRELATED": "retained"}
            if inherited_flag is not None:
                inherited["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"] = inherited_flag
            with mock.patch.dict(os.environ, inherited, clear=True):
                env = controls.control_environment(root)
                self.assertEqual(dict(os.environ), inherited)
            self.assertEqual(env["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"], "false")
        self.assertEqual(env["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"], "false")
        self.assertEqual(env["DOTNET_CLI_HOME"], str(root / ".dotnet-home"))
        self.assertEqual(env["NUGET_PACKAGES"], str(root / ".nuget"))
        self.assertEqual(env["TMPDIR"], str(root / ".tmp"))
        self.assertEqual(env["PATH"], "unchanged")
        self.assertEqual(env["UNRELATED"], "retained")

    def test_overrides_are_refused_even_when_assertions_are_removed(self):
        for optimization in (0, 1, 2):
            controls = launcher(optimization)
            for key in ("COMPlus_GCHeapHardLimit", "COMPLUS_GCHEAPHARDLIMIT", "complus_gcheapHardLimit",
                        "CORECLR_ENABLE_PROFILING", "CORECLR_PROFILER", "DOTNET_STARTUP_HOOKS"):
                with self.subTest(optimization=optimization, key=key), \
                     mock.patch.dict(os.environ, {key: "false"}, clear=True):
                    with self.assertRaisesRegex(ValueError, "(?i)" + key):
                        controls.control_environment(Path("unused-evidence-root"))

    def test_override_refusal_precedes_input_resolution_and_evidence_creation(self):
        controls = launcher()
        args = argparse.Namespace(**{name: Path("unused-" + name) for name in
            ("repo", "baseline_library", "candidate_library", "prerequisite", "root", "dotnet")})
        with mock.patch.dict(os.environ, {"CORECLR_ENABLE_PROFILING": "1"}, clear=True), \
             mock.patch.object(controls.argparse, "ArgumentParser") as parser, \
             mock.patch.object(Path, "resolve") as resolve, \
             mock.patch.object(Path, "mkdir") as mkdir, \
             mock.patch.object(controls.signal, "signal") as signal, \
             mock.patch.object(controls.importlib.util, "spec_from_file_location") as helper, \
             mock.patch.object(controls.ctypes, "CDLL") as native:
            parser.return_value.parse_args.return_value = args
            with self.assertRaisesRegex(ValueError, "CORECLR_ENABLE_PROFILING"):
                controls.main()
            for operation in (resolve, mkdir, signal, helper, native):
                operation.assert_not_called()


if __name__ == "__main__":
    unittest.main()
