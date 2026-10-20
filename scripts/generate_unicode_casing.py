#!/usr/bin/env python3
"""Generate the frozen Unicode 17 full-casing data; no network or packages."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

# Reuse the JSON5 generator's pinned-input I/O without leaving bytecode beside it.
sys.dont_write_bytecode = True
from generate_json5_identifiers import (  # noqa: E402
    check_existing,
    read_regular,
    require,
    sha256,
    write_fresh,
)


VERSION = "17.0.0"
CODEPOINTS = 0x110000
MAX_INPUT_BYTES = 4 * 1024 * 1024
MAX_OUTPUT_BYTES = 512 * 1024
MAX_RECORDS = 2048
MAX_EXPANSION = 3
CAPITAL_SIGMA = 0x03A3
SMALL_SIGMA = 0x03C3
FINAL_SIGMA = 0x03C2
INPUTS = {
    "unicode_data": (
        2198209,
        "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c",
    ),
    "special_casing": (
        17049,
        "efc25faf19de21b92c1194c111c932e03d2a5eaf18194e33f1156e96de4c9588",
    ),
    "derived_core_properties": (
        1134783,
        "24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08",
    ),
    "readme": (
        740,
        "9fe1a90bd32659d7953616283dc2bffaa165518aae9ace026040c42c559ba606",
    ),
    "license": (
        1995,
        "e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96",
    ),
}
PROPERTIES = ("Cased", "Case_Ignorable", "Lowercase", "Uppercase")
SCALAR = re.compile(r"[0-9A-F]{4,6}")
PROPERTY_LINE = re.compile(r"([0-9A-F]{4,6})(?:\.\.([0-9A-F]{4,6}))?\s*;\s*([A-Za-z_]+)")
RUN_LINE = re.compile(r"(upper|lower) ([0-9A-F]{6})\.\.([0-9A-F]{6}) ([12]) ([+-][0-9]+)")
EXPANSION_LINE = re.compile(r"(upper|lower) ([0-9A-F]{6}) ([0-9A-F]{6}(?: [0-9A-F]{6}){1,2})")
CLASS_LINE = re.compile(r"(cased|ignorable) ([0-9A-F]{6})\.\.([0-9A-F]{6})")


def pinned_inputs(args: argparse.Namespace) -> dict[str, bytes]:
    bodies = {}
    for role, (length, digest) in INPUTS.items():
        body = read_regular(getattr(args, role), MAX_INPUT_BYTES)
        require(len(body) == length and sha256(body) == digest,
                f"unqualified input: {role}")
        body.decode("utf-8", errors="strict")
        require(b"\r" not in body, f"noncanonical input newline: {role}")
        bodies[role] = body
    for role, name in (("special_casing", "SpecialCasing"),
                       ("derived_core_properties", "DerivedCoreProperties")):
        require(bodies[role].startswith(f"# {name}-{VERSION}.txt\n".encode("ascii")),
                f"{name} version differs")
    require(b"Version 17.0.0 of the Unicode Standard." in bodies["readme"],
            "ReadMe version differs")
    require(bodies["license"].startswith(b"UNICODE LICENSE V3\n"),
            "license version differs")
    require(bodies["license"].endswith(b"\n"), "license lacks terminal LF")
    return bodies


def scalars(field: str) -> list[int]:
    values = field.split()
    require(all(SCALAR.fullmatch(value) for value in values), "malformed scalar list")
    result = [int(value, 16) for value in values]
    require(all(0 < value < CODEPOINTS and not 0xD800 <= value <= 0xDFFF
                for value in result), "mapping contains an invalid scalar")
    return result


def unicode_data(body: bytes):
    """Simple single-scalar mappings and titlecase letters from UnicodeData."""
    upper, lower, titlecase = {}, {}, set()
    previous = -1
    for number, line in enumerate(body.decode("utf-8").splitlines(), 1):
        fields = line.split(";")
        require(len(fields) == 15 and SCALAR.fullmatch(fields[0]) is not None,
                f"malformed UnicodeData line {number}")
        point = int(fields[0], 16)
        require(previous < point < CODEPOINTS, "UnicodeData is not strictly ordered")
        previous = point
        if fields[1].endswith((", First>", ", Last>")):
            require(fields[12] == fields[13] == "", "range record has case mappings")
            continue
        if fields[2] == "Lt":
            titlecase.add(point)
        for table, field in ((upper, fields[12]), (lower, fields[13])):
            if field:
                mapped = scalars(field)
                require(len(mapped) == 1, "simple mapping is not one scalar")
                table[point] = mapped[0]
    return upper, lower, titlecase


def special_casing(body: bytes):
    """Unconditional full mappings plus the only language-neutral condition."""
    upper, lower = {}, {}
    final_sigma = False
    for number, line in enumerate(body.decode("utf-8").splitlines(), 1):
        content = line.split("#", 1)[0].strip()
        if not content:
            continue
        fields = [field.strip() for field in content.split(";")]
        require(len(fields) in (5, 6) and fields[-1] == "",
                f"malformed SpecialCasing line {number}")
        point = scalars(fields[0])
        require(len(point) == 1, "SpecialCasing code is not one scalar")
        point = point[0]
        if len(fields) == 6:
            conditions = fields[4].split()
            if conditions == ["Final_Sigma"]:
                require(point == CAPITAL_SIGMA and scalars(fields[1]) == [FINAL_SIGMA],
                        "unexpected Final_Sigma record")
                final_sigma = True
            else:
                # Language-specific mappings (lt, tr, az) are outside the contract.
                require(bool(conditions) and re.fullmatch(r"[a-z]{2}", conditions[0])
                        is not None, f"unsupported language-neutral condition: {line}")
            continue
        for table, field in ((lower, fields[1]), (upper, fields[3])):
            mapped = scalars(field)
            require(1 <= len(mapped) <= MAX_EXPANSION, "full mapping length out of range")
            require(point not in table, "duplicate unconditional SpecialCasing record")
            table[point] = mapped
    require(final_sigma, "Final_Sigma record is absent")
    return upper, lower


def properties(body: bytes) -> dict[str, bytearray]:
    result = {name: bytearray(CODEPOINTS) for name in PROPERTIES}
    for number, line in enumerate(body.decode("utf-8").splitlines(), 1):
        content = line.split("#", 1)[0].strip()
        if not content:
            continue
        match = PROPERTY_LINE.fullmatch(content)
        if match is None:
            # Enumerated properties such as InCB carry a second value field.
            require(content.count(";") == 2, f"malformed DerivedCoreProperties line {number}")
            continue
        if match[3] not in result:
            continue
        first, last = int(match[1], 16), int(match[2] or match[1], 16)
        require(first <= last < CODEPOINTS, "out-of-range property record")
        table = result[match[3]]
        require(not any(table[first:last + 1]), "overlapping property records")
        table[first:last + 1] = b"\x01" * (last + 1 - first)
    require(all(any(table) for table in result.values()), "missing casing property")
    return result


def full_mappings(simple: dict[int, int], special: dict[int, list[int]]) -> dict[int, list[int]]:
    result = {}
    for point in range(CODEPOINTS):
        if 0xD800 <= point <= 0xDFFF:
            continue
        mapped = special.get(point)
        if mapped is None:
            mapped = [simple.get(point, point)]
        if mapped != [point]:
            result[point] = mapped
    return result


def runs(mapping: dict[int, list[int]]) -> list[tuple[int, int, int, int]]:
    """Greedy stride-1/2 runs; a stride-2 run skips only unmapped scalars."""
    result: list[list[int]] = []
    for point in sorted(mapping):
        if len(mapping[point]) != 1:
            continue
        delta = mapping[point][0] - point
        if result:
            first, last, stride, run_delta = result[-1]
            gap = point - last
            if run_delta == delta and gap in (1, 2) and (first == last or gap == stride):
                result[-1] = [first, point, gap, delta]
                continue
        result.append([point, point, 1, delta])
    return [tuple(run) for run in result]


def expansions(mapping: dict[int, list[int]]) -> list[tuple[int, list[int]]]:
    return [(point, mapped) for point, mapped in sorted(mapping.items()) if len(mapped) > 1]


def ranges(table: bytearray) -> list[tuple[int, int]]:
    result = []
    for point in range(CODEPOINTS):
        if not table[point]:
            continue
        if result and result[-1][1] + 1 == point:
            result[-1] = (result[-1][0], point)
        else:
            result.append((point, point))
    return result


def canonical(tables: dict) -> bytes:
    header = (
        f"# Unicode-Version: {VERSION}\n"
        + "".join(f"# {role}-SHA256: {digest}\n"
                  for role, (_, digest) in sorted(INPUTS.items()))
        + "# License: LICENSE-UNICODE.txt\n"
        "# Records: direction first..last stride delta; direction scalar expansion;"
        " class first..last\n"
    )
    lines = []
    for direction in ("upper", "lower"):
        for first, last, stride, delta in tables[direction + "_runs"]:
            lines.append(f"{direction} {first:06X}..{last:06X} {stride} {delta:+d}\n")
        for point, mapped in tables[direction + "_expansions"]:
            lines.append(f"{direction} {point:06X} "
                         + " ".join(f"{value:06X}" for value in mapped) + "\n")
    for name in ("cased", "ignorable"):
        for first, last in tables[name]:
            lines.append(f"{name} {first:06X}..{last:06X}\n")
    return (header + "".join(lines)).encode("ascii")


def parse_canonical(body: bytes) -> dict:
    tables = {key: [] for key in ("upper_runs", "upper_expansions", "lower_runs",
                                  "lower_expansions", "cased", "ignorable")}
    for line in body.decode("ascii").splitlines():
        if line.startswith("#"):
            continue
        if match := RUN_LINE.fullmatch(line):
            tables[match[1] + "_runs"].append(
                (int(match[2], 16), int(match[3], 16), int(match[4]), int(match[5])))
        elif match := EXPANSION_LINE.fullmatch(line):
            tables[match[1] + "_expansions"].append(
                (int(match[2], 16), [int(value, 16) for value in match[3].split()]))
        elif match := CLASS_LINE.fullmatch(line):
            tables[match[1]].append((int(match[2], 16), int(match[3], 16)))
        else:
            raise ValueError(f"malformed canonical record: {line}")
    for key, records in tables.items():
        require(0 < len(records) <= MAX_RECORDS, f"record cap exceeded: {key}")
        firsts = [record[0] for record in records]
        require(firsts == sorted(set(firsts)), f"records are unordered: {key}")
    require(canonical(tables) == body, "noncanonical casing bytes")
    return tables


def lookup(point: int, runs_table, expansions_table) -> list[int]:
    """Mirror of the Rust/C# lookup: expansion table first, then runs."""
    low, high = 0, len(expansions_table)
    while low < high:
        middle = (low + high) // 2
        if expansions_table[middle][0] < point:
            low = middle + 1
        else:
            high = middle
    if low < len(expansions_table) and expansions_table[low][0] == point:
        return expansions_table[low][1]
    low, high = 0, len(runs_table)
    while low < high:
        middle = (low + high) // 2
        first, last, stride, delta = runs_table[middle]
        if point < first:
            high = middle
        elif point > last:
            low = middle + 1
        else:
            return [point + delta] if (point - first) % stride == 0 else [point]
    return [point]


