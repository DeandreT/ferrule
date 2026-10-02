use engine::{EngineError, run, run_with_sources, validate};
use ir::{
    GroupAlternative, Instance, InstanceGroup, PrimaryRootError, ScalarType, SchemaNode, Value,
    XML_TYPE_FIELD, XmlTypeOrigin,
};
use mapping::{Binding, Graph, NamedSource, NamedTarget, Node, Project, Scope, ScopeIteration};
fn schema(namespace: Option<&str>) -> SchemaNode {
    let name =
        |local: &str| namespace.map_or_else(|| local.to_owned(), |ns| format!("{{{ns}}}{local}"));
    let mut schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String).attribute(),
            SchemaNode::scalar("Extra", ScalarType::String).attribute(),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: name("Base"),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: name("Derived"),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some(name("Base"));
    schema
}
fn project() -> Project {
    Project {
        source: schema(None),
        target: schema(None),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type: "Derived".into(),
                    },
                ),
                (
                    1,
                    Node::SourceRootField {
                        path: vec!["Code".into()],
                        required: false,
                    },
                ),
                (2, Node::Const { value: Value::Null }),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 1,
                        else_: 2,
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("Derived".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 3,
                },
                Binding {
                    target_field: XML_TYPE_FIELD.into(),
                    node: 4,
                },
            ],
            ..Scope::default()
        },
    }
}
fn marked(fields: Vec<(&str, Instance)>, origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<Vec<_>>(),
        )
        .with_xml_type_origin(origin)
        .unwrap(),
    )
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
#[test]
fn five_annotation_cases_gate_source_projection_but_keep_selected_target_type() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    for (xml, expected, annotation) in [
        (
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="a" Extra="b"/>"#,
            Value::String("a".into()),
            XmlTypeOrigin::Explicit("Derived"),
        ),
        (
            r#"<Root Code="a" Extra="b"/>"#,
            Value::Null,
            XmlTypeOrigin::Absent,
        ),
        (r#"<Root Code="a"/>"#, Value::Null, XmlTypeOrigin::Absent),
        (
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="a"/>"#,
            Value::Null,
            XmlTypeOrigin::Explicit("Base"),
        ),
        (
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Extra="b"/>"#,
            Value::Null,
            XmlTypeOrigin::Explicit("Derived"),
        ),
    ] {
        let source = format_xml::from_str(xml, &project.source).unwrap();
        let before = source.clone();
        assert_eq!(source.xml_type_origin(), Ok(annotation));
        let output = run(&project, &source).unwrap();
        assert_eq!(
            output.field("Code").and_then(Instance::as_scalar),
            Some(&expected)
        );
        assert_eq!(
            output.field(XML_TYPE_FIELD).and_then(Instance::as_scalar),
            Some(&Value::String("Derived".into()))
        );
        assert_eq!(output.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
        let bytes = format_xml::to_string(&project.target, &output).unwrap();
        assert!(bytes.contains("xsi:type=\"Derived\""));
        assert_eq!(source, before);
    }
}
#[test]
fn resolved_annotation_prefixes_compare_exact_canonical_identity() {
    let mut project = project();
    project.source = schema(Some("urn:t"));
    project.graph.nodes.insert(
        0,
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "{urn:t}Derived".into(),
        },
    );
    for prefix in ["a", "longAlias"] {
        let xml = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:{prefix}="urn:t" xsi:type="{prefix}:Derived" Code="exact" Extra="e"/>"#
        );
        let source = format_xml::from_str(&xml, &project.source).unwrap();
        assert_eq!(
            run(&project, &source)
                .unwrap()
                .field("Code")
                .and_then(Instance::as_scalar),
            Some(&Value::String("exact".into()))
        );
    }
}
#[test]
fn unknown_is_a_typed_reachable_error_and_unselected_branches_stay_lazy() {
    let mut project = project();
    let source = marked(
        vec![
            ("Code", text("ordinary")),
            (ir::XML_TYPE_ORIGIN_FIELD, text("Derived")),
            (XML_TYPE_FIELD, text("Derived")),
        ],
        XmlTypeOrigin::Unknown,
    );
    assert!(matches!(
        run(&project, &source),
        Err(EngineError::PrimaryRoot {
            node: 0,
            source: PrimaryRootError::UnknownXmlTypeOrigin
        })
    ));
    project.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.graph.nodes.insert(
        6,
        Node::If {
            condition: 5,
            then: 3,
            else_: 2,
        },
    );
    project.root.bindings[0].node = 6;
    assert_eq!(
        run(&project, &source)
            .unwrap()
            .field("Code")
            .and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
}
#[test]
fn exact_root_missing_never_reads_same_named_secondary_source() {
    let mut project = project();
    project.root.bindings[0].node = 1;
    project.extra_sources.push(NamedSource {
        name: "Code".into(),
        path: "unused.xml".into(),
        schema: SchemaNode::scalar("Code", ScalarType::String),
        options: Default::default(),
        dynamic_path: None,
    });
    let root = marked(vec![], XmlTypeOrigin::Absent);
    let result =
        run_with_sources(&project, &root, vec![("Code".into(), text("secondary"))]).unwrap();
    assert_eq!(
        result.field("Code").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    // The same missing root is not a required named-input access.
    assert_eq!(run(&project, &root).unwrap(), result);
}
#[test]
fn duplicate_and_wrong_shaped_root_values_preserve_typed_node_payloads() {
    let mut project = project();
    project.root.bindings[0].node = 1;
    let duplicate = marked(
        vec![("Code", text("one")), ("Code", text("two"))],
        XmlTypeOrigin::Absent,
    );
    assert!(
        matches!(run(&project,&duplicate),Err(EngineError::PrimaryRoot { node:1,source:PrimaryRootError::DuplicateField { path } }) if path==vec!["Code"])
    );
    let repeated = marked(
        vec![("Code", Instance::Repeated(vec![text("first")]))],
        XmlTypeOrigin::Absent,
    );
    assert!(
        matches!(run(&project,&repeated),Err(EngineError::PrimaryRoot { node:1,source:PrimaryRootError::ExpectedScalar { path,found:"repeated" } }) if path==vec!["Code"])
    );
}
#[test]
fn initial_context_gate_rejects_controls_descendants_named_targets_and_virtual_fields() {
    let mut filtered = project();
    filtered.root.filter = Some(0);
    assert!(!validate(&filtered).is_empty());
    let mut descendant = project();
    let mut child = Scope {
        target_field: "Child".into(),
        bindings: vec![Binding {
            target_field: "Code".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    child.iteration = ScopeIteration::None;
    descendant.root.children.push(child);
    assert!(!validate(&descendant).is_empty());
    let mut named = project();
    named.extra_targets.push(NamedTarget {
        name: "other".into(),
        path: None,
        schema: named.target.clone(),
        options: Default::default(),
        root: named.root.clone(),
    });
    assert!(!validate(&named).is_empty());
    let mut virtual_path = project();
    virtual_path.graph.nodes.insert(
        1,
        Node::SourceRootField {
            path: vec![XML_TYPE_FIELD.into()],
            required: false,
        },
    );
    assert!(!validate(&virtual_path).is_empty());
    let mut malformed = project();
    malformed.graph.nodes.insert(
        0,
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "p:Derived".into(),
        },
    );
    assert!(!validate(&malformed).is_empty());
}

#[test]
fn isolated_user_function_cannot_read_primary_root_provenance_or_data() {
    for node in [
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Derived".into(),
        },
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: false,
        },
    ] {
        let mut project = project();
        let id = mapping::FunctionId::new(1);
        project.user_functions.insert(
            id,
            mapping::UserFunction {
                library: "test".into(),
                name: "isolated".into(),
                description: None,
                parameters: vec![],
                output_name: "result".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: [(0, node)].into(),
                },
                output: 0,
            },
        );
        project.graph.nodes.insert(
            5,
            Node::UserFunctionCall {
                function: id,
                args: vec![],
            },
        );
        project.root.bindings[0].node = 5;
        assert!(
            validate(&project).iter().any(|issue| issue.message
                == "node kind is not supported in an isolated scalar user-defined function"),
            "{:?}",
            validate(&project)
        );
    }
}

#[test]
fn engine_never_unwraps_wrong_primary_owner_for_either_primitive() {
    let known = marked(
        vec![("Code", text("first"))],
        XmlTypeOrigin::Explicit("Derived"),
    );
    for (root, kind) in [
        (text("scalar"), "scalar"),
        (Instance::Repeated(vec![known.clone()]), "repeated"),
        (
            Instance::MappedSequence(vec![known.clone()]),
            "mapped sequence",
        ),
        (
            Instance::DocumentSet(vec![ir::DocumentMember::new("first.xml", known).unwrap()]),
            "document set",
        ),
    ] {
        let mut project = project();
        assert!(
            matches!(run(&project,&root),Err(EngineError::PrimaryRoot {node:0,source:PrimaryRootError::ExpectedGroup {found}}) if found==kind)
        );
        project.root.bindings[0].node = 1;
        assert!(
            matches!(run(&project,&root),Err(EngineError::PrimaryRoot {node:1,source:PrimaryRootError::ExpectedGroup {found}}) if found==kind)
        );
    }
}
#[test]
fn primary_annotation_fact_stays_private_in_trace_and_is_immutable() {
    #[derive(Default)]
    struct Sink(std::cell::RefCell<Vec<engine::TraceEvent>>);
    impl engine::TraceSink for Sink {
        fn record(&self, event: engine::TraceEvent) {
            self.0.borrow_mut().push(event);
        }
    }
    let source = marked(
        vec![("Code", text("ordinary"))],
        XmlTypeOrigin::Explicit("PrivateObservedAnnotation"),
    );
    let before = source.clone();
    let sink = Sink::default();
    let context = engine::ExecutionContext::new(std::path::Path::new("private-project.json"))
        .with_trace_sink(&sink);
    let result = engine::run_with_context(&project(), &source, &context).unwrap();
    assert_eq!(
        result.field("Code").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    assert!(!sink.0.borrow().is_empty());
    assert!(!format!("{:?}", sink.0.borrow()).contains("PrivateObservedAnnotation"));
    assert_eq!(source, before);
}
