use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_target_datetime_cast_{}_{}",
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

fn write_design(directory: &Path, cast_mode: bool) -> PathBuf {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Day" type="xs:string"/>
            <xs:element name="Timestamp" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:simpleType name="RecordedAt"><xs:restriction base="xs:dateTime"/></xs:simpleType>
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Received" type="xs:dateTime"/>
            <xs:element name="Existing" type="RecordedAt"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();

    let cast_attribute = if cast_mode {
        r#" casttotargettypemode="cast-in-subtree""#
    } else {
        ""
    };
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        format!(
            r#"<mapping version="31"><component name="map"><structure><children>
              <component name="source" library="xml" kind="14"><data>
                <root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Day" outkey="1"/><entry name="Timestamp" outkey="2"/></entry></entry></entry></root>
                <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/>
              </data></component>
              <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
                <root><entry name="FileInstance"><entry name="document"{cast_attribute}><entry name="Target"><entry name="Received" inpkey="11"/><entry name="Existing" inpkey="12"/></entry></entry></entry></root>
                <document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Target"/>
              </data></component>
            </children><graph><vertices>
              <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
              <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
            </vertices></graph></structure></component></mapping>"#
        ),
    )
    .unwrap();
    design
}

fn source() -> Instance {
    Instance::Group(vec![
        (
            "Day".to_string(),
            Instance::Scalar(Value::String("2031-08-17+05:45".to_string())),
        ),
        (
            "Timestamp".to_string(),
            Instance::Scalar(Value::String("2031-08-17T06:07:08.9Z".to_string())),
        ),
    ])
}

#[test]
fn cast_in_subtree_coerces_connected_datetime_leaves_and_serializes_them() {
    let directory = TempDir::new();
    let design = write_design(&directory.0, true);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    for field in ["Received", "Existing"] {
        let binding = imported
            .project
            .root
            .bindings
            .iter()
            .find(|binding| binding.target_field == field)
            .unwrap();
        assert!(matches!(
            imported.project.graph.nodes.get(&binding.node),
            Some(Node::Call { function, .. }) if function == "coerce_datetime"
        ));
    }

    let output = engine::run(&imported.project, &source()).unwrap();
    assert_eq!(
        output.field("Received").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17T00:00:00+05:45".to_string()))
    );
    assert_eq!(
        output.field("Existing").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17T06:07:08.9Z".to_string()))
    );
    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(xml.contains("<Received>2031-08-17T00:00:00+05:45</Received>"));
    assert!(xml.contains("<Existing>2031-08-17T06:07:08.9Z</Existing>"));

    let exported = directory.0.join("round-trip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let round_trip = engine::run(&reimported.project, &source()).unwrap();
    assert_eq!(round_trip, output);
}

#[test]
fn target_without_cast_mode_preserves_the_connected_lexical_value() {
    let directory = TempDir::new();
    let imported = mfd::import(&write_design(&directory.0, false)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let output = engine::run(&imported.project, &source()).unwrap();
    assert_eq!(
        output.field("Received").and_then(Instance::as_scalar),
        Some(&Value::String("2031-08-17+05:45".to_string()))
    );
}
