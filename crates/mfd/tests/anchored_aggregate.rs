use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{AggregateOp, Node};

struct TempDir(PathBuf);

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_anchored_aggregate_{}_{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
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

fn setup() -> TempDir {
    let directory = TempDir::new();
    std::fs::write(
        directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Office" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Address" maxOccurs="unbounded"><xs:complexType><xs:sequence>
                <xs:element name="line" type="xs:string" maxOccurs="unbounded"/>
              </xs:sequence></xs:complexType></xs:element>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Office" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Location" type="xs:string" minOccurs="0"/>
              <xs:element name="Address" maxOccurs="unbounded"><xs:complexType><xs:sequence>
                <xs:element name="Text" type="xs:string"/>
              </xs:sequence></xs:complexType></xs:element>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data>
            <root><entry name="Source"><entry name="Office" outkey="1"><entry name="Address" outkey="2"><entry name="line" outkey="3"/></entry></entry></entry></root>
            <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
          </data></component>
          <component name="constant" library="core" kind="2">
            <targets><datapoint pos="0" key="4"/></targets>
            <data><constant value="1" datatype="integer"/></data>
          </component>
          <component name="item-at" library="core" kind="5">
            <sources><datapoint pos="0" key="5"/><datapoint pos="1" key="6"/></sources>
            <targets><datapoint pos="0" key="7"/></targets>
          </component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="Target"><entry name="Office" inpkey="11"><entry name="Location" inpkey="13"/><entry name="Address" inpkey="12"><entry name="Text" inpkey="14"/></entry></entry></entry></root>
            <document schema="target.xsd" instanceroot="{}Target"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
          <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
          <vertex vertexkey="3"><edges><edge vertexkey="5"/><edge vertexkey="14"/></edges></vertex>
          <vertex vertexkey="4"><edges><edge vertexkey="6"/></edges></vertex>
          <vertex vertexkey="7"><edges><edge vertexkey="13"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    directory
}

fn address(lines: &[&str]) -> Instance {
    Instance::Group(vec![(
        "line".into(),
        Instance::Repeated(
            lines
                .iter()
                .map(|line| Instance::Scalar(Value::String((*line).into())))
                .collect(),
        ),
    )])
}

fn office(lines: &[&str]) -> Instance {
    Instance::Group(vec![(
        "Address".into(),
        Instance::Repeated(vec![address(lines)]),
    )])
}

#[test]
fn sibling_aggregate_keeps_an_unentered_nested_collection_in_its_path() {
    let directory = setup();
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let office_scope = &imported.project.root.children[0];
    let location = office_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Location")
        .unwrap();
    assert!(matches!(
        imported.project.graph.nodes.get(&location.node),
        Some(Node::Aggregate {
            function: AggregateOp::ItemAt,
            collection,
            value,
            ..
        }) if collection == &["Address", "line"] && value.is_empty()
    ));

    let source = Instance::Group(vec![(
        "Office".into(),
        Instance::Repeated(vec![office(&["US", "street"]), office(&["EU", "road"])]),
    )]);
    let output = engine::run(&imported.project, &source).unwrap();
    let offices = output.field("Office").unwrap().as_repeated().unwrap();
    assert_eq!(
        offices[0].field("Location").and_then(Instance::as_scalar),
        Some(&Value::String("US".into()))
    );
    assert_eq!(
        offices[1].field("Location").and_then(Instance::as_scalar),
        Some(&Value::String("EU".into()))
    );
}
