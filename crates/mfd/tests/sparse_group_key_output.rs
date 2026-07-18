use std::path::PathBuf;

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-sparse-group-key-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sparse_group_key_output_keeps_its_positional_input() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Summary"><xs:complexType><xs:sequence><xs:element name="Company" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.0.join("sparse-group-key.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="group-by" library="core" kind="5"><sources><datapoint pos="0" key="8"/><datapoint pos="1" key="15"/></sources><targets><datapoint/><datapoint pos="1" key="23"/></targets></component>
  <component name="people" library="text" kind="16"><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="7"><entry name="Company" outkey="14"/></entry></entry></entry></root><text type="csv" inputinstance="people.csv"><settings separator="," firstrownames="true"><names root="People" block="Rows"><field0 name="Company" type="string"/></names></settings></text></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Summary"><entry name="Company" inpkey="39"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Summary"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="7"><edges><edge vertexkey="8"/></edges></vertex>
  <vertex vertexkey="14"><edges><edge vertexkey="15"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="39"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let binding = &imported.project.root.bindings[0];
    assert!(matches!(
        imported.project.graph.nodes.get(&binding.node),
        Some(Node::SourceField { path, frame: None }) if path == &["Company"]
    ));

    let source = Instance::Repeated(vec![Instance::Group(vec![(
        "Company".into(),
        Instance::Scalar(Value::String("Example Corp".into())),
    )])]);
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Company").and_then(Instance::as_scalar),
        Some(&Value::String("Example Corp".into()))
    );
    Ok(())
}
