"""Portable entry/environment checks; no campaign, worker, or .NET launch."""
import ast
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def environment_builder():
    # campaign.py requires Linux modules at import time. Compile only its pure
    # environment function, without importing or running the launcher.
    tree = ast.parse((HERE / "campaign.py").read_text(encoding="utf-8"))
    function, = [node for node in tree.body
                 if isinstance(node, ast.FunctionDef) and node.name == "child_environment"]
    namespace = {}
    exec(compile(ast.Module(body=[function], type_ignores=[]), "campaign.py", "exec"), namespace)
    return namespace["child_environment"]


class VerificationEntry(unittest.TestCase):
    def test_optimization_refuses_before_evidence_read_or_audit_mkdir(self):
        for flags, optimization in ((["-O"], None), (["-OO"], None),
                                    ([], "1"), ([], "2")):
            with self.subTest(flags=flags, optimization=optimization), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                evidence = root / "absent-evidence"
                audit = root / "audit"
                env = os.environ.copy()
                env.pop("PYTHONOPTIMIZE", None)
                if optimization is not None:
                    env["PYTHONOPTIMIZE"] = optimization
                result = subprocess.run([sys.executable, *flags, str(HERE / "verify.py"),
                                         str(evidence), str(audit)], env=env, capture_output=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"Python optimization is not supported", result.stderr)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(list(root.iterdir()), [])

    def test_unoptimized_entry_still_rejects_failed_campaign(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            evidence = root / "evidence"
            evidence.mkdir()
            (evidence / "CAMPAIGN-RESULT.json").write_text('{"failure": "test refusal"}\n', encoding="utf-8")
            audit = root / "audit"
            env = os.environ.copy()
            env.pop("PYTHONOPTIMIZE", None)
            result = subprocess.run([sys.executable, str(HERE / "verify.py"), str(evidence), str(audit)],
                                    env=env, capture_output=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"AssertionError", result.stderr)
            self.assertNotIn(b"Python optimization is not supported", result.stderr)
            self.assertEqual(result.stdout, b"")
            self.assertEqual(list(audit.iterdir()), [])


class ChildEnvironment(unittest.TestCase):
    def test_fresh_home_always_opts_out_without_changing_parent_or_path(self):
        build = environment_builder()
        root, scratch, target = Path("evidence"), Path("scratch"), Path("host-target")
        for inherited_flag in (None, "false", "true"):
            with self.subTest(inherited_flag=inherited_flag):
                inherited = {"PATH": "existing-user-global-tools-and-other-tools", "OTHER": "retained"}
                if inherited_flag is not None:
                    inherited["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"] = inherited_flag
                before = inherited.copy()
                child = build(inherited, root, scratch, target)
                self.assertEqual(inherited, before)
                self.assertEqual(child["PATH"], inherited["PATH"])
                self.assertEqual(child["OTHER"], "retained")
                self.assertEqual(child["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"], "false")
                self.assertEqual(child["DOTNET_CLI_HOME"], str(root / ".dotnet-home"))
                self.assertEqual(child["NUGET_PACKAGES"], str(root / ".nuget"))
                self.assertEqual(child["TMPDIR"], str(scratch))
                self.assertEqual(child["FERRULE_CODEGEN_HOST_TARGET_DIR"], str(target))

    def test_measurement_overrides_remain_forbidden(self):
        build = environment_builder()
        for key in ("COMPlus_gcServer", "COMPLUS_GCSERVER", "complus_gcserver", "CORECLR_ENABLE_PROFILING", "DOTNET_GCHeapHardLimit",
                    "DOTNET_DiagnosticPorts", "DOTNET_CLI_HOME"):
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, "remove inherited CLR overrides"):
                build({key: "override", "DOTNET_ADD_GLOBAL_TOOLS_TO_PATH": "false"},
                      Path("evidence"), Path("scratch"), Path("host-target"))

    def test_campaign_passes_constructed_environment_to_children(self):
        tree = ast.parse((HERE / "campaign.py").read_text(encoding="utf-8"))
        main, = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "main"]
        assignments = [node for node in ast.walk(main) if isinstance(node, ast.Assign)
                       and any(isinstance(target, ast.Name) and target.id == "env" for target in node.targets)]
        self.assertEqual(len(assignments), 1)
        self.assertEqual(ast.unparse(assignments[0].value),
                         "child_environment(os.environ, args.root, short_tmp, args.host_target)")
        constructors = [node for node in ast.walk(main) if isinstance(node, ast.Call)
                        and isinstance(node.func, ast.Name) and node.func.id == "Campaign"]
        self.assertEqual(len(constructors), 1)
        self.assertEqual(ast.unparse(constructors[0]), "Campaign(args.root, env)")
        campaign, = [node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "Campaign"]
        launches = [node for node in ast.walk(campaign) if isinstance(node, ast.Call)
                    and isinstance(node.func, ast.Attribute) and node.func.attr == "Popen"]
        self.assertEqual(len(launches), 1)
        self.assertEqual([ast.unparse(keyword.value) for keyword in launches[0].keywords if keyword.arg == "env"],
                         ["self.env"])


if __name__ == "__main__":
    unittest.main()
