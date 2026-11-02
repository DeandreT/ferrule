use super::*;
use ir::{GroupAlternative, ScalarType, SchemaNode, Value, XmlNamespace, XmlTypeOrigin};
use mapping::{Binding, FormatOptions, Graph, Node, Scope};

fn project(root_view: bool) -> Project {
    let attribute = |name: &str| {
        let mut field = SchemaNode::scalar(name, ScalarType::String).attribute();
        field.xml_namespace = Some(XmlNamespace::Unqualified);
        field
    };
    let mut source = SchemaNode::group("Envelope", vec![attribute("Code"), attribute("Extra")])
        .with_alternatives(vec![
            GroupAlternative {
                name: "Base".into(),
                members: vec!["Code".into()],
                required: vec![],
                constraints: vec![],
            },
            GroupAlternative {
                name: "Derived".into(),
                members: vec!["Code".into(), "Extra".into()],
                required: vec![],
                constraints: vec![],
            },
        ])
        .unwrap();
    source.xml_namespace = Some(XmlNamespace::Unqualified);
    source.xml_type_alternatives = true;
    source.xml_default_type = Some("Base".into());
    Project {
        source,
        target: SchemaNode::group(
            "Result",
            vec![
                SchemaNode::scalar("Captured", ScalarType::String),
                SchemaNode::scalar("Derived", ScalarType::Bool),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            xml_allow_inactive_root_type_members: true,
            xml_root_view_read_policy: root_view,
            ..Default::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceRootField {
                        path: vec!["Extra".into()],
                        required: true,
                    },
                ),
                (
                    1,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type: "Derived".into(),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Captured".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Derived".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        },
    }
}

fn xml(annotation: &str) -> String {
    format!(
        r#"<Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:p="urn:test" xsi:type="{annotation}" Code="c" Extra="kept 雪"/>"#
    )
}

fn output(project: &Project, text: &str) -> String {
    assert!(engine::validate(project).is_empty());
    run(project, text, DataFormat::Xml, DataFormat::Json).unwrap()
}

fn expected(derived: bool) -> String {
    format!("{{\n  \"Captured\": \"kept 雪\",\n  \"Derived\": {derived}\n}}\n")
}

#[test]
fn inactive_root_members_survive_browser_parse_and_execution() {
    let project = project(false);
    let parsed = parse_source(&project, &xml("Base"), DataFormat::Xml).unwrap();
    assert_eq!(
        parsed.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Base"))
    );
    assert_eq!(
        ir::primary_root_scalar(Some(&parsed), &["Extra"]).unwrap(),
        Value::String("kept 雪".into())
    );
    assert_eq!(output(&project, &xml("Base")), expected(false));
}

#[test]
fn root_view_preserves_literal_annotation_and_exact_type_matching() {
    let project = project(true);
    for (annotation, origin, derived) in [
        ("Derived", XmlTypeOrigin::Explicit("Derived"), true),
        (
            " Derived ",
            XmlTypeOrigin::ExplicitPadded {
                literal: " Derived ",
                resolved_identity: "Derived",
            },
            false,
        ),
        ("Unknown", XmlTypeOrigin::Explicit("Unknown"), false),
        (
            "p:Unknown",
            XmlTypeOrigin::Explicit("{urn:test}Unknown"),
            false,
        ),
    ] {
        let parsed = parse_source(&project, &xml(annotation), DataFormat::Xml).unwrap();
        assert_eq!(parsed.xml_type_origin(), Ok(origin));
        assert_eq!(output(&project, &xml(annotation)), expected(derived));
    }
}

#[test]
fn root_view_nil_and_absence_keep_declared_attribute_values() {
    let project = project(true);
    let nil = xml("Derived").replace(" Code=", " xsi:nil=\"true\" Code=");
    assert_eq!(output(&project, &nil), expected(true));
    let absent = r#"<Envelope Code="c" Extra="kept 雪"/>"#;
    let parsed = parse_source(&project, absent, DataFormat::Xml).unwrap();
    assert_eq!(parsed.xml_type_origin(), Ok(XmlTypeOrigin::Absent));
    assert_eq!(output(&project, absent), expected(false));
}

#[test]
fn missing_required_root_field_remains_an_execution_error() {
    let project = project(true);
    let text = xml("Base").replace(" Extra=\"kept 雪\"", "");
    let error = run(&project, &text, DataFormat::Xml, DataFormat::Json).unwrap_err();
    assert!(
        matches!(error, RuntimeError::Execute(ref message)
        if message.contains("required") && message.contains("Extra")),
        "{error}"
    );
}

#[test]
fn root_read_policy_refuses_non_xml_selection_before_parsing() {
    for (inactive, root_view) in [(true, false), (false, true), (true, true)] {
        let mut project = project(root_view);
        project.source_options.xml_allow_inactive_root_type_members = inactive;
        for format in [DataFormat::Json, DataFormat::Csv, DataFormat::Xbrl] {
            assert!(matches!(
                parse_source(&project, "not a document", format),
                Err(RuntimeError::InvalidProject(_))
            ));
            assert!(run(&project, "not a document", format, DataFormat::Json).is_err());
        }
    }
}

#[test]
fn direct_parser_refuses_unsupported_policy_options_and_schemas() {
    for change in 0..7 {
        let mut project = project(true);
        match change {
            0 => project.source_options.xml_document = false,
            1 => project.source_options.xml_allow_inactive_root_type_members = false,
            2 => project.source_options.json_lines = true,
            3 => project.source.repeating = true,
            4 => project.source.xml_namespace = None,
            5 => {
                let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
                    unreachable!()
                };
                children[0].kind = ir::SchemaKind::Scalar {
                    ty: ScalarType::Int,
                };
            }
            _ => {
                let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
                    unreachable!()
                };
                children[0].fixed = Some("c".into());
            }
        }
        assert!(
            matches!(
                parse_source(&project, &xml("Derived"), DataFormat::Xml),
                Err(RuntimeError::InvalidProject(_))
            ),
            "case {change}"
        );
    }
}

