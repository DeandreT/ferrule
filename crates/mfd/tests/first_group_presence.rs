use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration, SequenceWindow,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_first_presence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn project() -> Project {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Department",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group(
                        "Person",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Keep", ScalarType::Bool),
                            SchemaNode::scalar("Score", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Department",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group(
                        "Selected",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Position", ScalarType::Int),
                        ],
                    ),
                ],
            )
            .repeating(),
        ],
    );
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Id".into()],
                        frame: Some(vec!["Department".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Score".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    4,
                    Node::Position {
                        // The inner iteration records its relative selector.
                        collection: vec!["Person".into()],
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Department".into(),
                iteration: ScopeIteration::Source(vec!["Department".into()]),
                bindings: vec![Binding {
                    target_field: "Id".into(),
                    node: 0,
                }],
                children: vec![Scope {
                    target_field: "Selected".into(),
                    iteration: ScopeIteration::Source(vec!["Person".into()]),
                    filter: Some(2),
                    sort_by: Some(3),
                    sort_descending: true,
                    windows: vec![
                        SequenceWindow::SkipFirst { count: 5 },
                        SequenceWindow::First { count: 6 },
                    ],
                    iteration_output: IterationOutput::First,
                    bindings: vec![
                        Binding {
                            target_field: "Name".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "Position".into(),
                            node: 4,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}
const INPUT: &str = "<Source><Department><Id>A</Id><Person><Name>discard</Name><Keep>false</Keep><Score>100</Score></Person><Person><Name>first</Name><Keep>true</Keep><Score>90</Score></Person><Person><Name>selected</Name><Keep>true</Keep><Score>80</Score></Person></Department><Department><Id>B</Id><Person><Name>none</Name><Keep>false</Keep><Score>10</Score></Person></Department></Source>";
fn incoming(design: &roxmltree::Document<'_>, input: &str) -> Vec<u32> {
    design
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .flat_map(|node| {
            let from = node.attribute("vertexkey").unwrap().parse().unwrap();
            node.descendants()
                .filter(move |edge| {
                    edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(input)
                })
                .map(move |_| from)
        })
        .collect()
}
fn assert_guard(project: &Project) {
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );
    let dir = TempDir::new();
    let path = dir.0.join("mapping.mfd");
    let report = mfd::preflight_export(project, &path).unwrap();
    assert!(!report.is_native_compatible());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::XmlFirstGroupPresence),
        "{report:?}"
    );
    assert!(matches!(
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
    let extensions =
        mfd::export_with_profile(project, &path, mfd::ExportProfile::FerruleExtensions).unwrap();
    assert!(
        extensions
            .issues
            .iter()
            .any(|issue| issue.feature == mfd::ExportCompatibilityFeature::XmlFirstGroupPresence)
    );
    assert!(path.exists());
}
#[test]
fn qualified_first_separates_parent_presence_from_selected_payload() {
    let p = project();
    let d = TempDir::new();
    let path = d.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.is_native_compatible());
    let text = std::fs::read_to_string(&path).unwrap();
    let xml = roxmltree::Document::parse(&text).unwrap();
    let selected = xml
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Selected"))
        .unwrap();
    let parent = selected.parent_element().unwrap();
    let driver = incoming(&xml, selected.attribute("inpkey").unwrap());
    assert_eq!(driver.len(), 1);
    assert_eq!(driver, incoming(&xml, parent.attribute("inpkey").unwrap()));
    let variable = xml
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("xml")
                && node.descendants().any(|entry| {
                    entry.has_tag_name("parameter")
                        && entry.attribute("usageKind") == Some("variable")
                })
        })
        .unwrap();
    let compute = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("compute-when"))
        .unwrap();
    assert_eq!(driver, incoming(&xml, compute.attribute("inpkey").unwrap()));
    let payload = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Person"))
        .unwrap();
    let payload_key = payload.attribute("outkey").unwrap().parse::<u32>().unwrap();
    assert_ne!(driver[0], payload_key);
    let position = xml
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("core")
                && node.attribute("name") == Some("position")
        })
        .unwrap();
    let input = position
        .descendants()
        .find(|node| node.has_tag_name("sources"))
        .unwrap()
        .children()
        .find(|node| node.is_element())
        .unwrap()
        .attribute("key")
        .unwrap();
    assert_eq!(incoming(&xml, input), vec![payload_key]);
    assert_eq!(text.matches(r#"name="first-items""#).count(), 2);
    assert!(text.contains(r#"dataconnection type="2""#));
}
#[test]
fn first_empty_candidates_remain_a_present_empty_group_for_each_empty_cause() {
    for case in 0..4 {
        let mut p = project();
        let input = if case == 3 {
            "<Source><Department><Id>B</Id></Department></Source>"
        } else {
            INPUT
        };
        if case == 0 {
            p.graph.nodes.insert(
                2,
                Node::Const {
                    value: Value::Bool(false),
                },
            );
        }
        if case == 1 {
            p.graph.nodes.insert(
                6,
                Node::Const {
                    value: Value::Int(0),
                },
            );
        }
        if case == 2 {
            p.graph.nodes.insert(
                5,
                Node::Const {
                    value: Value::Int(100),
                },
            );
        }
        let source = format_xml::from_str(input, &p.source).unwrap();
        let output = engine::run(&p, &source).unwrap();
        for item in output.field("Department").unwrap().as_repeated().unwrap() {
            assert_eq!(
                item.field("Selected"),
                Some(&Instance::Group((vec![]).into()))
            );
        }
        let written = format_xml::to_string(&p.target, &output).unwrap();
        assert!(written.contains("<Selected"));
        let d = TempDir::new();
        let report = mfd::preflight_export(&p, &d.0.join("mapping.mfd")).unwrap();
        assert!(report.is_native_compatible(), "{report:?}");
    }
}
#[test]
fn first_without_a_selected_position_has_a_native_only_diagnostic() {
    let mut p = project();
    p.root.children[0].children[0].bindings.pop();
    assert_guard(&p);
}
#[test]
fn constants_cannot_populate_an_empty_first_group_in_native_mode() {
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::String("constant".into()),
        },
    );
    p.root.children[0].children[0].bindings[0].node = 7;
    assert_guard(&p);
}
#[test]
fn broadcasts_and_unpinned_scalar_fields_do_not_claim_selected_payload_context() {
    let mut p = project();
    p.root.children[0].children[0].bindings[0].node = 0;
    assert_guard(&p);
    let mut p = project();
    if let Node::SourceField { frame, .. } = p.graph.nodes.get_mut(&1).unwrap() {
        *frame = None;
    }
    assert_guard(&p);
}
#[test]
fn a_controlled_parent_is_outside_the_qualified_raw_parent_boundary() {
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.root.children[0].filter = Some(7);
    assert_guard(&p);
}
#[test]
fn copied_first_groups_keep_their_existing_extension_export_and_reject_native_presence() {
    let mut p = project();
    let source_kind = p
        .source
        .child("Department")
        .unwrap()
        .child("Person")
        .unwrap()
        .kind
        .clone();
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
    {
        children[1].kind = source_kind;
    }
    let selected = &mut p.root.children[0].children[0];
    selected.construction = mapping::ScopeConstruction::CopyCurrentSource;
    selected.bindings.clear();
    selected.filter = None;
    selected.sort_by = None;
    selected.windows.clear();
    assert_guard(&p);
}

#[test]
fn mixed_first_content_does_not_claim_the_plain_constructed_presence_mechanism() {
    let mut p = project();
    if let ir::SchemaKind::Group { children, .. } = &mut p.source.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[1].kind
    {
        children[0].repeating = true;
        children.push(SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text());
    }
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[1].kind
    {
        children[0].repeating = true;
        children.push(SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text());
    }
    let selected = &mut p.root.children[0].children[0];
    selected.construction = mapping::ScopeConstruction::XmlMixedContent {
        elements: vec![mapping::XmlMixedContentElement {
            source: "Name".into(),
            target: "Name".into(),
        }],
    };
    selected.filter = None;
    selected.sort_by = None;
    selected.windows.clear();
    selected.bindings.remove(0);
    assert_guard(&p);
}

#[test]
fn absolute_selected_scalar_fields_keep_the_qualified_payload_context() {
    let mut p = project();
    p.root.children[0].children[0].iteration =
        ScopeIteration::Source(vec!["Department".into(), "Person".into()]);
    p.graph.nodes.insert(
        4,
        Node::Position {
            collection: vec!["Department".into(), "Person".into()],
        },
    );
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Department".into(), "Person".into(), "Name".into()],
            frame: None,
        },
    );
    let source = format_xml::from_str(INPUT, &p.source).unwrap();
    let output = engine::run(&p, &source).unwrap();
    assert_eq!(
        output.field("Department").unwrap().as_repeated().unwrap()[0]
            .field("Selected")
            .unwrap()
            .field("Name"),
        Some(&Instance::Scalar(Value::String("selected".into())))
    );
    let d = TempDir::new();
    let report = mfd::preflight_export(&p, &d.0.join("mapping.mfd")).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
}

