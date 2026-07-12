use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::{AggregateOp, Node};

struct TempDir(PathBuf);

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_output_parameter_{}_{}",
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

fn output(name: &str, input: &str, datatype: &str) -> String {
    format!(
        r#"<component name="{name}" library="core" kind="7">
          <sources><datapoint pos="0"{input}/></sources>
          <data><output datatype="{datatype}"/><parameter usageKind="output" name="{name}"/></data>
        </component>"#
    )
}

fn mapping(outputs: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
        <component name="source" library="xml" kind="14"><data>
          <root><entry name="Source"><entry name="Item"><entry name="Quantity" outkey="1"/><entry name="Price" outkey="2"/></entry></entry></root>
          <document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/>
        </data></component>
        <component name="multiply" library="core" kind="5">
          <sources><datapoint pos="0" key="3"/><datapoint pos="1" key="4"/></sources>
          <targets><datapoint pos="0" key="5"/></targets>
        </component>
        <component name="sum" library="core" kind="5">
          <sources><datapoint/><datapoint pos="1" key="6"/></sources>
          <targets><datapoint pos="0" key="10"/></targets>
        </component>
        {outputs}
      </children><graph><vertices>
        <vertex vertexkey="1"><edges><edge vertexkey="3"/></edges></vertex>
        <vertex vertexkey="2"><edges><edge vertexkey="4"/></edges></vertex>
        <vertex vertexkey="5"><edges><edge vertexkey="6"/></edges></vertex>
        <vertex vertexkey="10"><edges><edge vertexkey="11"/><edge vertexkey="12"/></edges></vertex>
      </vertices></graph></structure></component></mapping>"#
    )
}

fn setup(design: &str) -> TempDir {
    let dir = TempDir::new();
    std::fs::write(
        dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Quantity" type="xs:integer"/>
              <xs:element name="Price" type="xs:integer"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(dir.0.join("mapping.mfd"), design).unwrap();
    dir
}

fn item(quantity: i64, price: i64) -> Instance {
    Instance::Group(vec![
        ("Quantity".into(), Instance::Scalar(Value::Int(quantity))),
        ("Price".into(), Instance::Scalar(Value::Int(price))),
    ])
}

#[test]
fn connected_output_parameter_becomes_an_executable_typed_target() {
    let design = mapping(&output("total", " key=\"11\"", "integer"));
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target.name, "Outputs");
    assert!(imported.project.target_path.is_none());
    assert!(matches!(
        imported
            .project
            .target
            .child("total")
            .map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    let binding = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "total")
        .unwrap();
    let Node::Aggregate {
        function,
        expression: Some(expression),
        ..
    } = &imported.project.graph.nodes[&binding.node]
    else {
        panic!("output should bind to a computed aggregate");
    };
    assert_eq!(*function, AggregateOp::Sum);
    assert!(matches!(
        imported.project.graph.nodes.get(expression),
        Some(Node::Call { function, .. }) if function == "multiply"
    ));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![item(2, 5), item(4, 3)]),
    )]);
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        target.field("total").and_then(Instance::as_scalar),
        Some(&Value::Int(22))
    );

    let exported_path = dir.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let reimported = mfd::import(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let target = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(
        target.field("total").and_then(Instance::as_scalar),
        Some(&Value::Int(22))
    );
}

#[test]
fn duplicate_output_name_keeps_the_first_connected_parameter() {
    let outputs = format!(
        "{}{}",
        output("total", " key=\"11\"", "integer"),
        output("total", " key=\"12\"", "integer")
    );
    let dir = setup(&mapping(&outputs));
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(matches!(
        &imported.project.target.kind,
        SchemaKind::Group { children, .. } if children.len() == 1
    ));
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("duplicate output parameter `total`"));
}

#[test]
fn invalid_output_parameters_do_not_bypass_the_missing_target_error() {
    let cases = [
        output("total", " key=\"13\"", "integer"),
        output("total", " key=\"11\"", "date"),
        output("total", "", "integer"),
    ];
    for design in cases.map(|output| mapping(&output)) {
        let dir = setup(&design);
        assert!(matches!(
            mfd::import(&dir.0.join("mapping.mfd")),
            Err(mfd::MfdError::Unsupported(message))
                if message.contains("output parameters were present but none")
        ));
    }

    let dir = setup(&mapping(""));
    assert!(matches!(
        mfd::import(&dir.0.join("mapping.mfd")),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("no importable target component")
                && !message.contains("output parameters were present")
    ));
}
