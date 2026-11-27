use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_LOCAL_NAME_FIELD, XML_TEXT_FIELD,
};
use mapping::{Binding, Graph, Node, Project, Scope};

use test_support::TempDir;

fn source_schema() -> SchemaNode {
    let attributes = SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating();
    let generic = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Score", ScalarType::Int),
            attributes,
        ],
    )
    .repeating();
    SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::scalar("Batch", ScalarType::Int).nillable(),
            SchemaNode::group(
                "Payload",
                vec![SchemaNode::scalar("Kind", ScalarType::String), generic],
            ),
        ],
    )
}

fn project() -> Project {
    Project {
        execution_contract: None,
        source: source_schema(),
        target: SchemaNode::group("Result", vec![SchemaNode::scalar("Batch", ScalarType::Int)]),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Batch".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Batch".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn export_project(directory: &TempDir) -> Result<PathBuf, Box<dyn Error>> {
    let path = directory.join("mapping.mfd");
    let warnings = mfd::export(&project(), &path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    Ok(path)
}

fn assert_execution(project: &Project) -> Result<(), Box<dyn Error>> {
    let source = format_xml::from_str(
        r#"<Envelope><Batch>12</Batch><Payload><Kind>scores</Kind><Entry unit="points">raw<Score>9</Score></Entry></Payload></Envelope>"#,
        &project.source,
    )?;
    let output = engine::run(project, &source)?;
    assert_eq!(
        output.field("Batch").and_then(Instance::as_scalar),
        Some(&Value::Int(12))
    );
    Ok(())
}

#[test]
fn typed_entry_schema_preserves_mixed_fixed_and_generic_children() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("roundtrip")?;
    let path = export_project(&directory)?;
    let mapping = std::fs::read_to_string(&path)?;
    assert!(mapping.contains("ferrule-kind=\"group\""), "{mapping}");
    assert!(mapping.contains("ferrule-repeating=\"1\""), "{mapping}");

    let generated_xsd = directory.join("mapping-source.xsd");
    assert!(matches!(
        format_xml::xsd::import_root(&generated_xsd, Some("Envelope")),
        Err(format_xml::XmlFormatError::UnsupportedXmlWildcard { .. })
    ));

    let imported = mfd::import(&path)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, source_schema());
    assert!(engine::validate(&imported.project).is_empty());
    assert_execution(&imported.project)?;
    Ok(())
}

#[test]
fn wildcard_fallback_does_not_silently_widen_a_declared_root() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("wildcard_name")?;
    let path = directory.join("mapping.mfd");
    let mut project = project();
    project.source = project.source.xml_unqualified();
    mfd::export(&project, &path)?;
    assert!(matches!(
        format_xml::xsd::import_root(&directory.join("mapping-source.xsd"), Some("Envelope")),
        Err(format_xml::XmlFormatError::UnsupportedXmlWildcard { .. })
    ));
    let imported = mfd::import(&path)?;
    assert!(imported.warnings.iter().any(|message| {
        message.contains("xs:any wildcard cannot be represented")
            && message.contains("falling back to the entry tree")
    }));
    assert!(matches!(
        mfd::import_with_profile(&path, &Default::default(), mfd::ImportProfile::Executable),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
    Ok(())
}

#[test]
fn malformed_typed_entry_schema_warns_and_falls_back() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("malformed")?;
    let path = export_project(&directory)?;
    let mapping = std::fs::read_to_string(&path)?.replacen(
        "ferrule-repeating=\"0\"",
        "ferrule-repeating=\"invalid\"",
        1,
    );
    std::fs::write(&path, mapping)?;

    let imported = mfd::import(&path)?;
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| { warning.contains("invalid Ferrule typed entry-schema metadata") })
    );
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("xs:any wildcard cannot be represented")
            && warning.contains("falling back to the entry tree")
    }));
    Ok(())
}
