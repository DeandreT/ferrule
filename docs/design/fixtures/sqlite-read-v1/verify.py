#!/usr/bin/env python3
"""Check SQLite premises for a proposal, without implementing a Ferrule API."""
import json
import math
from pathlib import Path
import sqlite3
import tempfile


def tagged(value, kind):
    if value is None:
        return ["null"]
    if kind == "bool":
        if type(value) is not int or value not in (0, 1):
            raise ValueError("not a SQLite Boolean")
        return [kind, bool(value)]
    if kind == "int" and type(value) is int:
        return [kind, value]
    if kind == "string" and type(value) is str:
        return [kind, value]
    if kind == "float" and type(value) in (int, float):
        converted = float(value)
        if math.isfinite(converted) and (type(value) is float or int(converted) == value):
            return [kind, converted]
    raise ValueError(f"unexpected cell tag for {kind}")


def verify():
    here = Path(__file__).resolve().parent
    cases = json.loads((here / "sql-cases.json").read_text())
    outcomes = []
    with tempfile.TemporaryDirectory(prefix="ferrule-query-design-") as scratch:
        path = Path(scratch) / "fixture.db"
        writer = sqlite3.connect(path, isolation_level=None)
        reader = None
        try:
            writer.executescript((here / "seed.sql").read_text())
            writer.execute("PRAGMA journal_mode=WAL")
            reader = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, isolation_level=None)
            for case in cases:
                rows = reader.execute(case["sql"], case["bindings"]).fetchall()
                outcomes.append({"id": case["id"], "rows": [
                    [tagged(cell, kind) for cell, kind in zip(row, case["types"], strict=True)]
                    for row in rows]})
            rows = reader.execute(
                "SELECT id,label,score,active,ratio FROM orders "
                "WHERE score >= :minimum AND label COLLATE BINARY = :label "
                "ORDER BY score DESC NULLS LAST,id ASC NULLS LAST",
                {"minimum": 10, "label": "alpha"}).fetchall()
            fields = [("id", "int"), ("label", "string"), ("score", "int"),
                      ("active", "bool"), ("ratio", "float")]
            outcomes.append({"id": "complete_profile", "instance": {"repeated": [
                {"group": [[name, tagged(cell, kind)]
                           for cell, (name, kind) in zip(row, fields, strict=True)]}
                for row in rows]}})
            try:
                reader.execute("INSERT INTO orders VALUES (60,'forbidden',0,0,0.0)")
                refusal = {"accepted": True}
            except sqlite3.DatabaseError as error:
                refusal = {"accepted": False, "sqlite_errorname": error.sqlite_errorname,
                           "sqlite_errorcode": error.sqlite_errorcode, "message": str(error)}
            outcomes.append({"id": "read_only_insert", **refusal})
            reader.execute("BEGIN")
            before = reader.execute("SELECT id FROM orders ORDER BY id").fetchall()
            writer.execute("INSERT INTO orders VALUES (60,'committed',0,0,0.0)")
            during = reader.execute("SELECT id FROM orders ORDER BY id").fetchall()
            reader.execute("ROLLBACK")
            after = reader.execute("SELECT id FROM orders ORDER BY id").fetchall()
            outcomes.append({"id": "snapshot", "before": before, "during": during, "after": after})
        finally:
            if reader is not None:
                reader.close()
            writer.close()
    # Complete observations are printed before any semantic assertion.
    print(json.dumps({"sqlite_version": sqlite3.sqlite_version,
                      "scope": "SQLite premises only; no Ferrule runtime qualification",
                      "outcomes": outcomes}, indent=2), flush=True)
    for case, outcome in zip(cases, outcomes[:len(cases)], strict=True):
        assert outcome["rows"] == case["expected"], case["id"]
    assert outcomes[len(cases)]["instance"] == json.loads((here / "profile-output.json").read_text())
    assert outcomes[-2]["accepted"] is False
    assert outcomes[-2]["sqlite_errorname"] == "SQLITE_READONLY"
    assert outcomes[-1] == {"id": "snapshot", "before": [(10,), (20,), (30,), (40,), (50,)],
                           "during": [(10,), (20,), (30,), (40,), (50,)],
                           "after": [(10,), (20,), (30,), (40,), (50,), (60,)]}
    print("PASS: eight complete typed SQL results, read-only refusal and WAL snapshot")


if __name__ == "__main__":
    verify()
