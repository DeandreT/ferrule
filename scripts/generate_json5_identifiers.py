#!/usr/bin/env python3
"""Generate the frozen Unicode 17 JSON5 identifier data; no network or packages."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat


VERSION = "17.0.0"
CODEPOINTS = 0x110000
MAX_INPUT_BYTES = 4 * 1024 * 1024
MAX_OUTPUT_BYTES = 512 * 1024
MAX_RANGES = 4096
MAX_PROBES = 13
INPUTS = {
    "derived_general_category": (
        277514,
        "d62e5bab70ca74f099343f71224fa051cb1fdd61a1ab45c0488c44cfc0b6102e",
    ),
    "unicode_data": (
        2198209,
        "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c",
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
CATEGORIES = (
    "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No",
    "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So",
    "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co", "Cn",
)
CATEGORY_ID = {name: index for index, name in enumerate(CATEGORIES)}
START = frozenset(("Lu", "Ll", "Lt", "Lm", "Lo", "Nl"))
CONTINUE_ONLY = frozenset(("Mn", "Mc", "Nd", "Pc"))
RANGE_LINE = re.compile(r"([0-9A-F]{4,6})(?:\.\.([0-9A-F]{4,6}))?\s*;\s*([A-Za-z]{2})")
CANONICAL_LINE = re.compile(r"([0-9A-F]{6})\.\.([0-9A-F]{6});([23])")
STAT_FIELDS = (
    "st_dev", "st_ino", "st_mode", "st_nlink", "st_uid", "st_gid",
    "st_size", "st_mtime_ns", "st_ctime_ns",
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def sha256(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def read_regular(path: Path, bound: int) -> bytes:
    """Reject aliases and mutation; consume the complete bounded regular file."""
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(path, flags)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode), f"not a regular file: {path}")
        require(0 <= before.st_size <= bound, f"file exceeds bound: {path}")
        with os.fdopen(fd, "rb", closefd=False) as handle:
            body = handle.read(bound + 1)
            require(handle.read(1) == b"", f"incomplete EOF: {path}")
        after = os.fstat(fd)
        named = os.lstat(path)
        require(all(getattr(before, k) == getattr(after, k) for k in STAT_FIELDS),
                f"file changed during read: {path}")
        require(all(getattr(after, k) == getattr(named, k) for k in STAT_FIELDS),
                f"path changed during read: {path}")
        require(len(body) == after.st_size and len(body) <= bound,
                f"length mismatch: {path}")
        return body
    finally:
        os.close(fd)


def pinned_inputs(args: argparse.Namespace) -> dict[str, bytes]:
    bodies = {}
    for role, (length, digest) in INPUTS.items():
        body = read_regular(getattr(args, role), MAX_INPUT_BYTES)
        require(len(body) == length and sha256(body) == digest,
                f"unqualified input: {role}")
        body.decode("utf-8", errors="strict")
        require(b"\r" not in body, f"noncanonical input newline: {role}")
        bodies[role] = body
    require(b"# DerivedGeneralCategory-17.0.0.txt\n" in
            bodies["derived_general_category"].splitlines(keepends=True)[:1],
            "DerivedGeneralCategory version differs")
    require(b"Version 17.0.0 of the Unicode Standard." in bodies["readme"],
            "ReadMe version differs")
    require(bodies["license"].startswith(b"UNICODE LICENSE V3\n"),
            "license version differs")
    require(bodies["license"].endswith(b"\n"), "license lacks terminal LF")
    return bodies


def paint(categories: bytearray, first: int, last: int, category: str,
          occupied: bytearray | None = None) -> None:
    require(category in CATEGORY_ID, f"unknown category: {category}")
    require(0 <= first <= last < CODEPOINTS, "out-of-range UCD record")
    if occupied is not None:
        require(not any(occupied[first:last + 1]), "overlapping UCD records")
        occupied[first:last + 1] = b"\x01" * (last + 1 - first)
    categories[first:last + 1] = bytes((CATEGORY_ID[category],)) * (last + 1 - first)


def derived_categories(body: bytes) -> bytearray:
    """Parse category ranges independently of UnicodeData's record encoding."""
    result = bytearray([255]) * CODEPOINTS
    occupied = bytearray(CODEPOINTS)
    records = []
    for number, line in enumerate(body.decode("utf-8").splitlines(), 1):
        content = line.split("#", 1)[0].strip()
        if not content:
            continue
        match = RANGE_LINE.fullmatch(content)
        require(match is not None, f"malformed DerivedGeneralCategory line {number}")
        first = int(match[1], 16)
        last = int(match[2] or match[1], 16)
        records.append((first, last, match[3]))
    require(bool(records), "empty DerivedGeneralCategory")
    for first, last, category in sorted(records):
        paint(result, first, last, category, occupied)
    require(all(occupied), "DerivedGeneralCategory does not cover every code point")
    require(255 not in result, "unassigned category slot")
    return result