def in_ranges(point: int, table) -> bool:
    low, high = 0, len(table)
    while low < high:
        middle = (low + high) // 2
        first, last = table[middle]
        if point < first:
            high = middle
        elif point > last:
            low = middle + 1
        else:
            return True
    return False


def verify(tables: dict, upper: dict, lower: dict, props: dict[str, bytearray]) -> None:
    for direction, expected in (("upper", upper), ("lower", lower)):
        runs_table = tables[direction + "_runs"]
        expansions_table = tables[direction + "_expansions"]
        previous = -1
        for first, last, stride, _ in runs_table:
            require(previous < first <= last and stride in (1, 2), "overlapping runs")
            previous = last
        for point in range(CODEPOINTS):
            if 0xD800 <= point <= 0xDFFF:
                continue
            require(lookup(point, runs_table, expansions_table) == expected.get(point, [point]),
                    f"{direction} mapping differs at U+{point:06X}")
    for name, prop in (("cased", "Cased"), ("ignorable", "Case_Ignorable")):
        for point in range(CODEPOINTS):
            require(in_ranges(point, tables[name]) == bool(props[prop][point]),
                    f"{prop} differs at U+{point:06X}")


def source_header(license_body: bytes, canonical_body: bytes) -> str:
    notice = license_body.decode("utf-8")
    require("\x00" not in notice and "\r" not in notice, "unsafe notice bytes")
    return (
        "// Generated by scripts/generate_unicode_casing.py; do not edit.\n"
        f"// Unicode {VERSION}; canonical SHA256 {sha256(canonical_body)}\n"
        + "".join(("// " + line if line else "//") + "\n" for line in notice.splitlines())
        + "\n"
    )