#[test]
fn optional_payload_fields_do_not_change_the_group_presence_qualification() {
    let mut p = project();
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[1].kind
    {
        for field in children {
            field.set_xml_optional(true);
        }
    }
    let d = TempDir::new();
    let path = d.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let target = std::fs::read_to_string(d.0.join("mapping-target.xsd")).unwrap();
    assert!(
        target.contains(r#"name="Name" type="xs:string" minOccurs="0""#),
        "{target}"
    );
    assert!(
        target.contains(r#"name="Position" type="xs:integer" minOccurs="0""#),
        "{target}"
    );
}

#[test]
fn non_xml_first_targets_keep_their_existing_typed_rejection() {
    let mut p = project();
    for extension in ["json", "csv"] {
        p.target_path = Some(format!("target.{extension}"));
        let d = TempDir::new();
        let error = mfd::preflight_export(&p, &d.0.join("mapping.mfd")).unwrap_err();
        assert!(
            matches!(&error, mfd::MfdError::Unsupported(message) if message.contains("requires an XML or XBRL target")),
            "{error:?}"
        );
        assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 0);
    }
}

#[test]
fn an_absolute_scalar_path_under_a_relative_scope_does_not_claim_current_selection() {
    let mut p = project();
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Department".into(), "Person".into(), "Name".into()],
            frame: None,
        },
    );
    assert_guard(&p);
}