def unicode_data_categories(body: bytes) -> bytearray:
    """Expand First/Last pairs, with omitted code points explicitly Cn."""
    result = bytearray([CATEGORY_ID["Cn"]]) * CODEPOINTS
    occupied = bytearray(CODEPOINTS)
    previous = -1
    pending = None
    for number, line in enumerate(body.decode("utf-8").splitlines(), 1):
        fields = line.split(";")
        require(len(fields) == 15, f"malformed UnicodeData line {number}")
        require(re.fullmatch(r"[0-9A-F]{4,6}", fields[0]) is not None,
                f"malformed UnicodeData scalar at line {number}")
        point = int(fields[0], 16)
        require(previous < point < CODEPOINTS, "UnicodeData is not strictly ordered")
        previous = point
        name, category = fields[1], fields[2]
        require(category in CATEGORY_ID, "unknown UnicodeData category")
        if pending is not None:
            first, stem, first_fields = pending
            require(name.endswith(", Last>") and name[:-len(", Last>")] == stem,
                    "UnicodeData First record lacks its immediate matching Last")
            require(first_fields[2:] == fields[2:] and point > first,
                    "UnicodeData First/Last properties differ")
            paint(result, first, point, category, occupied)
            pending = None
        elif name.endswith(", First>"):
            pending = (point, name[:-len(", First>")], fields)
        else:
            require(not name.endswith(", Last>"), "orphan UnicodeData Last")
            paint(result, point, point, category, occupied)
    require(pending is None, "unterminated UnicodeData First")
    require(previous >= 0, "empty UnicodeData")
    return result


def membership(point: int, category: str) -> int:
    if 0xD800 <= point <= 0xDFFF:
        return 0
    if point in (0x24, 0x5F) or category in START:
        return 3
    if point in (0x200C, 0x200D) or category in CONTINUE_ONLY:
        return 2
    return 0


def coalesced_ranges(categories: bytearray) -> list[tuple[int, int, int]]:
    ranges = []
    for point in range(0x80, CODEPOINTS):
        mask = membership(point, CATEGORIES[categories[point]])
        if mask == 0:
            continue
        if ranges and ranges[-1][1] + 1 == point and ranges[-1][2] == mask:
            first, _, _ = ranges[-1]
            ranges[-1] = (first, point, mask)
        else:
            ranges.append((point, point, mask))
    require(0 < len(ranges) <= MAX_RANGES, "range cap exceeded; reviewed design revision required")
    require(len(ranges).bit_length() <= MAX_PROBES, "binary lookup probe bound exceeded")
    return ranges


def validate_ranges(ranges: list[tuple[int, int, int]]) -> None:
    require(0 < len(ranges) <= MAX_RANGES, "invalid range count")
    prior = None
    for first, last, mask in ranges:
        require(0x80 <= first <= last < CODEPOINTS and mask in (2, 3),
                "invalid projected scalar range")
        require(last < 0xD800 or first > 0xDFFF, "projected range contains a surrogate")
        if prior is not None:
            require(prior[1] < first, "projected ranges overlap or are unordered")
            require(not (prior[1] + 1 == first and prior[2] == mask),
                    "projected ranges are not maximally coalesced")
        prior = (first, last, mask)


def canonical_ranges(ranges: list[tuple[int, int, int]]) -> bytes:
    validate_ranges(ranges)
    header = (
        f"# Unicode-Version: {VERSION}\n"
        f"# DerivedGeneralCategory-SHA256: {INPUTS['derived_general_category'][1]}\n"
        f"# License-SHA256: {INPUTS['license'][1]}\n"
        "# License: LICENSE-UNICODE.txt\n"
        "# Masks: 3=start+continue; 2=continue-only; ASCII excluded\n"
    )
    return (header + "".join(f"{first:06X}..{last:06X};{mask}\n"
                             for first, last, mask in ranges)).encode("ascii")


def parse_canonical(body: bytes) -> list[tuple[int, int, int]]:
    ranges = []
    for line in body.decode("ascii").splitlines():
        if line.startswith("#"):
            continue
        match = CANONICAL_LINE.fullmatch(line)
        require(match is not None, "malformed canonical range record")
        ranges.append((int(match[1], 16), int(match[2], 16), int(match[3])))
    validate_ranges(ranges)
    require(canonical_ranges(ranges) == body, "noncanonical range bytes")
    return ranges


