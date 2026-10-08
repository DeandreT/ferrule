#!/usr/bin/env python3
"""Prepare independent JSON fixtures and eight trial argv; do not launch measured processes."""
import argparse
import hashlib
import json
from pathlib import Path

WORKLOADS = {"many": (32768, 1024), "wide": (1, 33554432)}


def scalar(name, ty):
    return {"name": name, "kind": {"kind": "scalar", "ty": ty}}


def document(name):
    return {"name": name, "kind": {"kind": "group", "children": [
        {"name": "Rows", "repeating": True, "kind": {"kind": "group", "children": [
            scalar("Id", "int"), scalar("Text", "string")]}}]}}


def mapping():
    scope = {"children": [{"target_field": "Rows", "source": ["Rows"], "bindings": [
        {"target_field": "Id", "node": 0}, {"target_field": "Text", "node": 1}]}]}
    return {"source": document("Input"), "target": document("Primary"),
        "target_path": "primary.json", "source_options": {"json_document": True},
        "target_options": {"json_document": True}, "extra_targets": [{"name": "mirror",
            "path": "named.json", "schema": document("Mirror"),
            "options": {"json_document": True}, "root": scope}],
        "graph": {"nodes": {"0": {"kind": "source_field", "path": ["Id"], "frame": ["Rows"]},
            "1": {"kind": "source_field", "path": ["Text"], "frame": ["Rows"]}}}, "root": scope}


def text(file, row, width):
    file.write(f"{row:08}:".encode("ascii"))
    remaining = width - 10
    chunk = b"x" * 65536
    while remaining:
        n = min(len(chunk), remaining)
        file.write(chunk[:n])
        remaining -= n
    file.write(b";")


def write_input(path, rows, width):
    with path.open("xb") as file:
        file.write(b'{"Rows":[')
        for row in range(rows):
            if row:
                file.write(b",")
            file.write(f'{{"Id":{row},"Text":"'.encode("ascii"))
            text(file, row, width)
            file.write(b'"}')
        file.write(b"]}\n")


def write_expected(path, rows, width):
    # Hand-authored public pretty JSON layout. No application/format writer supplies this oracle.
    with path.open("xb") as file:
        file.write(b'{\n  "Rows": [\n')
        for row in range(rows):
            if row:
                file.write(b",\n")
            file.write(f'    {{\n      "Id": {row},\n      "Text": "'.encode("ascii"))
            text(file, row, width)
            file.write(b'"\n    }')
        file.write(b"\n  ]\n}\n")


def identity(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1048576), b""):
            digest.update(chunk)
    return {"path": str(path), "bytes": path.stat().st_size, "sha256": digest.hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    root = args.directory.absolute()
    root.mkdir()  # A fresh directory is required; no old evidence is replaced or removed.
    project = (json.dumps(mapping(), indent=2) + "\n").encode("utf-8")
    (root / "project.original.json").write_bytes(project)
    workloads = {}
    for name, (rows, width) in WORKLOADS.items():
        source = root / f"{name}.input.json"
        expected = root / f"{name}.expected.json"
        write_input(source, rows, width)
        write_expected(expected, rows, width)
        workloads[name] = {"rows": rows, "text_bytes_per_row": width,
            "input": identity(source), "expected": identity(expected)}
    trials = []
    for repeat in (1, 2):
        for name in WORKLOADS:
            for selection in (("one", "two") if repeat == 1 else ("two", "one")):
                directory = root / f"r{repeat}-{name}-{selection}"
                directory.mkdir()
                saved = directory / "project.json"
                saved.write_bytes(project)
                source = workloads[name]["input"]["path"]
                expected = workloads[name]["expected"]["path"]
                trials.append({"ordinal": len(trials) + 1, "repeat": repeat, "workload": name,
                    "outputs": 1 if selection == "one" else 2, "project": identity(saved),
                    "run_argv": ["run", str(saved), source, selection],
                    "verify_argv": ["verify", str(saved), source, selection, name, expected]})
    manifest = {"status": "PREPARED_INPUTS_ONLY_NO_MEASUREMENTS", "project": identity(root / "project.original.json"),
        "workloads": workloads, "trials": trials, "repetitions": 2,
        "order": "serial; paired one/two, then paired two/one in the second repetition",
        "expected_output_kind": "Group(Rows:Repeated(Group(Id:Int,Text:String))), ordered and unknown XML origin",
        "fixture_generation_and_all_verification_outside_measured_process": True}
    (root / "campaign.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"campaign": str(root / "campaign.json"), "status": manifest["status"]}))


if __name__ == "__main__":
    main()
