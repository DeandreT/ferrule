"""Check ignored compiled-test selection and the scheduled workflow without building."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import tempfile
import unittest
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location(
    "ignored_codegen_tests", Path(__file__).with_name("ignored-codegen-tests.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
ROOT = Path(__file__).resolve().parents[2]


def known():
    names = set(module.EXCLUDED) | set(module.OPT_IN) | set(module.RETAINED)
    return names | {name for group in module.PREPARED.values() for name in group}


def listing(names, *, unignored=(), suite=module.SUITE, status="matches"):
    cases = {name: {"ignored": True, "filter-match": {"status": status}} for name in names}
    cases.update({name: {"ignored": False, "filter-match": {"status": "mismatch"}} for name in unignored})
    return {"rust-suites": {suite: {"testcases": cases}}}


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.tests = sorted(known() | {f"plain_{index}::compiled" for index in range(20)})

    def test_listing_keeps_only_ignored_tests_of_the_one_suite(self):
        parsed = module.ignored_tests(listing(["b::x", "a::y"], unignored=["c::z"]))
        self.assertEqual(parsed, ["a::y", "b::x"])
        for bad in (listing(["a::y"], suite="cli::other"), listing([]),
                    listing(["a::y"], status="mismatch"), {"rust-suites": {}}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                module.ignored_tests(bad)
        two = listing(["a::y"])
        two["rust-suites"]["cli::other"] = {"testcases": {}}
        with self.assertRaises(ValueError):
            module.ignored_tests(two)

    def test_every_listed_test_is_selected_once_or_excluded(self):
        self.assertEqual(module.problems(self.tests), [])
        selected = [name for shard in module.shards(self.tests) for unit in shard for name in unit]
        self.assertEqual(sorted(selected), sorted(set(self.tests) - set(module.EXCLUDED)))
        sizes = [len(shard) for shard in module.shards(self.tests)]
        self.assertLessEqual(max(sizes) - min(sizes), 1)

    def test_new_ignored_test_is_selected_without_a_table_entry(self):
        tests = sorted(self.tests + ["brand_new::compiled_parity"])
        self.assertEqual(module.problems(tests), [])
        self.assertIn(("brand_new::compiled_parity",), [u for s in module.shards(tests) for u in s])

    def test_stale_and_incomplete_entries_fail(self):
        removed = next(iter(module.OPT_IN))
        self.assertIn(f"stale entry names no ignored test: {removed}",
                      module.problems([name for name in self.tests if name != removed]))
        variable, group = next((v, g) for v, g in module.PREPARED.items() if len(g) == 2)
        found = module.problems([name for name in self.tests if name != group[0]])
        self.assertTrue(any(problem.startswith(f"incomplete prepared group {variable}") for problem in found))

    def test_dropped_selection_fails(self):
        original = module.shards
        try:
            module.shards = lambda tests, count=module.SHARDS: [shard[1:] for shard in original(tests, count)]
            self.assertTrue(any("not selected by any shard" in problem
                                for problem in module.problems(self.tests)))
        finally:
            module.shards = original

    def test_prepared_groups_stay_ordered_in_one_shard(self):
        units = [unit for shard in module.shards(self.tests) for unit in shard]
        for group in module.PREPARED.values():
            self.assertIn(tuple(group), units)


class RunTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        (self.root / "host").mkdir()
        (self.root / "evidence").mkdir()
        self.base = {"PATH": "/usr/bin", "FERRULE_CODEGEN_HOST_TARGET_DIR": str(self.root / "host")}
        self.tests = sorted(known())
        self.calls = []
        self.warmups = []
        output = contextlib.ExitStack()
        output.enter_context(contextlib.redirect_stdout(io.StringIO()))
        output.enter_context(contextlib.redirect_stderr(io.StringIO()))
        self.addCleanup(output.close)

    def tearDown(self):
        self.directory.cleanup()

    def runner(self, failing=()):
        def run(command, *, stdout, stderr, env):
            if command == ["dotnet", "--help"]:
                self.warmups.append(env)
                failed = "dotnet-warmup" in failing
                stdout.write(b"first-use failed\n" if failed else b"dotnet help\n")
                return SimpleNamespace(returncode=1 if failed else 0)
            name = command[-1][len("test(="):-1]
            # A run_test-only unit check has no shard warm-up. Every full run
            # must finish its single warm-up before starting nextest.
            if self.warmups:
                self.assertEqual(env["XDG_RUNTIME_DIR"], self.warmups[-1]["XDG_RUNTIME_DIR"])
            self.calls.append((name, env))
            failed = name in failing
            stderr.write(b"     Summary 1 test run: " + (b"0 passed, 1 failed" if failed else b"1 passed") + b"\n")
            return SimpleNamespace(returncode=1 if failed else 0)
        return run

    def shard_of(self, name):
        return next(index for index, shard in enumerate(module.shards(self.tests), 1)
                    if any(name in unit for unit in shard))

    def test_runs_each_test_with_its_own_logs_and_settings(self):
        evidence = self.root / "evidence"
        for shard in range(1, module.SHARDS + 1):
            self.assertEqual(module.run(self.tests, shard, evidence, self.base, runner=self.runner(),
                                        space=lambda path: module.RESERVE_BYTES), 0)
        ran = [name for name, _ in self.calls]
        self.assertEqual(len(self.warmups), module.SHARDS)
        self.assertEqual(sorted(ran), sorted(set(self.tests) - set(module.EXCLUDED)))
        for name, env in self.calls:
            stem = evidence / module.label(name)
            for leaf in ("command.txt", "environment.json", "stdout.log", "stderr.log", "exit-status.txt"):
                self.assertTrue((stem / leaf).is_file(), (name, leaf))
            self.assertIn(f"test(={name})", (stem / "command.txt").read_text())
            for key, value in module.OPT_IN.get(name, {}).items():
                self.assertEqual(env[key], value)
            runtime = Path(env["XDG_RUNTIME_DIR"])
            self.assertEqual(runtime, evidence / ".runtime")
            self.assertTrue(runtime.is_dir())
            if os.name == "posix":
                self.assertEqual(runtime.stat().st_mode & 0o777, 0o700)
        prepared = dict(self.calls)
        for variable, group in module.PREPARED.items():
            paths = {prepared[name][variable] for name in group}
            self.assertEqual(len(paths), 1)
            path = Path(paths.pop())
            self.assertFalse(path.exists())
            self.assertTrue(path.parent.is_dir())
            self.assertEqual(ran[ran.index(group[0]):ran.index(group[0]) + len(group)], list(group))

    def test_default_compiler_pair_removes_selectors_without_changing_other_tests(self):
        selectors = {"RUSTC": "custom-rustc", "RUSTC_WRAPPER": "",
                     "RUSTC_WORKSPACE_WRAPPER": "custom-wrapper"}
        guards = {"RUSTFLAGS": "-D warnings", "CARGO_ENCODED_RUSTFLAGS": "-D\x1fwarnings",
                  "RUSTUP_TOOLCHAIN": "pinned-toolchain"}
        base = dict(self.base, **selectors, **guards, XDG_RUNTIME_DIR="/missing/ambient-runtime")
        snapshot = dict(base)
        for name in module.DEFAULT_COMPILER:
            selected = module.environment(name, self.root / "evidence", base)
            self.assertTrue(set(selectors).isdisjoint(selected))
            self.assertEqual(selected["XDG_RUNTIME_DIR"], str(self.root / "evidence/.runtime"))
            for guard, value in guards.items():
                self.assertEqual(selected[guard], value)
        ordinary = module.environment("ordinary::compiled", self.root / "evidence", base)
        for selector, value in selectors.items():
            self.assertEqual(ordinary[selector], value)
        self.assertEqual(base, snapshot)

    def test_removed_selectors_are_recorded_by_name_without_their_values(self):
        base = dict(self.base, RUSTC_WRAPPER="compiler-wrapper")
        name = next(iter(module.DEFAULT_COMPILER))
        evidence = self.root / "evidence"
        self.assertTrue(module.run_test(name, evidence, base, [self.root / "host"],
                                        runner=self.runner(), space=lambda _: module.RESERVE_BYTES))
        removed = json.loads((evidence / module.label(name) / "removed-environment.json").read_text())
        self.assertEqual(removed, ["RUSTC_WRAPPER"])
        self.assertNotIn("RUSTC_WRAPPER", self.calls[0][1])

    def test_warmup_is_serial_private_and_does_not_replace_consumer_homes(self):
        evidence = self.root / "evidence"
        base = dict(self.base, DOTNET_CLI_HOME="ambient-home", TMPDIR="ambient-temp",
                    DOTNET_ADD_GLOBAL_TOOLS_TO_PATH="true")
        snapshot = dict(base)
        runner = self.runner()
        def ordered(command, **kwargs):
            if command != ["dotnet", "--help"]:
                self.assertEqual(len(self.warmups), 1)
                self.assertEqual((evidence / "dotnet-warmup/exit-status.txt").read_text(), "0\n")
                self.assertEqual(kwargs["env"]["DOTNET_CLI_HOME"], "ambient-home")
                self.assertEqual(kwargs["env"]["TMPDIR"], "ambient-temp")
            return runner(command, **kwargs)
        self.assertEqual(module.run(self.tests, 1, evidence, base, runner=ordered,
                                    space=lambda _: module.RESERVE_BYTES), 0)
        settings, = self.warmups
        self.assertEqual(settings["DOTNET_CLI_HOME"], str(evidence / "dotnet-warmup/.dotnet-home"))
        self.assertEqual(settings["DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"], "false")
        self.assertEqual(settings["TMPDIR"], str(evidence / "dotnet-warmup/.tmp"))
        self.assertTrue(Path(settings["TMPDIR"]).is_dir())
        self.assertEqual(base, snapshot)

    def test_failed_warmup_fails_shard_without_running_or_retrying_tests(self):
        evidence = self.root / "evidence"
        self.assertEqual(module.run(self.tests, 1, evidence, self.base,
                                    runner=self.runner({"dotnet-warmup"})), 1)
        self.assertEqual(self.calls, [])
        self.assertEqual(len(self.warmups), 1)
        self.assertEqual((evidence / "failed.txt").read_text(), "dotnet-warmup\n")
        self.assertEqual((evidence / "dotnet-warmup/exit-status.txt").read_text(), "1\n")
        self.assertEqual((evidence / "dotnet-warmup/stdout.log").read_bytes(), b"first-use failed\n")
        self.assertTrue((evidence / "dotnet-warmup/stderr.log").is_file())

    @unittest.skipUnless(os.name == "posix", "POSIX runtime directory")
    def test_linked_runtime_directory_is_refused_before_starting_a_test(self):
        evidence = self.root / "evidence"
        (evidence / ".runtime").symlink_to(self.root / "host", target_is_directory=True)
        with self.assertRaises(ValueError):
            module.run_test(self.tests[0], evidence, self.base, [self.root / "host"],
                            runner=self.runner(), space=lambda _: module.RESERVE_BYTES)
        self.assertEqual(self.calls, [])

    def test_failed_preparation_skips_its_qualification_and_fails_the_shard(self):
        group = next(g for g in module.PREPARED.values() if len(g) == 2)
        shard = self.shard_of(group[0])
        evidence = self.root / "evidence"
        status = module.run(self.tests, shard, evidence, self.base, runner=self.runner({group[0]}),
                            space=lambda path: module.RESERVE_BYTES)
        self.assertEqual(status, 1)
        self.assertNotIn(group[1], [name for name, _ in self.calls])
        self.assertTrue((evidence / module.label(group[1]) / "skipped.txt").is_file())
        self.assertEqual((evidence / "failed.txt").read_text().split(), list(group))

    def test_missing_pass_summary_fails(self):
        def run(command, *, stdout, stderr, env):
            return SimpleNamespace(returncode=0)
        self.assertEqual(module.run(self.tests, 1, self.root / "evidence", self.base, runner=run,
                                    space=lambda path: module.RESERVE_BYTES), 1)

    def test_low_storage_refuses_before_running_compiled_tests(self):
        evidence = self.root / "evidence"
        self.assertEqual(module.run(self.tests, 1, evidence, self.base, runner=self.runner(),
                                    space=lambda path: module.RESERVE_BYTES - 1), 1)
        self.assertEqual(self.calls, [])
        self.assertEqual(len(self.warmups), 1)
        self.assertTrue(any(evidence.glob("*/storage-refusal.txt")))

    def test_requires_absolute_existing_directories(self):
        for base, evidence in ((self.base, Path("relative")), ({"PATH": "/usr/bin"}, self.root / "evidence"),
                               ({"FERRULE_CODEGEN_HOST_TARGET_DIR": str(self.root / "absent")},
                                self.root / "evidence")):
            with self.subTest(base=base, evidence=evidence), self.assertRaises(ValueError):
                module.run(self.tests, 1, evidence, base, runner=self.runner())


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.source = (ROOT / ".github/workflows/codegen.yml").read_text(encoding="utf8")
        start = self.source.index("\n  ignored-compiled-tests:\n")
        self.job = self.source[start:]
        self.pull_request = self.source[:start]

    def test_scheduled_job_covers_every_shard_and_retains_logs(self):
        self.assertRegex(self.source, r"\n  schedule:\n    - cron: '[^']+'\n")
        self.assertIn("if: github.event_name == 'schedule' || inputs.jobs == 'ignored-compiled-tests'", self.job)
        shards = re.search(r"\n        shard: \[([0-9, ]+)\]\n", self.job)
        self.assertEqual([int(value) for value in shards.group(1).split(",")],
                         list(range(1, module.SHARDS + 1)))
        self.assertIn("--run-ignored only --message-format json", self.job)
        self.assertIn("python3 -I scripts/ci/ignored-codegen-tests.py check --list", self.job)
        self.assertIn("python3 -I scripts/ci/ignored-codegen-tests.py run --list", self.job)
        self.assertIn("--shard ${{ matrix.shard }}", self.job)
        self.assertIn("uses: actions/upload-artifact@v4", self.job)
        self.assertIn("path: |", self.job)
        self.assertNotIn("path: ${{ steps.ignored.outputs.evidence_dir }}\n", self.job)
        for leaf in ("list.json", "failed.txt", "*/command.txt", "*/environment.json",
                     "*/removed-environment.json", "*/stdout.log", "*/stderr.log",
                     "*/exit-status.txt", "*/storage-refusal.txt", "*/skipped.txt"):
            self.assertIn("${{ steps.ignored.outputs.evidence_dir }}/" + leaf, self.job)
        self.assertIn("python3 -B scripts/ci/test-ignored-codegen-tests.py", self.job)

    def test_artifact_paths_select_only_logs_not_preparation_caches(self):
        matched = re.search(r"\n          path: \|\n((?:            [^\n]+\n)+)", self.job)
        self.assertIsNotNone(matched)
        prefix = "${{ steps.ignored.outputs.evidence_dir }}/"
        patterns = [line.strip().removeprefix(prefix) for line in matched.group(1).splitlines()]
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            logs = ("list.json", "failed.txt", "case/command.txt", "case/stdout.log",
                     "case/stderr.log", "case/environment.json", "case/removed-environment.json",
                     "case/exit-status.txt", "case/storage-refusal.txt", "case/skipped.txt",
                     "dotnet-warmup/command.txt", "dotnet-warmup/environment.json",
                     "dotnet-warmup/stdout.log", "dotnet-warmup/stderr.log", "dotnet-warmup/exit-status.txt")
            caches = ("case/prepare/.dotnet-home/http-cache/feed/list_package:10.0.dat",
                      "case/prepare/Host/bin/Host.dll", "case/prepare/Host/obj/compiler.log",
                      "dotnet-warmup/.dotnet-home/.dotnet/first-use-sentinel")
            files = logs + caches
            for name in files:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("test")
            selected = {str(path.relative_to(root)) for pattern in patterns for path in root.glob(pattern)}
            self.assertEqual(selected, set(logs))

    def test_pull_request_job_checks_selection_and_keeps_its_controls(self):
        self.assertIn("if: github.event_name != 'schedule' && inputs.jobs != 'ignored-compiled-tests'",
                      self.pull_request)
        self.assertIn("python3 -I scripts/ci/ignored-codegen-tests.py check --list", self.pull_request)
        self.assertIn("python3 -B scripts/ci/test-ignored-codegen-tests.py", self.pull_request)
        controls = re.findall(r"^            ([a-z0-9_]+::[a-z0-9_]+)(?: \\|;)", self.pull_request, re.M)
        self.assertEqual(len(controls), 9)
        self.assertTrue(set(controls).isdisjoint(module.EXCLUDED))


if __name__ == "__main__":
    unittest.main()
