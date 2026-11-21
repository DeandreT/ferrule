"""Check the bounded saved JSON string parser through the CLI parameter API."""

import json
from pathlib import Path
import shutil


def typed_json(value):
    return json.dumps(value, sort_keys=True, allow_nan=False)


def check(cli, fixtures):
    source = fixtures / "json-array-parser"
    for filename in ("mapping.mfd", "source.schema.json", "payload.schema.json", "target.schema.json"):
        shutil.copyfile(source / filename, filename)
    Path("source.json").write_bytes(b"{}")
    imported = cli.run(
        "import-mfd", "--mfd", "mapping.mfd", "--out", "project.json",
        "--package-root", ".", "--require-executable",
    )
    assert "(0 warning(s))" in imported.stdout and not imported.stderr
    assert Path("project.json").is_file()
    cases = json.loads((source / "cases.json").read_bytes())
    assert len(cases) == 23
    complete = refusals = 0
    for index, case in enumerate([*cases, None], 1):
        output = Path(f"case-{index:02}.json")
        args = ["run", "--project", "project.json", "--input", "source.json", "--output", output]
        if case is not None:
            args.extend(["--param", "InputText=" + case["input"]])
        success = case is not None and "expected" in case
        observed = cli.run(*args, success=success)
        if success:
            assert not observed.stderr
            assert typed_json(json.loads(output.read_bytes())) == typed_json(case["expected"]), index
            complete += 1
        else:
            if case is None:
                assert observed.stderr.strip() == (
                    "Error: graph node 0: execution context does not provide runtime parameter `InputText`"
                )
            else:
                assert case["error"] in observed.stderr, index
            assert not output.exists(), index
            refusals += 1
    assert (complete, refusals) == (9, 15)
    return complete, refusals
