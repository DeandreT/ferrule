use ir::{GroupAlternative, Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD};
use mapping::{Graph, Project, Scope, ScopeConstruction};
fn schema(default: &str) -> SchemaNode {
    let mut s = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(
        ["A", "B"]
            .into_iter()
            .map(|name| GroupAlternative {
                name: name.into(),
                members: vec!["Value".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            })
            .collect(),
    )
    .unwrap();
    s.xml_type_alternatives = true;
    s.xml_default_type = Some(default.into());
    s
}
fn project(target: SchemaNode) -> Project {
    Project {
        source: schema("A"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}
#[test]
fn whole_group_copy_retains_explicit_base_and_derived_type_identity() {
    for target_default in ["A", "B"] {
        let p = project(schema(target_default));
        assert!(engine::validate(&p).is_empty());
        for explicit in ["A", "B"] {
            let xml = format!(
                r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{explicit}"><Value>x</Value></Root>"#
            );
            let source = format_xml::from_str(&xml, &p.source).unwrap();
            let copy = engine::run(&p, &source).unwrap();
            assert_eq!(copy, source);
            assert_eq!(
                copy.field(XML_TYPE_FIELD).and_then(Instance::as_scalar),
                Some(&Value::String(explicit.into()))
            );
            let output = format_xml::to_string(&p.target, &copy).unwrap();
            assert!(output.contains(&format!(r#"xsi:type="{explicit}""#)));
        }
    }
    let p = project(schema("A"));
    let source = format_xml::from_str("<Root><Value>x</Value></Root>", &p.source).unwrap();
    assert!(source.field(XML_TYPE_FIELD).is_none());
    let copy = engine::run(&p, &source).unwrap();
    assert_eq!(copy, source);
    assert!(
        !format_xml::to_string(&p.target, &copy)
            .unwrap()
            .contains("xsi:type")
    );
}
#[test]
fn programmatic_and_nested_invalid_defaults_have_schema_diagnostics() {
    let mut p = project(schema("A"));
    p.target.xml_default_type = Some("Missing".into());
    assert!(
        engine::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("XML default-type metadata"))
    );
    let mut invalid = schema("A");
    invalid.xml_default_type = Some("Missing".into());
    p.target = SchemaNode::group("Root", vec![invalid]);
    assert!(
        engine::validate(&p)
            .iter()
            .any(|issue| issue.message.contains("XML default-type metadata"))
    );
}
