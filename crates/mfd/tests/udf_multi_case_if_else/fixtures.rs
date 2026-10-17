use std::path::{Path, PathBuf};

// The feed list describes physical positions, including declared keyless pins.
// First/Second are boolean inputs; A/B/C/D are independently authored literals.
pub(crate) fn fixture(
    root: &Path,
    multi: bool,
    feeds: &[Option<u32>],
    bad: Option<u32>,
) -> PathBuf {
    std::fs::write(
        root.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="First" type="xs:boolean"/><xs:element name="Second" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let marker_schema = if multi {
        r#"<xs:element name="Marker" type="xs:string"/>"#
    } else {
        ""
    };
    std::fs::write(
        root.join("target.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Choice" type="xs:string" minOccurs="0"/>{marker_schema}</xs:sequence></xs:complexType></xs:element></xs:schema>"#
        ),
    )
    .unwrap();
    let marker_port = if multi {
        r#"<entry name="Marker" outkey="23" componentid="103"/>"#
    } else {
        ""
    };
    let marker_target = if multi {
        r#"<entry name="Marker" inpkey="31"/>"#
    } else {
        ""
    };
    let marker_output = if multi {
        r#"<component name="Marker" library="core" uid="103" kind="7"><sources><datapoint pos="0" key="1051"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Marker"/></data></component>"#
    } else {
        ""
    };
    let marker_main_edge = if multi {
        r#"<vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>"#
    } else {
        ""
    };
    let marker_body_edge = if multi {
        r#"<vertex vertexkey="1005"><edges><edge vertexkey="1051"/></edges></vertex>"#
    } else {
        ""
    };
    let mut pins = String::new();
    let mut edges = String::new();
    for (pos, feed) in feeds.iter().enumerate().rev() {
        if let Some(feed) = feed {
            let key = 1020 + pos;
            pins.push_str(&format!(r#"<datapoint pos="{pos}" key="{key}"/>"#));
            let source = if Some(*feed) == bad { 1071 } else { *feed };
            edges.push_str(&format!(
                r#"<vertex vertexkey="{source}"><edges><edge vertexkey="{key}"/></edges></vertex>"#
            ));
        } else {
            pins.push_str(&format!(r#"<datapoint pos="{pos}"/>"#));
        }
    }
    let fallible = if let Some(feed) = bad {
        edges.push_str(&format!(
            r#"<vertex vertexkey="{feed}"><edges><edge vertexkey="1070"/></edges></vertex>"#
        ));
        r#"<component name="format-guid-string" library="lang" uid="125" kind="5"><sources><datapoint pos="0" key="1070"/></sources><targets><datapoint pos="0" key="1071"/></targets></component>"#
    } else {
        ""
    };
    let path = root.join("mapping.mfd");
    std::fs::write(
        &path,
        format!(
            r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="Input" library="xml" uid="1" kind="14"><data><root><entry name="Input"><entry name="First" outkey="10"/><entry name="Second" outkey="11"/></entry></root><document schema="source.xsd" instanceroot="{{}}Input"/></data></component>
    <component name="Choose" library="example" uid="2" kind="19"><data>
      <root><entry name="Second" inpkey="21" componentid="101"/><entry name="First" inpkey="20" componentid="100"/></root>
      <root rootindex="1">{marker_port}<entry name="Choice" outkey="22" componentid="102"/></root>
    </data></component>
    <component name="Output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Choice" inpkey="30"/>{marker_target}</entry></root><document schema="target.xsd" instanceroot="{{}}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>{marker_main_edge}
  </vertices></graph></structure></component>
  <component name="Choose" library="example" inline="1"><structure><children>
    <component name="First" library="core" uid="100" kind="6"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="boolean"/><parameter usageKind="input" name="First"/></data></component>
    <component name="Second" library="core" uid="101" kind="6"><targets><datapoint pos="0" key="1001"/></targets><data><input datatype="boolean"/><parameter usageKind="input" name="Second"/></data></component>
    <component name="constant" library="core" uid="110" kind="2"><targets><datapoint pos="0" key="1002"/></targets><data><constant value="A" datatype="string"/></data></component>
    <component name="constant" library="core" uid="111" kind="2"><targets><datapoint pos="0" key="1003"/></targets><data><constant value="B" datatype="string"/></data></component>
    <component name="constant" library="core" uid="112" kind="2"><targets><datapoint pos="0" key="1004"/></targets><data><constant value="C" datatype="string"/></data></component>
    <component name="constant" library="core" uid="113" kind="2"><targets><datapoint pos="0" key="1005"/></targets><data><constant value="D" datatype="string"/></data></component>
    {fallible}
    <component name="if-else" library="core" uid="120" kind="4"><sources>{pins}</sources><targets><datapoint pos="0" key="1040"/></targets></component>
    <component name="Choice" library="core" uid="102" kind="7"><sources><datapoint pos="0" key="1050"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Choice"/></data></component>{marker_output}
  </children><graph><vertices>{edges}
    <vertex vertexkey="1040"><edges><edge vertexkey="1050"/></edges></vertex>{marker_body_edge}
  </vertices></graph></structure></component>
</mapping>"#
        ),
    )
    .unwrap();
    path
}

#[derive(Clone, Copy)]
pub(crate) enum NestedFailure {
    Late,
    Default,
}

// Independently authored definitions are saved caller-first: ChooseNested ->
// Pick -> Join. Every call binds by componentid, not by declaration/port order.
pub(crate) fn nested_fixture(root: &Path, multi: bool, bad: Option<NestedFailure>) -> PathBuf {
    std::fs::write(
        root.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="First" type="xs:boolean"/><xs:element name="Second" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let (marker_schema, marker_port, marker_target, marker_output, marker_edges) = if multi {
        (
            r#"<xs:element name="Marker" type="xs:string"/>"#,
            r#"<entry name="Marker" outkey="23" componentid="103"/>"#,
            r#"<entry name="Marker" inpkey="31"/>"#,
            r#"<component name="Marker" library="core" uid="103" kind="7"><sources><datapoint pos="0" key="1051"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Marker"/></data></component>"#,
            (
                r#"<vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>"#,
                r#"<vertex vertexkey="1005"><edges><edge vertexkey="1051"/></edges></vertex>"#,
            ),
        )
    } else {
        ("", "", "", "", ("", ""))
    };
    std::fs::write(
        root.join("target.xsd"),
        format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Choice" type="xs:string" minOccurs="0"/>{marker_schema}</xs:sequence></xs:complexType></xs:element></xs:schema>"#
        ),
    )
    .unwrap();
    let (late, default, fallible, fallible_edge) = match bad {
        Some(which) => (
            if matches!(which, NestedFailure::Late) {
                1071
            } else {
                1120
            },
            if matches!(which, NestedFailure::Default) {
                1071
            } else {
                1121
            },
            r#"<component name="format-guid-string" library="lang" uid="125" kind="5"><sources><datapoint pos="0" key="1070"/></sources><targets><datapoint pos="0" key="1071"/></targets></component>"#,
            format!(
                r#"<vertex vertexkey="{}"><edges><edge vertexkey="1070"/></edges></vertex>"#,
                if matches!(which, NestedFailure::Late) {
                    1120
                } else {
                    1121
                }
            ),
        ),
        None => (1120, 1121, "", String::new()),
    };
    let (marker_main_edge, marker_body_edge) = marker_edges;
    let path = root.join("nested.mfd");
    std::fs::write(
        &path,
        format!(
            r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="Input" library="xml" uid="1" kind="14"><data><root><entry name="Input"><entry name="First" outkey="10"/><entry name="Second" outkey="11"/></entry></root><document schema="source.xsd" instanceroot="{{}}Input"/></data></component>
    <component name="ChooseNested" library="example" uid="2" kind="19"><data>
      <root><entry name="First" inpkey="20" componentid="100"/><entry name="Second" inpkey="21" componentid="101"/></root>
      <root rootindex="1">{marker_port}<entry name="Choice" outkey="22" componentid="102"/></root>
    </data></component>
    <component name="Output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Choice" inpkey="30"/>{marker_target}</entry></root><document schema="target.xsd" instanceroot="{{}}Output"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>{marker_main_edge}
  </vertices></graph></structure></component>
  <component name="ChooseNested" library="example"><structure><children>
    <component name="Second" library="core" uid="101" kind="6"><targets><datapoint pos="0" key="1001"/></targets><data><input datatype="boolean"/><parameter usageKind="input" name="Second"/></data></component>
    <component name="First" library="core" uid="100" kind="6"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="boolean"/><parameter usageKind="input" name="First"/></data></component>
    <component name="constant" library="core" uid="110" kind="2"><targets><datapoint pos="0" key="1002"/></targets><data><constant value="A" datatype="string"/></data></component>
    <component name="constant" library="core" uid="111" kind="2"><targets><datapoint pos="0" key="1003"/></targets><data><constant value="B" datatype="string"/></data></component>
    <component name="constant" library="core" uid="112" kind="2"><targets><datapoint pos="0" key="1004"/></targets><data><constant value="C" datatype="string"/></data></component>
    <component name="constant" library="core" uid="113" kind="2"><targets><datapoint pos="0" key="1005"/></targets><data><constant value="D" datatype="string"/></data></component>
    <component name="constant" library="core" uid="114" kind="2"><targets><datapoint pos="0" key="1006"/></targets><data><constant value="!" datatype="string"/></data></component>
    <component name="constant" library="core" uid="115" kind="2"><targets><datapoint pos="0" key="1007"/></targets><data><constant value="?" datatype="string"/></data></component>
    <component name="constant" library="core" uid="116" kind="2"><targets><datapoint pos="0" key="1008"/></targets><data><constant value="LATE-UNUSED" datatype="string"/></data></component>
    <component name="constant" library="core" uid="117" kind="2"><targets><datapoint pos="0" key="1009"/></targets><data><constant value="DEFAULT-UNUSED" datatype="string"/></data></component>
    <component name="Pick" library="example" uid="130" kind="19"><data>
      <root><entry name="Fallback" inpkey="1103" componentid="202"/><entry name="Gate" inpkey="1100" componentid="200"/><entry name="Suffix" inpkey="1102" componentid="203"/><entry name="Preferred" inpkey="1101" componentid="201"/></root>
      <root rootindex="1"><entry name="Picked" outkey="1120" componentid="204"/></root>
    </data></component>
    <component name="Pick" library="example" uid="131" kind="19"><data>
      <root><entry name="Preferred" inpkey="1111" componentid="201"/><entry name="Suffix" inpkey="1112" componentid="203"/><entry name="Fallback" inpkey="1113" componentid="202"/><entry name="Gate" inpkey="1110" componentid="200"/></root>
      <root rootindex="1"><entry name="Picked" outkey="1121" componentid="204"/></root>
    </data></component>
    {fallible}
    <component name="if-else" library="core" uid="120" kind="4"><sources><datapoint pos="4" key="1024"/><datapoint pos="3" key="1023"/><datapoint pos="2" key="1022"/><datapoint pos="1" key="1021"/><datapoint pos="0" key="1020"/></sources><targets><datapoint pos="0" key="1040"/></targets></component>
    <component name="Choice" library="core" uid="102" kind="7"><sources><datapoint pos="0" key="1050"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Choice"/></data></component>{marker_output}
  </children><graph><vertices>
    <vertex vertexkey="1000"><edges><edge vertexkey="1020"/><edge vertexkey="1110"/></edges></vertex>
    <vertex vertexkey="1001"><edges><edge vertexkey="1022"/><edge vertexkey="1100"/></edges></vertex>
    <vertex vertexkey="1002"><edges><edge vertexkey="1021"/></edges></vertex>
    <vertex vertexkey="1003"><edges><edge vertexkey="1101"/></edges></vertex>
    <vertex vertexkey="1004"><edges><edge vertexkey="1113"/></edges></vertex>
    <vertex vertexkey="1006"><edges><edge vertexkey="1102"/></edges></vertex>
    <vertex vertexkey="1007"><edges><edge vertexkey="1112"/></edges></vertex>
    <vertex vertexkey="1008"><edges><edge vertexkey="1103"/></edges></vertex>
    <vertex vertexkey="1009"><edges><edge vertexkey="1111"/></edges></vertex>
    <vertex vertexkey="{late}"><edges><edge vertexkey="1023"/></edges></vertex>
    <vertex vertexkey="{default}"><edges><edge vertexkey="1024"/></edges></vertex>{fallible_edge}
    <vertex vertexkey="1040"><edges><edge vertexkey="1050"/></edges></vertex>{marker_body_edge}
  </vertices></graph></structure></component>
  <component name="Pick" library="example"><structure><children>
    <component name="Suffix" library="core" uid="203" kind="6"><targets><datapoint pos="0" key="2003"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Suffix"/></data></component>
    <component name="Preferred" library="core" uid="201" kind="6"><targets><datapoint pos="0" key="2001"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Preferred"/></data></component>
    <component name="Gate" library="core" uid="200" kind="6"><targets><datapoint pos="0" key="2000"/></targets><data><input datatype="boolean"/><parameter usageKind="input" name="Gate"/></data></component>
    <component name="Fallback" library="core" uid="202" kind="6"><targets><datapoint pos="0" key="2002"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Fallback"/></data></component>
    <component name="Join" library="example" uid="210" kind="19"><data>
      <root><entry name="Head" inpkey="2010" componentid="300"/><entry name="Tail" inpkey="2011" componentid="301"/></root>
      <root rootindex="1"><entry name="Joined" outkey="2030" componentid="302"/></root>
    </data></component>
    <component name="Join" library="example" uid="211" kind="19"><data>
      <root><entry name="Head" inpkey="2020" componentid="300"/><entry name="Tail" inpkey="2021" componentid="301"/></root>
      <root rootindex="1"><entry name="Joined" outkey="2031" componentid="302"/></root>
    </data></component>
    <component name="if-else" library="core" uid="212" kind="4"><sources><datapoint pos="2" key="2042"/><datapoint pos="0" key="2040"/><datapoint pos="1" key="2041"/></sources><targets><datapoint pos="0" key="2050"/></targets></component>
    <component name="Picked" library="core" uid="204" kind="7"><sources><datapoint pos="0" key="2060"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Picked"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="2000"><edges><edge vertexkey="2040"/></edges></vertex>
    <vertex vertexkey="2001"><edges><edge vertexkey="2010"/></edges></vertex>
    <vertex vertexkey="2002"><edges><edge vertexkey="2020"/></edges></vertex>
    <vertex vertexkey="2003"><edges><edge vertexkey="2011"/><edge vertexkey="2021"/></edges></vertex>
    <vertex vertexkey="2030"><edges><edge vertexkey="2041"/></edges></vertex>
    <vertex vertexkey="2031"><edges><edge vertexkey="2042"/></edges></vertex>
    <vertex vertexkey="2050"><edges><edge vertexkey="2060"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="Join" library="example"><structure><children>
    <component name="Tail" library="core" uid="301" kind="6"><targets><datapoint pos="0" key="3001"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Tail"/></data></component>
    <component name="Head" library="core" uid="300" kind="6"><targets><datapoint pos="0" key="3000"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Head"/></data></component>
    <component name="concat" library="core" uid="310" kind="5"><sources><datapoint pos="1" key="3011"/><datapoint pos="0" key="3010"/></sources><targets><datapoint pos="0" key="3020"/></targets></component>
    <component name="Joined" library="core" uid="302" kind="7"><sources><datapoint pos="0" key="3030"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Joined"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="3000"><edges><edge vertexkey="3010"/></edges></vertex>
    <vertex vertexkey="3001"><edges><edge vertexkey="3011"/></edges></vertex>
    <vertex vertexkey="3020"><edges><edge vertexkey="3030"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#
        ),
    )
    .unwrap();
    path
}
