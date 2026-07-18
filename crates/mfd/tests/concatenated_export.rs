use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{GroupAlternative, Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration, ScopeSequence,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_concatenated_export_{}_{}",
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

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)])
                .repeating(),
        ],
    )
}

fn source_instance() -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            ["Alpha", "Beta"]
                .into_iter()
                .map(|name| {
                    Instance::Group(vec![(
                        "Name".into(),
                        Instance::Scalar(Value::String(name.into())),
                    )])
                })
                .collect(),
        ),
    )])
}

fn graph() -> Graph {
    Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: Some(vec!["Row".into()]),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("primary".into()),
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("secondary".into()),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String("heading".into()),
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::String("static".into()),
                },
            ),
        ]),
    }
}

fn binding(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn source_segment(kind: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec!["Row".into()]),
        bindings: vec![binding("Name", 0), binding("Kind", kind)],
        ..Scope::default()
    }
}

fn xml_project() -> Project {
    let target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::group(
                "Item",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Kind", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let item = Scope {
        target_field: "Item".into(),
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(
            source_segment(1),
            vec![source_segment(2)],
        )),
        ..Scope::default()
    };
    Project {
        source: source_schema(),
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: graph(),
        root: Scope {
            children: vec![item],
            ..Scope::default()
        },
    }
}

fn csv_project() -> Project {
    let target = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Kind", ScalarType::String),
        ],
    );
    let singleton = Scope {
        bindings: vec![binding("Name", 3), binding("Kind", 4)],
        ..Scope::default()
    };
    Project {
        source: source_schema(),
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.csv".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: graph(),
        root: Scope {
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                singleton,
                vec![source_segment(1)],
            )),
            ..Scope::default()
        },
    }
}

fn typed_address(name: &str) -> SchemaNode {
    let identity = |local: &str| format!("{{urn:ferrule:conditioned-concat}}{local}");
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("State", ScalarType::String),
            SchemaNode::scalar("Postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: identity("Domestic"),
            members: vec!["Name".into(), "State".into()],
            required: Vec::new(),
        },
        GroupAlternative {
            name: identity("International"),
            members: vec!["Name".into(), "Postcode".into()],
            required: Vec::new(),
        },
    ])
    .unwrap()
}

fn conditioned_segment(filter: u32, type_node: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(Vec::new()),
        filter: Some(filter),
        iteration_output: IterationOutput::MappedSequence,
        bindings: vec![
            binding("Name", 5),
            binding("State", 6),
            binding("Postcode", 7),
            binding(XML_TYPE_FIELD, type_node),
        ],
        ..Scope::default()
    }
}

fn conditioned_xml_project() -> Project {
    let source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group("Row", vec![typed_address("Address")]).repeating()],
    );
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::group("Item", vec![typed_address("Address")]).repeating()],
    );
    let domestic = Value::String("{urn:ferrule:conditioned-concat}Domestic".into());
    let international = Value::String("{urn:ferrule:conditioned-concat}International".into());
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Address".into(), XML_TYPE_FIELD.into()],
                    frame: Some(vec!["Row".into()]),
                },
            ),
            (1, Node::Const { value: domestic }),
            (
                2,
                Node::Call {
                    function: "equal".into(),
                    args: vec![0, 1],
                },
            ),
            (
                3,
                Node::Const {
                    value: international,
                },
            ),
            (
                4,
                Node::Call {
                    function: "equal".into(),
                    args: vec![0, 3],
                },
            ),
            (
                5,
                Node::SourceField {
                    path: vec!["Address".into(), "Name".into()],
                    frame: Some(vec!["Row".into()]),
                },
            ),
            (
                6,
                Node::SourceField {
                    path: vec!["Address".into(), "State".into()],
                    frame: Some(vec!["Row".into()]),
                },
            ),
            (
                7,
                Node::SourceField {
                    path: vec!["Address".into(), "Postcode".into()],
                    frame: Some(vec!["Row".into()]),
                },
            ),
        ]),
    };
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                children: vec![Scope {
                    target_field: "Address".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        conditioned_segment(2, 1),
                        vec![conditioned_segment(4, 3)],
                    )),
                    iteration_output: IterationOutput::MappedSequence,
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn conditioned_source_instance() -> Instance {
    let address = |name: &str, type_name: &str, field: &str, value: &str| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "State".into(),
                Instance::Scalar(if field == "State" {
                    Value::String(value.into())
                } else {
                    Value::Null
                }),
            ),
            (
                "Postcode".into(),
                Instance::Scalar(if field == "Postcode" {
                    Value::String(value.into())
                } else {
                    Value::Null
                }),
            ),
            (
                XML_TYPE_FIELD.into(),
                Instance::Scalar(Value::String(type_name.into())),
            ),
        ])
    };
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![
            Instance::Group(vec![(
                "Address".into(),
                address(
                    "West",
                    "{urn:ferrule:conditioned-concat}Domestic",
                    "State",
                    "CA",
                ),
            )]),
            Instance::Group(vec![(
                "Address".into(),
                address(
                    "North",
                    "{urn:ferrule:conditioned-concat}International",
                    "Postcode",
                    "N1",
                ),
            )]),
        ]),
    )])
}

