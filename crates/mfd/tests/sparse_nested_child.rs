use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sparse-nested-child")
        .join(name)
}

fn source_group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children)
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}

fn optional(name: &str, ty: ScalarType) -> SchemaNode {
    let mut node = SchemaNode::scalar(name, ty);
    node.xml_optional = true;
    node
}

fn expected() -> Project {
    let fields = || {
        vec![
            optional("Value", ScalarType::String),
            optional("Qty", ScalarType::Int),
            optional("OwnerId", ScalarType::Int),
            optional("Label", ScalarType::String),
        ]
    };
    let target_child = SchemaNode::group(
        "Child",
        ["Header", "Detail", "Footer"]
            .into_iter()
            .map(|group| SchemaNode::group(group, fields()))
            .collect(),
    )
    .repeating();
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("left".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("right".into()),
            },
        ),
    ]);
    for (id, field, frame, path) in [
        (2, "Id", vec!["First"], vec!["Id"]),
        (3, "Id", vec!["Second"], vec!["Id"]),
        (4, "Value", vec!["Second", "Details"], vec!["Value"]),
        (5, "Qty", vec!["Second", "Details"], vec!["Qty"]),
    ] {
        assert_eq!(path.last(), Some(&field));
        nodes.insert(
            id,
            Node::SourceField {
                frame: Some(frame.into_iter().map(String::from).collect()),
                path: path.into_iter().map(String::from).collect(),
            },
        );
    }
    let parent = |source: &str, marker, id, children| Scope {
        iteration: ScopeIteration::Source(vec![source.into()]),
        bindings: vec![
            Binding {
                target_field: "Marker".into(),
                node: marker,
            },
            Binding {
                target_field: "Id".into(),
                node: id,
            },
        ],
        children,
        ..Default::default()
    };
    let child = Scope {
        target_field: "Child".into(),
        iteration: ScopeIteration::Source(vec!["Details".into()]),
        children: ["Header", "Detail", "Footer"]
            .into_iter()
            .map(|name| Scope {
                target_field: name.into(),
                bindings: [("Value", 4), ("Qty", 5), ("OwnerId", 3), ("Label", 1)]
                    .into_iter()
                    .map(|(field, node)| Binding {
                        target_field: field.into(),
                        node,
                    })
                    .collect(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                source_group("First", vec![SchemaNode::scalar("Id", ScalarType::Int)]),
                source_group(
                    "Second",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::Int),
                        source_group(
                            "Details",
                            vec![
                                SchemaNode::scalar("Value", ScalarType::String),
                                SchemaNode::scalar("Qty", ScalarType::Int),
                            ],
                        ),
                    ],
                ),
            ],
        ),
        target: SchemaNode::group(
            "Result",
            vec![
                SchemaNode::group(
                    "Parent",
                    vec![
                        optional("Marker", ScalarType::String),
                        optional("Id", ScalarType::Int),
                        target_child,
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Parent".into(),
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                    parent("First", 0, 2, vec![]),
                    vec![parent("Second", 1, 3, vec![child])],
                )),
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

#[test]
fn sparse_child_preserves_complete_project_twelve_bindings_and_values() {
    let imported = mfd::import(&fixture("sparse.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project).unwrap(),
        mapping::project_file::encode_pretty(&expected()).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/sparse-nested-child/cases.json")).unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source)
                .unwrap();
        let actual = engine::run(&imported.project, &source).unwrap();
        let xml = format_xml::to_string_with_options(
            &imported.project.target,
            &actual,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(xml, case["expected_xml"].as_str().unwrap(), "{case}");
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &actual).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn sparse_same_feed_clone_keeps_parent_order_and_selected_child() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct FixtureDirectory(PathBuf);
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = FixtureDirectory(std::env::temp_dir().join(format!(
        "ferrule_sparse_clone_order_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&directory.0).unwrap();
    for name in ["source.schema.json", "target.xsd"] {
        std::fs::copy(fixture(name), directory.0.join(name)).unwrap();
    }
    let original = std::fs::read_to_string(fixture("sparse.mfd")).unwrap();
    let target_end = "</entry></root><document schema=\"target.xsd\"";
    assert_eq!(original.matches(target_end).count(), 1);
    let parent = r#"<entry name="Parent" inpkey="90" clone="1"><entry name="Marker" inpkey="91"/><entry name="Id" inpkey="92"/></entry>"#;
    let design = original.replace(target_end, &format!("{parent}{target_end}"));
    let edges = r#"<edge from="20" to="90"/><edge from="100" to="91"/><edge from="21" to="92"/>"#;
    assert_eq!(design.matches("</connections>").count(), 1);
    let design = design.replace("</connections>", &format!("{edges}</connections>"));
    let path = directory.0.join("sparse-same-feed.mfd");
    std::fs::write(&path, design).unwrap();

    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let mut expected_project = expected();
    let segments = expected_project.root.children[0]
        .concatenated()
        .unwrap()
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut tail = segments[1].clone();
    tail.bindings[0].node = 0;
    tail.children.clear();
    expected_project.root.children[0].iteration = ScopeIteration::Concatenate(ScopeSequence::new(
        segments[0].clone(),
        vec![segments[1].clone(), tail],
    ));
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project).unwrap(),
        mapping::project_file::encode_pretty(&expected_project).unwrap()
    );
    let actual_segments = imported.project.root.children[0]
        .concatenated()
        .unwrap()
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(actual_segments.len(), 3);
    for (segment, source) in actual_segments.iter().zip(["First", "Second", "Second"]) {
        assert_eq!(
            segment.source().unwrap(),
            Some([source.to_string()].as_slice())
        );
    }
    assert!(actual_segments[0].children.is_empty());
    assert_eq!(actual_segments[1].children[0].target_field, "Child");
    assert!(actual_segments[2].children.is_empty());

    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/sparse-nested-child/cases.json")).unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let literal: serde_json::Value = serde_json::from_str(input).unwrap();
        let mut expected_value = case["expected"].clone();
        let tails = literal["Second"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| serde_json::json!({"Marker": "left", "Id": row["Id"]}))
            .collect::<Vec<_>>();
        expected_value["Parent"]
            .as_array_mut()
            .unwrap()
            .extend(tails.clone());
        let mut expected_xml = case["expected_xml"]
            .as_str()
            .unwrap()
            .strip_suffix("</Result>")
            .unwrap()
            .to_string();
        for row in tails {
            expected_xml.push_str(&format!(
                "<Parent><Marker>left</Marker><Id>{}</Id></Parent>",
                row["Id"].as_i64().unwrap()
            ));
        }
        expected_xml.push_str("</Result>");
        let source = format_json::from_str(input, &imported.project.source).unwrap();
        let output = engine::run(&imported.project, &source).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, expected_value, "{}", case["name"]);
        let xml = format_xml::to_string_with_options(
            &imported.project.target,
            &output,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(xml, expected_xml, "{}", case["name"]);
    }

    let program = codegen::lower(&imported.project).unwrap();
    let rust = codegen_rust::emit(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-sparse-clone-integration".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()
                    .unwrap()
                    .display()
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let csharp = codegen_csharp::emit(&program).unwrap();
    if let Some(root) = std::env::var_os("FERRULE_FEATURE_INTEGRATION_ARTIFACT_ROOT") {
        let output = PathBuf::from(root).join("sparse-same-feed");
        std::fs::create_dir(&output).unwrap();
        for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
            for file in artifacts.files() {
                let path = output.join(language).join(file.path.as_str());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.contents).unwrap();
            }
        }
    }
}
