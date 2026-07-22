use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_dtd_post_group_filter_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn rows(output: &Instance) -> Vec<(String, String)> {
    output
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            let string = |field| match row.field(field).and_then(Instance::as_scalar) {
                Some(Value::String(value)) => value.clone(),
                value => panic!("expected string field {field}, got {value:?}"),
            };
            (string("Key"), string("Value"))
        })
        .collect()
}

#[test]
fn dtd_generic_members_filter_after_grouping_and_round_trip() {
    let dir = TempDir::new();
    write(
        &dir.0.join("config.dtd"),
        r#"<!ENTITY % value "(text | number)">
<!ELEMENT Root (dict)>
<!ATTLIST Root version CDATA "1">
<!ELEMENT dict (key, %value;)*>
<!ELEMENT key (#PCDATA)>
<!ELEMENT text (#PCDATA)>
<!ELEMENT number (#PCDATA)>"#,
    );
    write(
        &dir.0.join("config.xml"),
        r#"<!DOCTYPE Root SYSTEM "config.dtd"><Root version="2"><dict><key>Color</key><text>blue</text><key>Count</key><number>7</number><key>Shape</key><text>round</text></dict></Root>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Root"><entry name="dict"><entry name="element()" outkey="10"><entry name="LocalName" outkey="11"/><entry name="key" outkey="12"/><entry name="text" outkey="13"/></entry></entry></entry></root><document schema="config.dtd" inputinstance="config.xml" instanceroot="{}Root"/></data></component>
  <component name="exists" library="core" kind="5"><sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets></component>
  <component name="group-starting-with" library="core" kind="5"><sources><datapoint pos="0" key="22"/><datapoint pos="1" key="23"/></sources><targets><datapoint pos="0" key="24"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="25"/></targets><data><constant value="text" datatype="string"/></data></component>
  <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="26"/><datapoint pos="1" key="27"/></sources><targets><datapoint pos="0" key="28"/></targets></component>
  <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="29"/><datapoint pos="1" key="30"/></sources><targets><datapoint pos="0" key="31"/><datapoint/></targets></component>
  <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40"><entry name="Key" inpkey="41"/><entry name="Value" inpkey="42"/></entry></entry></entry></root><text type="csv" outputinstance="result.csv"><settings separator="," quote="&quot;" firstrownames="false"><names root="Text file" block="Rows"><field0 name="Key" type="string"/><field1 name="Value" type="string"/></names></settings></text></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="26"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="20"/><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="23"/></edges></vertex>
  <vertex vertexkey="24"><edges><edge vertexkey="29"/></edges></vertex>
  <vertex vertexkey="25"><edges><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="40"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.root.filter.is_none());
    assert!(imported.project.root.post_group_filter.is_some());

    let input = format_xml::read(&dir.0.join("config.xml"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    assert_eq!(
        rows(&output),
        [
            ("Color".into(), "blue".into()),
            ("Shape".into(), "round".into())
        ]
    );

    let exported = dir.0.join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let roundtrip = mfd::import(&exported).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    let roundtrip_input =
        format_xml::read(&dir.0.join("config.xml"), &roundtrip.project.source).unwrap();
    let roundtrip_output = engine::run(&roundtrip.project, &roundtrip_input).unwrap();
    assert_eq!(rows(&roundtrip_output), rows(&output));
}
