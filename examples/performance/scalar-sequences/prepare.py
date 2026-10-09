#!/usr/bin/env python3
"""Materialize frozen independent fixtures; never compile or measure an application."""
import argparse
import hashlib
import json
import os
from pathlib import Path

DIMENSIONS = [("n1-l32", 1, 32), ("n16-l32", 16, 32), ("n128-l32", 128, 32),
              ("n16-l4096", 16, 4096), ("n16-l262144", 16, 262144),
              ("n128-l262144", 128, 262144)]
CEILING = 64 * 1024 * 1024


def identity(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"path": str(path), "bytes": path.stat().st_size,
            "sha256": digest.hexdigest()}


def repeated_ascii(file, length):
    chunk = b"x" * 65536
    while length:
        count = min(length, len(chunk))
        file.write(chunk[:count])
        length -= count


def source(path, count, width):
    with path.open("xb") as file:
        file.write(f'{{"Count":{count},"Capture":"'.encode("ascii"))
        repeated_ascii(file, width)
        file.write(b'"}\n')


def oracle(path, mode, count, width):
    # Complete manually fixed JSON layout, independent of every application writer.
    with path.open("xb") as file:
        file.write(b'{\n  "Rows": [\n')
        for index in range(1, count + 1):
            if index != 1:
                file.write(b",\n")
            file.write(b'    {\n      "Value": ')
            if mode == "numeric":
                file.write(str(index).encode("ascii"))
            else:
                file.write(b'"')
                repeated_ascii(file, width)
                file.write(b'"')
            file.write(b"\n    }")
        file.write(b"\n  ]\n}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory")
    spelling = parser.parse_args().directory
    directory = Path(spelling)
    if not directory.is_absolute() or spelling != os.path.normpath(spelling):
        parser.error("Use a fresh absolute output directory.")
    if directory.exists() or directory.is_symlink():
        parser.error("Output directory already exists.")
    here = Path(__file__).resolve().parent
    raw = (here / "fixtures.json").read_bytes()
    manifest = json.loads(raw)
    assert [(row["id"], row["item_count"], row["capture_utf8_bytes"])
            for row in manifest["dimensions"]] == DIMENSIONS
    assert manifest["fixture_count"] == len(manifest["fixtures"]) == 12
    assert manifest["document_byte_ceiling_exclusive"] == CEILING
    directory.mkdir()
    with (directory / "frozen-contract.original.json").open("xb") as file:
        file.write(raw)
    projects = {}
    for mode in ("numeric", "capture"):
        body = (here / f"project-{mode}.json").read_bytes()
        path = directory / f"project-{mode}.json"
        with path.open("xb") as file:
            file.write(body)
        projects[mode] = identity(path)
    inputs = {}
    for name, count, width in DIMENSIONS:
        path = directory / f"{name}.input.json"
        source(path, count, width)
        assert path.stat().st_size == 24 + len(str(count)) + width < CEILING
        inputs[name] = identity(path)
    frozen = []
    for row in manifest["fixtures"]:
        mode = "numeric" if row["project"] == "project-numeric.json" else "capture"
        assert row["project"] == f"project-{mode}.json"
        name, count, width = next(v for v in DIMENSIONS if v[0] == row["dimension"])
        assert (row["item_count"], row["capture_utf8_bytes"]) == (count, width)
        assert row["id"] == f"{name}-{mode}"
        assert row["input_bytes"] == 24 + len(str(count)) + width
        path = directory / f'{row["id"]}.expected.json'
        oracle(path, mode, count, width)
        assert path.stat().st_size == row["expected_document_bytes"] < CEILING
        frozen.append({"id": row["id"], "mode": mode, "input": inputs[name],
                       "project": projects[mode], "expected": identity(path),
                       "typed_recipe": row["complete_typed_output_recipe"]})
    controls = []
    for row in manifest["qualification_only_small_literals"]:
        mode = "numeric" if row["project"] == "project-numeric.json" else "capture"
        assert row["project"] == f"project-{mode}.json"
        assert row["id"] == f"ordered-{mode}-3"
        input_path = directory / f'{row["id"]}.input.json'
        expected_path = directory / f'{row["id"]}.expected.json'
        source(input_path, 3, 4)
        oracle(expected_path, mode, 3, 4)
        assert input_path.read_bytes() == row["input"].encode("ascii")
        assert expected_path.read_bytes() == row["expected"].encode("ascii")
        controls.append({"id": row["id"], "input": identity(input_path),
                         "expected": identity(expected_path)})
    trials = []
    for repeat in (1, 2):
        dimensions = DIMENSIONS if repeat == 1 else list(reversed(DIMENSIONS))
        backends = ("native", "rust", "csharp") if repeat == 1 else ("csharp", "rust", "native")
        modes = ("numeric", "capture") if repeat == 1 else ("capture", "numeric")
        for name, _, _ in dimensions:
            for backend in backends:
                for mode in modes:
                    fixture = next(row for row in frozen if row["id"] == f"{name}-{mode}")
                    trials.append({"ordinal": len(trials) + 1, "repeat": repeat,
                                   "backend": backend, "fixture": fixture["id"],
                                   "input": fixture["input"], "project": fixture["project"],
                                   "expected": fixture["expected"],
                                   "host_binary": None, "run_argv": None, "verify_argv": None,
                                   "status": "UNRUN_REQUIRES_REVIEWED_HOST_AND_SOURCE_BINDINGS"})
    assert len(trials) == 72
    campaign = {"status": "MATERIALIZED_FIXTURES_ONLY_ALL_BUILD_AND_MEASUREMENT_UNRUN",
                "frozen_contract": identity(directory / "frozen-contract.original.json"),
                "projects": projects, "fixtures": frozen, "small_controls": controls,
                "trials": trials, "compilation_and_verification_excluded_from_measurement": True}
    with (directory / "campaign.json").open("x", encoding="utf-8") as file:
        json.dump(campaign, file, indent=2)
        file.write("\n")
    print(json.dumps({"campaign": str(directory / "campaign.json"),
                      "status": campaign["status"], "trials": 72}))


if __name__ == "__main__":
    main()
