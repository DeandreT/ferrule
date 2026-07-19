use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, SequenceExpr};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_sequence_exists_export_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&path);
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
    let nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("AABB".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            2,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("BB".into()),
            },
        ),
        (
            4,
            Node::Call {
                function: "equal".into(),
                args: vec![2, 3],
            },
        ),
        (5, Node::Position { collection: vec![] }),
        (
            6,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            7,
            Node::Call {
                function: "greater_than".into(),
                args: vec![5, 6],
            },
        ),
        (
            8,
            Node::Call {
                function: "and".into(),
                args: vec![4, 7],
            },
        ),
        (
            9,
            Node::SequenceExists {
                sequence: SequenceExpr::TokenizeByLength {
                    input: 0,
                    length: 1,
                    item: 2,
                },
                predicate: 8,
            },
        ),
    ]);
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Unused", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Result", ScalarType::Bool)],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 9,
            }],
            ..Scope::default()
        },
    }
}

fn lookup_project() -> Project {
    let mut project = project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "MissionKit",
                vec![
                    SchemaNode::scalar("Edition", ScalarType::String),
                    SchemaNode::scalar("ToolCodes", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.insert(
        0,
        Node::Lookup {
            collection: vec!["MissionKit".into()],
            key: vec!["Edition".into()],
            matches: 10,
            value: vec!["ToolCodes".into()],
        },
    );
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String("Enterprise".into()),
        },
    );
    project
}

fn lookup_source() -> Instance {
    let row = |edition: &str, codes: &str| {
        Instance::Group(vec![
            (
                "Edition".into(),
                Instance::Scalar(Value::String(edition.into())),
            ),
            (
                "ToolCodes".into(),
                Instance::Scalar(Value::String(codes.into())),
            ),
        ])
    };
    Instance::Group(vec![(
        "MissionKit".into(),
        Instance::Repeated(vec![row("Basic", "ZZ"), row("Enterprise", "AABB")]),
    )])
}

#[test]
fn sequence_exists_exports_filter_chain_and_generated_position_context() {
    let dir = TempDir::new();
    let design = dir.0.join("sequence-exists.mfd");
    let warnings = mfd::export(&project(), &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let component = |name: &str| {
        document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
            .unwrap()
    };
    let pin = |name: &str, side: &str, pos: &str| {
        component(name)
            .children()
            .find(|node| node.has_tag_name(side))
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("datapoint") && node.attribute("pos") == Some(pos))
            .and_then(|node| node.attribute("key"))
            .unwrap()
            .to_string()
    };
    let has_edge = |from: &str, to: &str| {
        document.descendants().any(|vertex| {
            vertex.has_tag_name("vertex")
                && vertex.attribute("vertexkey") == Some(from)
                && vertex.descendants().any(|edge| {
                    edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(to)
                })
        })
    };

    assert_eq!(xml.matches("name=\"tokenize-by-length\"").count(), 1);
    assert_eq!(xml.matches("name=\"filter\"").count(), 1);
    assert_eq!(xml.matches("name=\"exists\"").count(), 1);

    let sequence_output = pin("tokenize-by-length", "targets", "0");
    let filter_nodes = pin("filter", "sources", "0");
    let filter_predicate = pin("filter", "sources", "1");
    let filter_output = pin("filter", "targets", "0");
    let item_predicate_input = pin("equal", "sources", "0");
    let predicate_output = pin("logical-and", "targets", "0");
    let position_input = pin("position", "sources", "0");
    let exists_input = pin("exists", "sources", "0");

    assert!(has_edge(&sequence_output, &filter_nodes));
    assert!(has_edge(&sequence_output, &item_predicate_input));
    assert!(has_edge(&sequence_output, &position_input));
    assert!(has_edge(&predicate_output, &filter_predicate));
    assert!(has_edge(&filter_output, &exists_input));

    for pos in ["0", "1"] {
        let input = pin("tokenize-by-length", "sources", pos);
        assert!(
            document.descendants().any(|edge| {
                edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(input.as_str())
            }),
            "sequence input {pos} is not connected\n{xml}"
        );
    }

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Unused".into(),
        Instance::Scalar(Value::String("ignored".into())),
    )]);
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        target.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
}

#[test]
fn lookup_fed_sequence_exists_roundtrips_and_executes() {
    let dir = TempDir::new();
    let design = dir.0.join("lookup-sequence-exists.mfd");
    let warnings = mfd::export(&lookup_project(), &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::SequenceExists {
            sequence: SequenceExpr::TokenizeByLength { input, .. },
            ..
        } if matches!(imported.project.graph.nodes.get(input), Some(Node::Lookup { .. }))
    )));

    let target = engine::run(&imported.project, &lookup_source()).unwrap();
    assert_eq!(
        target.field("Result").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
}
