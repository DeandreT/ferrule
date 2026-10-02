use super::*;

#[test]
fn generated_xml_text_preserves_carriage_returns_and_matches_interpreter() -> TestResult<()> {
    let plain = SchemaNode::group("Plain", vec![string("id").attribute(), string("Value")]);
    let mut text = string("#text");
    text.text = true;
    let simple = SchemaNode::group("Simple", vec![string("id").attribute(), text]);
    let mut graph = Graph::default();
    let mut bindings = Vec::new();
    let mut fields = Vec::new();
    for (node, name, schema, indent) in [
        (1, "PlainCompact", plain.clone(), false),
        (2, "PlainIndented", plain.clone(), true),
        (3, "SimpleCompact", simple.clone(), false),
        (4, "SimpleIndented", simple.clone(), true),
    ] {
        graph.nodes.insert(
            node,
            Node::XmlSerialize {
                path: vec![schema.name.clone()],
                frame: None,
                schema: Box::new(schema),
                declaration: indent,
                indent,
                namespace: None,
            },
        );
        fields.push(string(name));
        bindings.push(Binding {
            target_field: name.into(),
            node,
        });
    }
    let project = Project {
        source: SchemaNode::group("Source", vec![plain.clone(), simple.clone()]),
        target: SchemaNode::group("Target", fields),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings,
            ..Scope::default()
        },
    };
    assert!(engine::validate(&project).is_empty());
    let mut cases = Vec::new();
    for value in ["a\r\nb\rc\nd\te&<>\"' &#xD;", "\r\r\n😀\t\r"] {
        let input = serde_json::json!({
            "Plain": {"id": value, "Value": value},
            "Simple": {"id": value, "#text": value},
        })
        .to_string();
        let source = format_json::from_str(&input, &project.source)?;
        let output = engine::run(&project, &source)?;
        let Instance::Group(outputs) = &output else {
            panic!("expected XML string outputs");
        };
        // Decode the actual native output as XML before using it as the
        // generated-host oracle; matching a shared lossy writer is insufficient.
        for (name, instance) in outputs {
            let Instance::Scalar(Value::String(xml)) = instance else {
                panic!("expected XML text for {name}");
            };
            let schema = if name.starts_with("Plain") {
                &plain
            } else {
                &simple
            };
            let parsed = format_xml::from_str(xml, schema)?;
            let expected = Instance::Group(
                (vec![
                    ("id".into(), Instance::Scalar(Value::String(value.into()))),
                    (
                        if name.starts_with("Plain") {
                            "Value"
                        } else {
                            "#text"
                        }
                        .into(),
                        Instance::Scalar(Value::String(value.into())),
                    ),
                ])
                .into(),
            );
            assert_eq!(parsed, expected, "{name}: {xml}");
        }
        cases.push(serde_json::json!({
            "input": input,
            "expected_json": format_json::to_string(&project.target, &output)?,
            "exact_output": true,
        }));
    }
    super::json_text_boundaries::run_generated_boundary_cases(&project, &cases, "xml_text")
}
