"""Independent sqlite3 result sets; execute the imported mapping through the CLI."""

import csv
import io
from pathlib import Path
import shutil
import sqlite3
from contextlib import closing
import xml.etree.ElementTree as ET


def check(cli, fixtures):
    for name in ["call.mfd", "direct.mfd", "article.xsd"]:
        shutil.copyfile(fixtures / name, name)
    columns = ["Number", "Name", "SinglePrice"]
    with closing(sqlite3.connect("rows.sqlite")) as db:
        db.executescript("""
            CREATE TABLE Articles(Number INTEGER PRIMARY KEY, Name TEXT, SinglePrice REAL, DepartmentID INTEGER);
            INSERT INTO Articles VALUES (21,'Copper',14.25,2),(10,'Rail',23.5,1),(39,'Tape',31.75,1),
                (11,'Copper',10.25,2),(50,'Crate',75.25,3),(18,'Rivet, blue',16.75,1),(31,'Café',6.5,2);
        """)
        # Independent of the saved SQL and Ferrule's interpretation; use a bound argument.
        expected = {n: list(db.execute("SELECT Number,Name,SinglePrice FROM Articles "
                                       "WHERE DepartmentID=? ORDER BY Number DESC", (n,))) for n in [1, 2, 4]}
        winner, = db.execute("SELECT Number,Name,SinglePrice FROM Articles "
                             "WHERE SinglePrice=(SELECT MAX(SinglePrice) FROM Articles)")
    assert len(expected[1]) == len(expected[2]) == 3 and expected[1] != expected[2] and expected[4] == []

    call = cli.import_pair("call.mfd")
    native = cli.export(call, "native-mfd")
    for project in [call, cli.import_pair(native)]:
        dest = Path("actual.csv")
        args = ["run", project, "rows.sqlite", dest]
        for n, rows in expected.items():
            result = cli.run(*args, "--param", f"DepartmentID={n}")
            assert not result.stderr
            oracle = io.StringIO(newline="")
            csv.writer(oracle, lineterminator="\n").writerows([columns, *rows])
            assert dest.read_bytes() == oracle.getvalue().encode("utf-8"), n
            dest.unlink()
        for flags, message in [([], "does not provide runtime parameter"),
                               (["--param", "DepartmentID=bad"], "expected Int, got string")]:
            result = cli.run(*args, *flags, success=False)
            assert message in result.stderr and "DepartmentID" in result.stderr and not dest.exists()

    direct = cli.import_pair("direct.mfd")
    cli.export(direct, "native-mfd", issue="xml_first_group_presence")
    extension = cli.export(direct, "ferrule-extensions", issue="xml_first_group_presence")
    for project, schema in [(direct, "article.xsd"), (cli.import_pair(extension), "saved-target.xsd")]:
        cli.run("run", project, "rows.sqlite", "actual.xml")
        root = ET.parse("actual.xml").getroot()
        assert root.tag == "Article" and root.attrib == {
            "{http://www.w3.org/2001/XMLSchema-instance}noNamespaceSchemaLocation": schema
        }
        assert [(el.tag, el.text) for el in root] == list(zip(columns, map(str, winner), strict=True))
        assert all(not list(el) and not el.attrib for el in root)
        Path("actual.xml").unlink()

    # SELECT * projects four columns but the MFD declares three result pins.
    tree = ET.parse("direct.mfd")
    tree.find(".//LocalViewElement").set("SQL", "SELECT * FROM Articles ORDER BY SinglePrice DESC LIMIT 1")
    tree.write("wildcard.mfd", encoding="utf-8")
    args = ["import-mfd", "--mfd", "wildcard.mfd", "--out", "incomplete.json"]
    reason = "SQL projection does not match the inline query outputs"
    assert reason in cli.run(*args, "--require-executable", success=False).stderr
    assert not Path("incomplete.json").exists()
    assert reason in cli.run(*args).stderr
    cli.run("run", "incomplete.json", "rows.sqlite", "incomplete.xml")
    root = ET.parse("incomplete.xml").getroot()
    assert root.tag == "Article" and not list(root)