def lookup(point: int, ranges: list[tuple[int, int, int]]) -> tuple[int, int]:
    if not (0 <= point < CODEPOINTS) or 0xD800 <= point <= 0xDFFF:
        return 0, 0
    if point < 0x80:
        start = point in (0x24, 0x5F) or 0x41 <= point <= 0x5A or 0x61 <= point <= 0x7A
        return (3 if start else 2 if 0x30 <= point <= 0x39 else 0), 0
    low, high, probes = 0, len(ranges), 0
    while low < high:
        probes += 1
        middle = low + (high - low) // 2
        first, last, mask = ranges[middle]
        if point < first:
            high = middle
        elif point > last:
            low = middle + 1
        else:
            return mask, probes
    return 0, probes


def verify_all_codepoints(derived: bytearray, independent: bytearray,
                          ranges: list[tuple[int, int, int]]) -> int:
    """No host Unicode APIs: compare both UCD formats and canonical lookup."""
    worst = 0
    for point in range(CODEPOINTS):
        require(derived[point] == independent[point],
                f"independent UCD category differs at U+{point:06X}")
        expected = membership(point, CATEGORIES[independent[point]])
        actual, probes = lookup(point, ranges)
        require(expected == actual, f"canonical membership differs at U+{point:06X}")
        require(probes <= len(ranges).bit_length() <= MAX_PROBES,
                "lookup exceeded proven probe bound")
        worst = max(worst, probes)
    for point in (-1, CODEPOINTS, 0x7FFFFFFF):
        require(lookup(point, ranges) == (0, 0), "invalid integer scalar admitted")
    return worst


def source_header(license_body: bytes, canonical: bytes) -> str:
    notice = license_body.decode("utf-8")
    require("\x00" not in notice and "\r" not in notice, "unsafe notice bytes")
    return (
        "// Generated by scripts/generate_json5_identifiers.py; do not edit.\n"
        f"// Unicode {VERSION}; canonical SHA256 {sha256(canonical)}\n"
        + "".join(("// " + line if line else "//") + "\n" for line in notice.splitlines())
        + "\n"
    )


def rust_leaf(ranges: list[tuple[int, int, int]], canonical: bytes,
              license_body: bytes) -> bytes:
    rows = "".join(f"    (0x{first:06X}, 0x{last:06X}, {mask}),\n"
                   for first, last, mask in ranges)
    source = source_header(license_body, canonical) + (
        f"// Maximum interval probes: {len(ranges).bit_length()}\n\n"
        "const RANGES: &[(u32, u32, u8)] = &[\n" + rows + "];\n\n"
        "pub(super) fn is_identifier_start(value: char) -> bool {\n"
        "    let scalar = u32::from(value);\n"
        "    if scalar < 0x80 {\n"
        "        return matches!(scalar, 0x24 | 0x5F | 0x41..=0x5A | 0x61..=0x7A);\n"
        "    }\n"
        "    lookup_mask(scalar) & 1 != 0\n"
        "}\n\n"
        "pub(super) fn is_identifier_continue(value: char) -> bool {\n"
        "    let scalar = u32::from(value);\n"
        "    if scalar < 0x80 {\n"
        "        return matches!(\n"
        "            scalar,\n"
        "            0x24 | 0x5F | 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A\n"
        "        );\n"
        "    }\n"
        "    lookup_mask(scalar) & 2 != 0\n"
        "}\n\n"
        "fn lookup_mask(scalar: u32) -> u8 {\n"
        "    let mut low = 0;\n"
        "    let mut high = RANGES.len();\n"
        "    while low < high {\n"
        "        let middle = low + (high - low) / 2;\n"
        "        let (first, last, mask) = RANGES[middle];\n"
        "        if scalar < first {\n"
        "            high = middle;\n"
        "        } else if scalar > last {\n"
        "            low = middle + 1;\n"
        "        } else {\n"
        "            return mask;\n"
        "        }\n"
        "    }\n"
        "    0\n"
        "}\n"
    )
    return source.encode("utf-8")