#[test]
fn default_xml_reader_remains_strict_and_valid_reads_are_unchanged() {
    let mut project = project(false);
    project.source_options = FormatOptions::default();
    assert!(matches!(
        parse_source(&project, &xml("Base"), DataFormat::Xml),
        Err(RuntimeError::Parse {
            format: DataFormat::Xml,
            ..
        })
    ));
    assert_eq!(output(&project, &xml("Derived")), expected(true));
}

#[test]
fn malformed_root_annotation_stays_a_parse_error() {
    let project = project(true);
    for annotation in ["missing:Derived", "p::Derived", "De rived"] {
        assert!(matches!(
            parse_source(&project, &xml(annotation), DataFormat::Xml),
            Err(RuntimeError::Parse {
                format: DataFormat::Xml,
                ..
            })
        ));
    }
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>{}",
        xml("Derived")
    );
    assert!(matches!(
        parse_source(&project, &text, DataFormat::Xml),
        Err(RuntimeError::Parse {
            format: DataFormat::Xml,
            ..
        })
    ));
}

#[test]
fn direct_serializer_refuses_source_only_root_policy() {
    for (inactive, root_view) in [(true, false), (false, true), (true, true)] {
        let mut project = project(root_view);
        project.target_options = FormatOptions {
            xml_allow_inactive_root_type_members: inactive,
            xml_root_view_read_policy: root_view,
            ..Default::default()
        };
        let target = Instance::Group(vec![].into());
        for format in [
            DataFormat::Xml,
            DataFormat::Json,
            DataFormat::Csv,
            DataFormat::Xbrl,
        ] {
            assert!(matches!(
                serialize_target(&project, &target, format),
                Err(RuntimeError::InvalidProject(_))
            ));
        }
    }
}

#[test]
fn root_policy_preserves_local_source_hint_restriction() {
    for path in [
        "http://example.test/input.xml",
        "hTtPs://example.test/input.xml",
    ] {
        let mut project = project(true);
        project.source_path = Some(path.into());
        assert!(matches!(
            parse_source(&project, &xml("Derived"), DataFormat::Xml),
            Err(RuntimeError::InvalidProject(_))
        ));
    }
    for path in [None, Some("input.xml"), Some("nested/input.xml")] {
        let mut project = project(true);
        project.source_path = path.map(str::to_owned);
        assert_eq!(output(&project, &xml("Derived")), expected(true));
    }
}

#[test]
fn root_policy_keeps_annotation_length_limit() {
    let project = project(true);
    assert!(matches!(
        parse_source(&project, &xml(&"A".repeat(4097)), DataFormat::Xml),
        Err(RuntimeError::Parse {
            format: DataFormat::Xml,
            ..
        })
    ));
}

#[test]
fn boundary_parsing_does_not_bypass_run_scope_validation() {
    let mut project = project(true);
    project.root.filter = Some(1);
    assert!(parse_source(&project, &xml("Derived"), DataFormat::Xml).is_ok());
    assert!(matches!(
        run(&project, &xml("Derived"), DataFormat::Xml, DataFormat::Json),
        Err(RuntimeError::InvalidProject(_))
    ));
}
