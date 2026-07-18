use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedTarget, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_multiple_targets_{}_{}",
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

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

#[test]
fn every_connected_target_imports_and_executes() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("first.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="First"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("second.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Second"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" instanceroot="{}Source"/></data></component>
  <component name="first" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="First"><entry name="Value" inpkey="20"/></entry></root><document schema="first.xsd" outputinstance="first.xml" instanceroot="{}First"/></data></component>
  <component name="second" library="xml" kind="14"><data><root><entry name="Second"><entry name="Value" inpkey="30"/></entry></root><document schema="second.xsd" outputinstance="second.xml" instanceroot="{}Second"/></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target.name, "First");
    assert_eq!(imported.project.target_path.as_deref(), Some("first.xml"));
    assert_eq!(imported.project.extra_targets.len(), 1);
    let second = &imported.project.extra_targets[0];
    assert_eq!(second.name, "second");
    assert_eq!(second.schema.name, "Second");
    assert_eq!(second.path.as_deref(), Some("second.xml"));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("shared".into())),
    )]);
    let outputs = engine::run_outputs(&imported.project, &source)?;
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(outputs.extras[0].name, "second");
    assert_eq!(
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    Ok(())
}

fn scalar_target(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn two_target_project() -> Project {
    let binding = Binding {
        target_field: "Value".into(),
        node: 0,
    };
    Project {
        source: scalar_target("Source"),
        target: scalar_target("Primary"),
        source_path: Some("source.xml".into()),
        target_path: Some("primary.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "Secondary report".into(),
            path: Some("secondary.xml".into()),
            schema: scalar_target("Secondary"),
            options: FormatOptions::default(),
            root: Scope {
                bindings: vec![binding.clone()],
                ..Scope::default()
            },
        }],
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![binding],
            ..Scope::default()
        },
    }
}

#[test]
fn exports_reimports_and_executes_independent_xml_targets() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let design = dir.0.join("mapping.mfd");
    let project = two_target_project();

    let warnings = mfd::export(&project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design)?;
    assert_eq!(xml.matches("XSLTDefaultOutput=\"1\"").count(), 1);
    assert!(xml.contains("component name=\"Secondary report\""));
    assert!(xml.contains("uid=\"3\""));
    assert!(xml.contains("uid=\"4\""));
    assert!(dir.0.join("mapping-target.xsd").is_file());
    assert!(dir.0.join("mapping-target-2.xsd").is_file());

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.target.name, "Primary");
    let [secondary] = imported.project.extra_targets.as_slice() else {
        return Err("expected one additional target".into());
    };
    assert_eq!(secondary.name, "Secondary report");
    assert_eq!(secondary.schema.name, "Secondary");
    assert_eq!(secondary.path.as_deref(), Some("secondary.xml"));

    let source = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("shared".into())),
    )]);
    let outputs = engine::run_outputs(&imported.project, &source)?;
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(outputs.extras[0].name, "Secondary report");
    assert_eq!(
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    Ok(())
}

#[test]
fn rejects_unretained_additional_edi_boundary_atomically() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "sentinel")?;
    let mut project = two_target_project();
    project.extra_targets[0].path = None;
    project.extra_targets[0].options.lenient_segments = true;

    let error = mfd::export(&project, &design).expect_err("EDI target should be rejected");
    assert!(format!("{error:#}").contains("configuration and dialect are not retained"));
    assert_eq!(std::fs::read_to_string(&design)?, "sentinel");
    assert!(!dir.0.join("mapping-source.xsd").exists());
    assert!(!dir.0.join("mapping-target.xsd").exists());
    Ok(())
}