def rows(tables: dict, key: str, prefix: str) -> str:
    """Shared row syntax; the Rust test compares both leaves row by row."""
    result = []
    for record in tables[key]:
        if key.endswith("_runs"):
            first, last, stride, delta = record
            body = f"0x{first:06X}, 0x{last:06X}, {stride}, {delta}"
        elif key.endswith("_expansions"):
            point, mapped = record
            padded = mapped + [0] * (MAX_EXPANSION - len(mapped))
            body = f"0x{point:06X}, " + ", ".join(f"0x{value:06X}" for value in padded)
        else:
            first, last = record
            body = f"0x{first:06X}, 0x{last:06X}"
        result.append(f"{prefix}({body}),\n")
    return "".join(result)


TABLE_NAMES = (
    ("upper_runs", "UPPER_RUNS", "UpperRuns", "Run"),
    ("upper_expansions", "UPPER_EXPANSIONS", "UpperExpansions", "Expansion"),
    ("lower_runs", "LOWER_RUNS", "LowerRuns", "Run"),
    ("lower_expansions", "LOWER_EXPANSIONS", "LowerExpansions", "Expansion"),
    ("cased", "CASED", "Cased", "ScalarRange"),
    ("ignorable", "CASE_IGNORABLE", "CaseIgnorable", "ScalarRange"),
)
RUST_TYPES = {
    "Run": "(u32, u32, u32, i32)",
    "Expansion": "(u32, u32, u32, u32)",
    "ScalarRange": "(u32, u32)",
}


