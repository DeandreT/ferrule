"""Exercise the local SELECT/workbook contracts using an already-built Ferrule CLI."""

import argparse
from contextlib import chdir
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent
FIXTURES = HERE.parents[2] / "crates/mfd/tests/fixtures"


class Cli:
    def __init__(self, executable):
        self.executable = executable.resolve(strict=True)

    def run(self, *args, success=True):
        result = subprocess.run([self.executable, *args], capture_output=True, encoding="utf-8", timeout=60)
        if (result.returncode == 0) != success:
            raise AssertionError(f"{args}: exit {result.returncode}\n{result.stdout}\n{result.stderr}")
        return result

    def import_pair(self, design):
        """The default is best effort. Check strict independently, then compare projects."""
        projects = []
        for label, flags in [("strict", ["--require-executable"]), ("best", [])]:
            project = Path(design).with_suffix(f".{label}.json")
            result = self.run("import-mfd", "--mfd", design, "--out", project, "--package-root", ".", *flags)
            assert "0 warning(s)" in result.stdout and not result.stderr, result
            projects.append(project)
        assert json.loads(projects[0].read_bytes()) == json.loads(projects[1].read_bytes())
        return projects[0]

    def export(self, project, profile, issue=None):
        folder = Path(f"{project.stem}-{profile}")
        folder.mkdir()
        design = folder / "saved.mfd"
        result = self.run("export-mfd", "--project", project, "--out", design, "--profile", profile,
                          "--report-json", success=issue is None or profile == "ferrule-extensions")
        report = json.loads(result.stdout)
        assert report["accepted"] == (result.returncode == 0) and design.exists() == report["accepted"], report
        if not report["accepted"]:
            assert not list(folder.iterdir()), report
        assert {i["feature"] for i in report["report"]["issues"]} == ({issue} if issue else set()), report
        return design


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--case", choices=["select", "workbook", "both"], default="both")
    args = parser.parse_args()
    if not __debug__:
        parser.error("run without Python optimization so the oracle assertions execute")
    cli = Cli(args.executable)
    with TemporaryDirectory() as directory, chdir(directory):
        if args.case != "workbook":
            from select_cases import check
            check(cli, HERE / "select")
            print("SELECT: ordered Call results, argument failures, direct result and local roundtrips passed")
        if args.case != "select":
            from workbook_cases import check
            check(cli, FIXTURES)
            print("XLSX: fresh/replace, styled update, formula boundaries and local roundtrips passed")