def csharp_leaf(ranges: list[tuple[int, int, int]], canonical: bytes,
                license_body: bytes) -> bytes:
    rows = "".join(f"        new Range(0x{first:06X}, 0x{last:06X}, {mask}),\n"
                   for first, last, mask in ranges)
    source = source_header(license_body, canonical) + (
        "namespace Ferrule.Runtime;\n\n"
        "internal static class Json5Identifiers\n{\n"
        f"    // Maximum interval probes: {len(ranges).bit_length()}\n\n"
        "    private readonly struct Range\n    {\n"
        "        internal readonly int First;\n"
        "        internal readonly int Last;\n"
        "        internal readonly byte Mask;\n"
        "        internal Range(int first, int last, byte mask)\n"
        "        { First = first; Last = last; Mask = mask; }\n"
        "    }\n\n"
        "    private static readonly Range[] Ranges =\n    {\n" + rows + "    };\n\n"
        "    internal static bool IsStart(int scalar) => (Mask(scalar) & 1) != 0;\n"
        "    internal static bool IsContinue(int scalar) => (Mask(scalar) & 2) != 0;\n\n"
        "    private static byte Mask(int scalar)\n    {\n"
        "        if (scalar < 0 || scalar > 0x10FFFF || scalar is >= 0xD800 and <= 0xDFFF)\n"
        "        { return 0; }\n"
        "        if (scalar < 0x80)\n        {\n"
        "            if (scalar is 0x24 or 0x5F or >= 0x41 and <= 0x5A or >= 0x61 and <= 0x7A)\n"
        "            { return 3; }\n"
        "            return scalar is >= 0x30 and <= 0x39 ? (byte)2 : (byte)0;\n"
        "        }\n"
        "        var low = 0;\n"
        "        var high = Ranges.Length;\n"
        "        while (low < high)\n        {\n"
        "            var middle = low + (high - low) / 2;\n"
        "            var range = Ranges[middle];\n"
        "            if (scalar < range.First) { high = middle; }\n"
        "            else if (scalar > range.Last) { low = middle + 1; }\n"
        "            else { return range.Mask; }\n"
        "        }\n"
        "        return 0;\n"
        "    }\n"
        "}\n"
    )
    return source.encode("utf-8")


def generate(bodies: dict[str, bytes]) -> dict[str, bytes]:
    derived = derived_categories(bodies["derived_general_category"])
    independent = unicode_data_categories(bodies["unicode_data"])
    projected = coalesced_ranges(derived)
    canonical = canonical_ranges(projected)
    ranges = parse_canonical(canonical)
    require(ranges == projected, "canonical roundtrip differs")
    worst = verify_all_codepoints(derived, independent, ranges)
    outputs = {
        "identifier_ranges.txt": canonical,
        "identifiers.rs": rust_leaf(ranges, canonical, bodies["license"]),
        "Json5Identifiers.cs": csharp_leaf(ranges, canonical, bodies["license"]),
        "LICENSE-UNICODE.txt": bodies["license"],
    }
    report = {
        "unicode_version": VERSION,
        "input_pins": {role: {"bytes": length, "sha256": digest}
                       for role, (length, digest) in sorted(INPUTS.items())},
        "canonical_sha256": sha256(canonical),
        "range_count": len(ranges),
        "range_cap": MAX_RANGES,
        "proven_probe_bound": len(ranges).bit_length(),
        "observed_all_codepoint_worst_probes": worst,
        "category_and_lookup_codepoints_compared": CODEPOINTS,
        "valid_scalars_compared": CODEPOINTS - 0x800,
        "language_source_only_not_compiled_runtime_equivalence": True,
        "outputs": {name: {"bytes": len(body), "sha256": sha256(body)}
                    for name, body in sorted(outputs.items())},
    }
    outputs["generation.json"] = (json.dumps(report, indent=2, sort_keys=True) + "\n").encode("ascii")
    require(all(len(body) <= MAX_OUTPUT_BYTES for body in outputs.values()),
            "generated file exceeds fixed output bound")
    return outputs


def write_fresh(root: Path, outputs: dict[str, bytes]) -> None:
    require(root.is_absolute() and root.parent.is_dir(), "output needs an existing absolute parent")
    root.mkdir(mode=0o700, exist_ok=False)
    for name, body in sorted(outputs.items()):
        path = root / name
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            with os.fdopen(fd, "wb", closefd=False) as handle:
                handle.write(body)
                handle.flush()
                os.fsync(fd)
            os.fchmod(fd, 0o444)
            os.fsync(fd)
        finally:
            os.close(fd)
        require(read_regular(path, MAX_OUTPUT_BYTES) == body, f"retained output differs: {name}")
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def check_existing(root: Path, outputs: dict[str, bytes]) -> None:
    require(root.is_absolute() and root.is_dir() and not root.is_symlink(),
            "check output root is not a regular directory")
    require({p.name for p in root.iterdir()} == set(outputs), "output file set differs")
    for name, body in sorted(outputs.items()):
        require(read_regular(root / name, MAX_OUTPUT_BYTES) == body,
                f"regeneration differs: {name}")


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
