"""Drive the existing XML-to-workbook fixture and reopen the actual OOXML files."""

import json
from pathlib import Path
import shutil

from openpyxl import Workbook, load_workbook
from openpyxl.styles import Font, PatternFill


def template(path, formula_at):
    book = Workbook()
    design = book.active
    design.title = "Design"
    notes = book.create_sheet("Notes")
    book.create_sheet("Research")
    for address, value in {"A2": "Division", "A3": "old", "B5": "Member", "D5": "Rating",
                           "B6": "old A", "D6": 1, "B7": "old B", "D7": 2,
                           "B8": "stale", "D8": 3, "A11": "old footer", "G2": "=D6*2"}.items():
        design[address] = value
    for cell in [design["D6"], design["D7"]]:
        cell.font = Font(bold=True, color="FF73234E")
        cell.fill = PatternFill("solid", fgColor="FFB9DCF0")
        cell.number_format = "0.0000"
    notes["A1"], notes["C4"], notes["E9"] = "Untouched", 42, "=C4+1"
    if formula_at:
        design[formula_at] = "=40+2"
    book.save(path)
    book.close()


def reopen(path):
    book = load_workbook(path, data_only=False)
    try:
        values = {sheet.title: {c.coordinate: c.value for row in sheet for c in row if c.value is not None}
                  for sheet in book}
        def style(c):
            color = c.font.color
            return (c.font.bold, color.rgb if color and color.type == "rgb" else None,
                    c.fill.patternType, c.fill.fgColor.value, c.number_format)
        return values, [style(book["Design"][coord]) for coord in ("D6", "D7")]
    finally:
        book.close()


def check(cli, fixtures):
    source = "xlsx-hierarchical-source.xml"
    for name in [source, "xlsx-hierarchical-source.xsd"]:
        shutil.copyfile(fixtures / name, name)
    original = (fixtures / "xlsx-hierarchical.mfd").read_bytes()
    assert original.count(b"<excel/>") == 1
    fresh = {"Design": {"A2": "Division", "A3": "Design", "B5": "Member", "D5": "Rating",
                        "B6": "Iris", "D6": 7, "B7": "Omar", "D7": 9, "A10": "Prepared internally"},
             "Research": {"A2": "Division", "A3": "Research", "B5": "Member", "D5": "Rating",
                          "B6": "Mina", "D6": 8, "A9": "Prepared internally"}}
    notes = {"A1": "Untouched", "C4": 42, "E9": "=C4+1"}
    updated = {"Design": dict(fresh["Design"], G2="=D6*2"), "Notes": notes, "Research": fresh["Research"]}
    styled = [(True, "FF73234E", "solid", "FFB9DCF0", "0.0000")] * 2
    dest = Path("report.xlsx")
    for mode, flag in [("create", b""), ("update", b' updateexistingfile="1"')]:
        design = Path(mode + ".mfd")
        design.write_bytes(original.replace(b"<excel/>", b'<excel outputinstance="report.xlsx"' + flag + b"/>"))
        project = cli.import_pair(design)
        restored = cli.import_pair(cli.export(project, "native-mfd"))
        for p in [project, restored]:
            options = json.loads(p.read_bytes())["target_options"]
            assert options["xlsx_hierarchical"] and options.get("xlsx_update_existing", False) == bool(flag)
            args = ["run", p, source, dest]
            if flag:
                template(dest, None)
                assert not cli.run(*args).stderr
                values, styles = reopen(dest)
                assert list(values) == list(updated) and values == updated and styles == styled
                template(dest, "D7")
                before, readback = dest.read_bytes(), reopen(dest)
                assert readback[0]["Design"]["D7"] == "=40+2"
                assert "D7 inside the layout holds a formula" in cli.run(*args, success=False).stderr
                assert dest.read_bytes() == before and reopen(dest) == readback
            else:
                for existing in [False, True]:
                    if existing:
                        template(dest, "D7")
                    else:
                        dest.unlink(missing_ok=True)
                    assert not cli.run(*args).stderr
                    values, styles = reopen(dest)
                    assert list(values) == list(fresh) and values == fresh
                    assert all(not s[0] and s[-1] == "General" for s in styles)

    # A flat update is a different writer, not the hierarchical writer with an
    # "ignore" option. Reuse the same flat output setup as mfd/tests/xlsx_export.rs.
    for name in ["people-to-csv.mfd", "people-source.xsd"]:
        shutil.copyfile(fixtures / name, name)
    Path("people.xml").write_text("<Company><Staff><First>Jo</First><Last>Li</Last><Age>7</Age></Staff>"
                                  "<Staff><First>Al</First><Last>Yu</Last><Age>9</Age></Staff></Company>",
                                  encoding="utf-8")
    p = json.loads(cli.import_pair("people-to-csv.mfd").read_bytes())
    p["target_options"] = dict(tabular_kind="xlsx", xlsx_sheet="Design", xlsx_start_row=5, xlsx_columns=[2, 4],
                               xlsx_headers=["Member", "Rating"], has_header_row=True, xlsx_update_existing=True)
    Path("flat.json").write_text(json.dumps(p), encoding="utf-8")
    for address in ["D7", "D8"]:
        template(dest, address)
        expected, _ = reopen(dest)
        expected["Design"].update(B6="Jo Li", D6=7, B7="Al Yu", D7=9)
        del expected["Design"]["B8"]
        if address == "D7":
            del expected["Design"]["D8"]
        result = cli.run("run", "flat.json", "people.xml", dest)
        assert not result.stderr and "warning" not in result.stdout.lower()
        assert reopen(dest) == (expected, styled)  # D7 replaced; D8 clear-only formula survives.
