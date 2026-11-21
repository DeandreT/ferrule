"""Check a saved FlexText string parser through the CLI interpreter."""

from pathlib import Path
import shutil
import xml.etree.ElementTree as ET


DESIGN = "flextext-string-parser.mfd"
SCHEMA = "flextext-string-parser-target.xsd"
FILES = (DESIGN, "flextext-string-parser-source.xsd", SCHEMA, "flextext/string-parser.mft")
XSI = "{http://www.w3.org/2001/XMLSchema-instance}noNamespaceSchemaLocation"
REFUSAL = "Error: `flextext_parse_field` input does not match the FlexText layout"


def expected_tree(rows):
    result = ET.Element("Output", {XSI: SCHEMA})
    for name, count in rows:
        row = ET.SubElement(result, "Row")
        ET.SubElement(row, "Name").text = name
        ET.SubElement(row, "Count").text = str(count)
    return result


def structure(element):
    def value(text, is_leaf):
        return (text or "") if is_leaf else (text or "").strip()

    return (
        element.tag, element.attrib, value(element.text, len(element) == 0),
        (element.tail or "").strip(), [structure(child) for child in element],
    )


def check(cli, fixtures):
    for leaf in FILES:
        path = Path(leaf)
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(fixtures / leaf, path)
    imported = cli.run(
        "import-mfd", "--mfd", DESIGN, "--out", "flextext.json",
        "--package-root", ".", "--require-executable",
    )
    assert "(0 warning(s))" in imported.stdout and not imported.stderr
    assert Path("flextext.json").is_file()
    cases = (
        ("ordered", ["Ada*#*3", "Grace*#*5"], [("Ada", 3), ("Grace", 5)]),
        ("reversed", ["Grace*#*5", "Ada*#*3"], [("Grace", 5), ("Ada", 3)]),
        ("quoted", ['"Dot*#*Name"*#*10', "Élodie*#*-7"], [("Dot*#*Name", 10), ("Élodie", -7)]),
        ("bad-count", ["Name*#*bad"], None),
        ("missing-separator", ["No delimiter"], None),
        ("later-bad-count", ["Ada*#*3", "Name*#*bad"], None),
        ("embedded-row-boundary", ["Ada*#*3\nGrace*#*5"], [("Ada", 3)]),
    )
    passed = refused = 0
    for name, lines, expected in cases:
        source = Path(f"flextext-{name}.source.xml")
        output = Path(f"flextext-{name}.output.xml")
        root = ET.Element("Source")
        for value in lines:
            ET.SubElement(root, "Line").text = value
        source.write_bytes(ET.tostring(root, encoding="utf-8"))
        assert not output.exists()
        result = cli.run(
            "run", "--project", "flextext.json", "--input", source, "--output", output,
            success=expected is not None,
        )
        if expected is None:
            assert result.returncode == 1 and not result.stdout and not output.exists()
            assert result.stderr.strip() == REFUSAL
            refused += 1
        else:
            assert not result.stderr
            assert structure(ET.parse(output).getroot()) == structure(expected_tree(expected)), name
            passed += 1
    return passed, refused
