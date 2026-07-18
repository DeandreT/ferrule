use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xsi_nil_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="State" type="xs:string" nillable="true" minOccurs="0"/><xs:element name="Detail" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="NilValue" type="xs:string" nillable="true"/><xs:element name="Detail" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element><xs:element name="NilRow" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Name" outkey="11"/><entry name="State" outkey="12"/><entry name="Detail" outkey="13"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="set-xsi-nil" library="core" kind="5"><targets><datapoint pos="0" key="20"/></targets></component>
          <component name="is-xsi-nil" library="core" kind="5"><sources><datapoint pos="0" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
          <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="23"/><datapoint pos="1" key="24"/></sources><targets><datapoint pos="0" key="25"/><datapoint pos="1"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Item" inpkey="30"><entry name="NilValue" inpkey="31"/><entry name="Detail" inpkey="34"/></entry><entry name="NilRow" inpkey="32"><entry name="Name" inpkey="33"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="30"/><edge vertexkey="23"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="33"/></edges></vertex>
          <vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
          <vertex vertexkey="13"><edges><edge vertexkey="34" edgekey="90"/></edges></vertex>
          <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
          <vertex vertexkey="22"><edges><edge vertexkey="24"/></edges></vertex>
          <vertex vertexkey="25"><edges><edge vertexkey="32"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    );
    dir
}

fn run(project: &mapping::Project) -> Instance {
    let source = format_xml::from_str(
        r#"<Source xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Row><Name>nil</Name><State xsi:nil="true"/><Detail><Code>A</Code></Detail></Row><Row><Name>empty</Name><State/><Detail><Code>B</Code></Detail></Row><Row><Name>absent</Name><Detail><Code>C</Code></Detail></Row></Source>"#,
        &project.source,
    )
    .unwrap();
    engine::run(project, &source).unwrap()
}

fn assert_output(project: &mapping::Project, output: &Instance) {
    let items = output
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|item| {
        item.field("NilValue")
            .and_then(Instance::as_scalar)
            .is_some_and(Value::is_xml_nil)
    }));
    for (item, code) in items.iter().zip(["A", "B", "C"]) {
        let details = item
            .field("Detail")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(details.len(), 1);
        assert_eq!(
            details[0].field("Code").and_then(Instance::as_scalar),
            Some(&Value::String(code.into()))
        );
    }
    let nil_rows = output
        .field("NilRow")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(nil_rows.len(), 1);
    assert_eq!(
        nil_rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("nil".into()))
    );
    let xml = format_xml::to_string(&project.target, output).unwrap();
    assert_eq!(xml.matches("xsi:nil=\"true\"").count(), 3);
}

#[test]
fn xsi_nil_functions_import_execute_and_round_trip() {
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let output = run(&imported.project);
    assert_output(&imported.project, &output);

    let exported = dir.0.join("round-trip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_output(&reimported.project, &run(&reimported.project));
}
