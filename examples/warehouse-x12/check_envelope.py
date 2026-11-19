"""Check the single-group, single-transaction envelope used by these examples."""

import argparse
from pathlib import Path


def check(wire: bytes, transaction: str) -> int:
    # ISA has a fixed width: the final two bytes are ISA16 and the terminator.
    if len(wire) < 106 or not wire.startswith(b"ISA"):
        raise ValueError("expected a fixed-width ISA at the beginning of the file")
    element, terminator = wire[3:4], wire[105:106]
    if not wire.endswith(terminator) and not wire.rstrip(b"\r\n").endswith(terminator):
        raise ValueError("final segment is missing its terminator")
    segments = [
        part.lstrip(b"\r\n").split(element)
        for part in wire.split(terminator)
        if part.strip(b"\r\n")
    ]
    tags = [seg[0] for seg in segments]
    required = [b"ISA", b"GS", b"ST", b"SE", b"GE", b"IEA"]
    if tags[:3] != required[:3] or tags[-3:] != required[-3:] or any(
        tags.count(tag) != 1 for tag in required
    ):
        raise ValueError("expected one interchange, one group and one transaction")
    isa, gs, st = segments[:3]
    se, ge, iea = segments[-3:]
    if len(isa) != 17 or len(gs) < 7 or min(map(len, (st, se, ge, iea))) < 3:
        raise ValueError("an envelope segment is missing required elements")
    if st[1] != transaction.encode("ascii"):
        raise ValueError(f"expected ST01={transaction}")
    for label, start, end in (
        ("ISA13 / IEA02", isa[13], iea[2]),
        ("GS06 / GE02", gs[6], ge[2]),
        ("ST02 / SE02", st[2], se[2]),
    ):
        if not start or start != end:
            raise ValueError(f"control numbers differ at {label}")
    count = tags.index(b"SE") - tags.index(b"ST") + 1
    if not se[1].isdigit() or int(se[1]) != count:
        raise ValueError(f"SE01 must count {count} segments including ST and SE")
    if ge[1] != b"1" or iea[1] != b"1":
        raise ValueError("expected GE01=1 and IEA01=1")
    return count


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("transaction", choices=["940", "945"])
    parser.add_argument("file", type=Path)
    args = parser.parse_args()
    try:
        total = check(args.file.read_bytes(), args.transaction)
    except (ValueError, OSError) as err:
        parser.exit(1, f"{err}\n")
    print(f"{args.transaction}: control numbers match; ST through SE contains {total} segments.")
