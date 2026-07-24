use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{ExecutionContext, RuntimeParameters};
use ir::{Instance, ScalarType, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_runtime_parameters_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn write_design(directory: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Dummy" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Echo" type="xs:string"/>
    <xs:element name="Correlation" type="xs:string"/>
    <xs:element name="Control" type="xs:int"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Dummy" outkey="9"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="Correlation input" library="core" kind="6"><targets><datapoint pos="0" key="10"/></targets><data><input datatype="string"/><parameter usageKind="input" name="correlation_id"/></data></component>
  <component name="Control input" library="core" kind="6"><targets><datapoint pos="0" key="11"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="control_number"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Echo" inpkey="19"/><entry name="Correlation" inpkey="20"/><entry name="Control" inpkey="21"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="9"><edges><edge vertexkey="19"/></edges></vertex>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn assert_declarations(project: &mapping::Project) {
    let mut declarations = project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::RuntimeParameter { name, ty } => Some((name.as_str(), *ty)),
            _ => None,
        })
        .collect::<Vec<_>>();
    declarations.sort_unstable_by_key(|(name, _)| *name);
    assert_eq!(
        declarations,
        vec![
            ("control_number", ScalarType::Int),
            ("correlation_id", ScalarType::String),
        ]
    );
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    let source = format_xml::from_str("<Input><Dummy>source</Dummy></Input>", &project.source)?;
    let mut parameters = RuntimeParameters::new();
    parameters.insert("correlation_id", Value::String("txn-23".into()))?;
    parameters.insert("control_number", Value::Int(7001))?;
    let execution = ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
    Ok(engine::run_with_context(project, &source, &execution)?)
}

fn assert_output(output: &Instance) {
    assert_eq!(
        output.field("Echo").and_then(Instance::as_scalar),
        Some(&Value::String("source".into()))
    );
    assert_eq!(
        output.field("Correlation").and_then(Instance::as_scalar),
        Some(&Value::String("txn-23".into()))
    );
    assert_eq!(
        output.field("Control").and_then(Instance::as_scalar),
        Some(&Value::Int(7001))
    );
}

#[test]
fn unconnected_input_parameters_become_typed_host_inputs_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_declarations(&imported.project);
    assert_output(&execute(&imported.project)?);

    let exported_path = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported_path)?.is_empty());
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(exported.contains("name=\"correlation_id\""));
    assert!(exported.contains("name=\"control_number\""));
    assert!(exported.contains("kind=\"6\""));

    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_declarations(&roundtrip.project);
    assert_output(&execute(&roundtrip.project)?);
    Ok(())
}