fn export_import(project: &Project) -> Result<Project, Box<dyn Error>> {
    let dir = TempDir::new()?;
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    Ok(imported.project)
}

#[test]
fn cloned_xml_branches_roundtrip_identical_source_collections() -> Result<(), Box<dyn Error>> {
    let project = xml_project();
    assert!(engine::validate(&project).is_empty());
    let imported = export_import(&project)?;
    let segments = imported.root.children[0]
        .concatenated()
        .ok_or("XML concatenation was not reconstructed")?;
    assert_eq!(segments.len(), 2);
    assert!(segments.iter().all(|segment| segment.filter.is_some()));

    let source = source_instance();
    assert_eq!(
        engine::run(&project, &source)?,
        engine::run(&imported, &source)?
    );
    Ok(())
}

#[test]
fn csv_singleton_and_repeated_rows_roundtrip_in_order() -> Result<(), Box<dyn Error>> {
    let project = csv_project();
    assert!(engine::validate(&project).is_empty());
    let imported = export_import(&project)?;
    let segments = imported
        .root
        .concatenated()
        .ok_or("CSV concatenation was not reconstructed")?;
    assert_eq!(segments.len(), 2);
    assert!(
        segments
            .iter()
            .next()
            .is_some_and(|segment| !segment.iterates())
    );

    let source = source_instance();
    assert_eq!(
        engine::run(&project, &source)?,
        engine::run(&imported, &source)?
    );
    Ok(())
}

#[test]
fn csv_rejects_multiple_repeated_segments_before_writing() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, "keep")?;
    let mut project = csv_project();
    project.root.iteration = ScopeIteration::Concatenate(ScopeSequence::new(
        source_segment(1),
        vec![source_segment(2)],
    ));

    let error = mfd::export(&project, &design).expect_err("multiple CSV row drivers must fail");
    assert!(
        error
            .to_string()
            .contains("exactly one repeated row segment")
    );
    assert_eq!(std::fs::read_to_string(design)?, "keep");
    Ok(())
}

#[test]
fn conditioned_non_repeating_xml_branches_roundtrip() -> Result<(), Box<dyn Error>> {
    let project = conditioned_xml_project();
    assert!(engine::validate(&project).is_empty());
    let imported = export_import(&project)?;
    let segments = imported.root.children[0].children[0]
        .concatenated()
        .ok_or("conditioned XML branches were not reconstructed")?;
    assert_eq!(segments.len(), 2);
    assert!(segments.iter().all(|segment| segment.filter.is_some()));

    let source = conditioned_source_instance();
    assert_eq!(
        engine::run(&project, &source)?,
        engine::run(&imported, &source)?
    );
    Ok(())
}
