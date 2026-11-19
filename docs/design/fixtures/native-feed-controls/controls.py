"""Materialize standalone synthetic reference inputs into a new directory.

    python controls.py NEW_DIRECTORY

The oracles are literals chosen before native preview. No Ferrule export,
vendor-generated XML, controller or installed application is needed.
"""

from pathlib import Path
import sys
from xml.etree import ElementTree as ET


def sub(parent, tag, **attrs):
    return ET.SubElement(parent, tag, attrs)


def write_xml(path, root):
    ET.indent(root)
    ET.ElementTree(root).write(path, encoding="utf-8", xml_declaration=True)


def schema(path, root_name, fields):
    x = ET.Element("xs:schema", {"xmlns:xs": "http://www.w3.org/2001/XMLSchema"})
    seq = sub(sub(sub(x, "xs:element", name=root_name), "xs:complexType"), "xs:sequence")
    for name, dtype, repeat in fields:
        sub(seq, "xs:element", name=name, type="xs:" + dtype, maxOccurs=repeat)
    write_xml(path, x)


def design(d, is_range, cells, edges, components=()):
    m = ET.Element("mapping", version="22")
    sub(m, "resources")
    owner = sub(m, "component", name="Observe", uid="1", editable="1")
    sub(owner, "properties", SelectedLanguage="builtin")
    s = sub(owner, "structure")
    children = sub(s, "children")
    for n, root_name, entries in [
        ("2", "Seed", [("Ticket", "18")] if is_range else [("East", "18"), ("West", "19")]),
        ("3", "Result", cells),
    ]:
        source = n == "2"
        c = sub(children, "component", name=root_name, library="xml", kind="14", uid=n)
        if not source:
            sub(c, "properties", XSLTDefaultOutput="1", AddSchemaLocation="0")
        data = sub(c, "data")
        r = sub(data, "root")
        ns = sub(sub(r, "header"), "namespaces")
        sub(ns, "namespace")
        sub(ns, "namespace", uid="http://www.altova.com/mapforce")
        doc = sub(sub(r, "entry", name="FileInstance", ns="1"), "entry", name="document", ns="1")
        entry = sub(doc, "entry", name=root_name, **({"outkey": "12"} if source else {"inpkey": "22"}))
        for i, (field, key) in enumerate(entries):
            attrs = {"outkey" if source else "inpkey": key}
            if not source and not is_range and i == 1:
                attrs["clone"] = "1"
            sub(entry, "entry", name=field, **attrs)
        sub(data, "document", schema="seed.xsd" if source else "result.xsd",
            instanceroot="{}" + root_name,
            **({"inputinstance": "input.xml"} if source else {"outputinstance": "output.xml"}))
    for uid, name, value in components:
        c = sub(children, "component", name=name, library="core", uid=uid,
                kind="5" if value is None else "2")
        if value is None:
            pins = sub(c, "sources")
            sub(pins, "datapoint", pos="0", key="41")
            sub(pins, "datapoint", pos="1", key="42")
        sub(sub(c, "targets"), "datapoint", pos="0", key=uid + "0")
        if value is not None:
            sub(sub(c, "data"), "constant", datatype="decimal", value=value)
    g = sub(s, "graph", directed="1")
    sub(g, "edges")
    vertices = sub(g, "vertices")
    for a, b in [("12", "22"), *edges]:
        v = sub(vertices, "vertex", vertexkey=a)
        sub(sub(v, "edges"), "edge", vertexkey=b)
    write_xml(d / "probe.mfd", m)


def freeze(root):
    root.mkdir()
    for lower in ("explicit", "omitted"):
        d = root / lower
        d.mkdir()
        schema(d / "seed.xsd", "Seed", [("Ticket", "string", "1")])
        schema(d / "result.xsd", "Result", [("Mark", "integer", "unbounded")])
        (d / "input.xml").write_text("<Seed><Ticket>ink</Ticket></Seed>")
        components = [("4", "generate-sequence", None), ("5", "constant", "3")]
        wires = [("50", "42"), ("40", "29")]
        if lower == "explicit":
            components.append(("6", "constant", "1"))
            wires.append(("60", "41"))
        design(d, True, [("Mark", "29")], wires, components)
        (d / "expected.xml").write_text("<Result><Mark>1</Mark><Mark>2</Mark><Mark>3</Mark></Result>")

    for name, a, b, reverse, failure in [
        ("two-values", "7", "31", False, None),
        ("east-bad", "ochre", "31", False, "ochre"),
        ("west-bad", "7", "indigo", False, "indigo"),
        ("both-bad", "ochre", "indigo", False, "ochre"),
        ("both-bad-reversed", "ochre", "indigo", True, "indigo"),
    ]:
        d = root / name
        d.mkdir()
        schema(d / "seed.xsd", "Seed", [("East", "string", "1"), ("West", "string", "1")])
        schema(d / "result.xsd", "Result", [("Reading", "double", "1")])
        (d / "input.xml").write_text(f"<Seed><East>{a}</East><West>{b}</West></Seed>")
        cells = [("Reading", "29"), ("Reading", "27")]
        design(d, False, cells[::-1] if reverse else cells, [("18", "29"), ("19", "27")])
        if failure is None:
            (d / "expected.xml").write_text("<Result><Reading>7</Reading><Reading>31</Reading></Result>")
        else:
            (d / "expected-error.txt").write_text(f"Conversion to double failed for '{failure}'")


if __name__ == "__main__":
    freeze(Path(sys.argv[1]))
