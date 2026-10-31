use super::*;
use ir::{ScalarType, SchemaNode, XmlNamespace, XmlSchemaHints, XmlSchemaLocation};
use mapping::{Binding, FormatOptions, Graph, Node, Scope};

const SOURCE: &str = "<Source><name>Ada</name><age>37</age></Source>";

fn project(target_namespace: Option<&str>, hints: Option<XmlSchemaHints>) -> Project {
    let fields = || {
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("age", ScalarType::Int),
        ]
    };
    let mut target = SchemaNode::group("Target", fields());
    target.xml_namespace =
        target_namespace.map(|namespace| XmlNamespace::qualified(namespace).unwrap());
    let mut graph = Graph::default();
    for (id, field) in [(0, "name"), (1, "age")] {
        graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![field.into()],
                frame: None,
            },
        );
    }
    Project {
        source: SchemaNode::group("Source", fields()),
        target,
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions {
            xml_document: hints.is_some(),
            xml_schema_hints: hints,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "name".into(),
                    node: 0,
                },
                Binding {
                    target_field: "age".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    }
}

// `.invalid` locations can never resolve; the writer must only emit them.
fn namespace_pair_hints() -> XmlSchemaHints {
    XmlSchemaHints {
        no_namespace_location: None,
        locations: vec![
            XmlSchemaLocation {
                namespace: "urn:order".into(),
                location: "https://schemas.invalid/order.xsd".into(),
            },
            XmlSchemaLocation {
                namespace: "urn:extension".into(),
                location: "extension&v2.xsd".into(),
            },
        ],
        derivation: None,
    }
}

fn no_namespace_hints() -> XmlSchemaHints {
    XmlSchemaHints {
        no_namespace_location: Some("https://schemas.invalid/target.xsd".into()),
        locations: Vec::new(),
        derivation: None,
    }
}

#[test]
fn namespace_pair_hints_reach_the_browser_xml_bytes() {
    let project = project(Some("urn:order"), Some(namespace_pair_hints()));
    let output = run(&project, SOURCE, DataFormat::Xml, DataFormat::Xml).unwrap();
    assert_eq!(
        output,
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<Target xmlns=\"urn:order\" ",
            "xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" ",
            "xsi:schemaLocation=\"urn:order https://schemas.invalid/order.xsd ",
            "urn:extension extension&amp;v2.xsd\">\n",
            "  <name>Ada</name>\n",
            "  <age>37</age>\n",
            "</Target>",
        )
    );
}

#[test]
fn no_namespace_hint_reaches_the_browser_xml_bytes() {
    let project = project(None, Some(no_namespace_hints()));
    let output = run(&project, SOURCE, DataFormat::Xml, DataFormat::Xml).unwrap();
    assert_eq!(
        output,
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<Target xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" ",
            "xsi:noNamespaceSchemaLocation=\"https://schemas.invalid/target.xsd\">\n",
            "  <name>Ada</name>\n",
            "  <age>37</age>\n",
            "</Target>",
        )
    );
}

#[test]
fn projects_without_hints_keep_default_writer_bytes() {
    for target_namespace in [None, Some("urn:order")] {
        let project = project(target_namespace, None);
        let output = run(&project, SOURCE, DataFormat::Xml, DataFormat::Xml).unwrap();
        let root = match target_namespace {
            Some(_) => "<Target xmlns=\"urn:order\">\n",
            None => "<Target>\n",
        };
        assert_eq!(
            output,
            [
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                root,
                "  <name>Ada</name>\n",
                "  <age>37</age>\n",
                "</Target>",
            ]
            .concat()
        );
    }
}

#[test]
fn hints_refuse_non_xml_browser_targets_before_execution() {
    let project = project(None, Some(no_namespace_hints()));
    for format in [DataFormat::Json, DataFormat::Csv] {
        // The unparsable source proves the refusal precedes parsing.
        assert_eq!(
            run(&project, "not a document", DataFormat::Xml, format),
            Err(RuntimeError::XmlSchemaHintsFormatRequired)
        );
        assert_eq!(
            serialize_target(&project, &Instance::Group(Vec::new().into()), format),
            Err(RuntimeError::XmlSchemaHintsFormatRequired)
        );
    }
}

#[test]
fn hints_outside_the_accepted_target_profile_refuse() {
    let mut project = project(None, Some(no_namespace_hints()));
    project.target_options.xml_document = false;
    assert!(matches!(
        run(&project, SOURCE, DataFormat::Xml, DataFormat::Xml),
        Err(RuntimeError::InvalidProject(issues)) if issues.len() == 1
    ));
    let target = Instance::Group(Vec::new().into());
    assert!(matches!(
        serialize_target(&project, &target, DataFormat::Xml),
        Err(RuntimeError::InvalidProject(issues)) if issues.len() == 1
    ));
}

#[test]
fn xml_writer_errors_are_preserved_with_and_without_hints() {
    let plain = project(None, None);
    let repeated = Instance::Repeated(vec![Instance::Group(Vec::new().into())]);
    assert_eq!(
        serialize_target(&plain, &repeated, DataFormat::Xml),
        Err(RuntimeError::Serialize {
            format: DataFormat::Xml,
            message: format_xml::to_string(&plain.target, &repeated)
                .unwrap_err()
                .to_string(),
        })
    );

    let mut colliding = project(None, Some(no_namespace_hints()));
    let mut attribute =
        SchemaNode::scalar("noNamespaceSchemaLocation", ScalarType::String).attribute();
    attribute.xml_namespace =
        Some(XmlNamespace::qualified("http://www.w3.org/2001/XMLSchema-instance").unwrap());
    colliding.target = SchemaNode::group("Target", vec![attribute]);
    let target = Instance::Group(
        vec![(
            "noNamespaceSchemaLocation".to_owned(),
            Instance::Scalar(ir::Value::String("mapped.xsd".into())),
        )]
        .into(),
    );
    match serialize_target(&colliding, &target, DataFormat::Xml) {
        Err(RuntimeError::Serialize { format, message }) => {
            assert_eq!(format, DataFormat::Xml);
            assert_eq!(
                message,
                format_xml::XmlFormatError::SchemaHintCollision(
                    "mapped attribute already supplies the configured schema hint".into()
                )
                .to_string()
            );
        }
        other => panic!("expected a schema-hint collision, got {other:?}"),
    }
}