def rust_leaf(tables: dict, canonical_body: bytes, license_body: bytes) -> bytes:
    source = source_header(license_body, canonical_body) + (
        "// Rows: runs are (first, last, stride, delta); expansions are\n"
        "// (scalar, mapped...) padded with 0; class rows are inclusive ranges.\n"
    )
    for key, rust_name, _, kind in TABLE_NAMES:
        source += (f"\n#[rustfmt::skip]\npub(super) const {rust_name}: &[{RUST_TYPES[kind]}] = &[\n"
                   + rows(tables, key, "    ") + "];\n")
    return source.encode("utf-8")


def csharp_leaf(tables: dict, canonical_body: bytes, license_body: bytes) -> bytes:
    source = source_header(license_body, canonical_body) + (
        "namespace Ferrule.Runtime;\n\n"
        "internal static partial class FerruleUnicodeCasing\n{\n"
        "    // Rows: runs are (first, last, stride, delta); expansions are\n"
        "    // (scalar, mapped...) padded with 0; class rows are inclusive ranges.\n"
    )
    for key, _, csharp_name, kind in TABLE_NAMES:
        source += (f"\n    private static readonly {kind}[] {csharp_name} =\n    [\n"
                   + rows(tables, key, "        new") + "    ];\n")
    source += "}\n"
    return source.encode("utf-8")


def generate(bodies: dict[str, bytes]) -> dict[str, bytes]:
    simple_upper, simple_lower, titlecase = unicode_data(bodies["unicode_data"])
    special_upper, special_lower = special_casing(bodies["special_casing"])
    props = properties(bodies["derived_core_properties"])
    for point in range(CODEPOINTS):
        derived = props["Lowercase"][point] or props["Uppercase"][point] or point in titlecase
        require(bool(props["Cased"][point]) == bool(derived),
                f"Cased differs from Lowercase/Uppercase/Lt at U+{point:06X}")
    require(simple_lower.get(CAPITAL_SIGMA) == SMALL_SIGMA, "capital sigma mapping differs")
    upper = full_mappings(simple_upper, special_upper)
    lower = full_mappings(simple_lower, special_lower)
    projected = {
        "upper_runs": runs(upper),
        "upper_expansions": expansions(upper),
        "lower_runs": runs(lower),
        "lower_expansions": expansions(lower),
        "cased": ranges(props["Cased"]),
        "ignorable": ranges(props["Case_Ignorable"]),
    }
    canonical_body = canonical(projected)
    tables = parse_canonical(canonical_body)
    require(tables == projected, "canonical roundtrip differs")
    verify(tables, upper, lower, props)
    outputs = {
        "casing_tables.txt": canonical_body,
        "tables.rs": rust_leaf(tables, canonical_body, bodies["license"]),
        "FerruleUnicodeCasing.Tables.cs": csharp_leaf(tables, canonical_body, bodies["license"]),
        "LICENSE-UNICODE.txt": bodies["license"],
    }
    report = {
        "unicode_version": VERSION,
        "input_pins": {role: {"bytes": length, "sha256": digest}
                       for role, (length, digest) in sorted(INPUTS.items())},
        "canonical_sha256": sha256(canonical_body),
        "record_counts": {key: len(records) for key, records in sorted(tables.items())},
        "record_cap": MAX_RECORDS,
        "mapping_and_class_codepoints_compared": CODEPOINTS,
        "outputs": {name: {"bytes": len(body), "sha256": sha256(body)}
                    for name, body in sorted(outputs.items())},
    }
    outputs["generation.json"] = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode("ascii")
    require(all(len(body) <= MAX_OUTPUT_BYTES for body in outputs.values()),
            "generated file exceeds fixed output bound")
    return outputs


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for role in INPUTS:
        parser.add_argument("--" + role.replace("_", "-"), type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--check", action="store_true", help="compare complete regenerated files without writing")
    args = parser.parse_args()
    outputs = generate(pinned_inputs(args))
    if args.check:
        check_existing(args.output_root, outputs)
    else:
        write_fresh(args.output_root, outputs)
    print(json.dumps({"status": "CHECKED" if args.check else "GENERATED",
                      "outputs": len(outputs), "unicode_version": VERSION}, sort_keys=True))


if __name__ == "__main__":
    main()
