use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedSource, Node, Project, Scope, ScopeIteration, ScopeSequence};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_{tag}_{}_{}",
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

fn binding(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn repeated_group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children).repeating()
}

fn scalar_group(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), Instance::Scalar(value)))
            .collect(),
    )
}

fn nested_position_project() -> Project {
    let source = SchemaNode::group(
        "Offices",
        vec![repeated_group(
            "Office",
            vec![repeated_group(
                "Contact",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )],
        )],
    );
    let target = SchemaNode::group(
        "Contacts",
        vec![repeated_group(
            "Contact",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("OfficePosition", ScalarType::Int),
                SchemaNode::scalar("ContactPosition", ScalarType::Int),
            ],
        )],
    );
    Project {
        source,
        target,
        source_path: Some("offices.xml".into()),
        target_path: Some("contacts.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Position {
                        collection: vec!["Office".into()],
                    },
                ),
                (
                    1,
                    Node::Position {
                        collection: vec!["Contact".into()],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Office".into(), "Contact".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Contact".into(),
                iteration: ScopeIteration::Source(vec!["Office".into(), "Contact".into()]),
                bindings: vec![
                    binding("OfficePosition", 0),
                    binding("ContactPosition", 1),
                    binding("Name", 2),
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn nested_position_source() -> Instance {
    let contacts = |names: &[&str]| {
        Instance::Repeated(
            names
                .iter()
                .map(|name| scalar_group([("Name", Value::String((*name).into()))]))
                .collect(),
        )
    };
    Instance::Group(vec![(
        "Office".into(),
        Instance::Repeated(vec![
            Instance::Group(vec![("Contact".into(), contacts(&["Ada", "Grace"]))]),
            Instance::Group(vec![("Contact".into(), contacts(&["Linus"]))]),
        ]),
    )])
}

#[test]
fn multi_hop_scope_positions_roundtrip_with_each_collection_stage() -> Result<(), Box<dyn Error>> {
    let project = nested_position_project();
    assert!(engine::validate(&project).is_empty());
    let source = nested_position_source();
    let expected = engine::run(&project, &source)?;

    let dir = TempDir::new("multi_hop_position")?;
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = std::fs::read_to_string(&design)?;
    let document = roxmltree::Document::parse(&xml)?;
    let source_component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("Offices"))
        .ok_or("missing Offices source component")?;
    let entry_key = |name: &str| {
        source_component
            .descendants()
            .find(|entry| entry.has_tag_name("entry") && entry.attribute("name") == Some(name))
            .and_then(|entry| entry.attribute("outkey"))
    };
    let office_key = entry_key("Office").ok_or("Office has no source port")?;
    let contact_key = entry_key("Contact").ok_or("Contact has no source port")?;
    let position_inputs = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some("position"))
        .filter_map(|component| {
            component
                .children()
                .find(|node| node.has_tag_name("sources"))
                .and_then(|sources| {
                    sources
                        .children()
                        .find(|node| node.has_tag_name("datapoint"))
                })
                .and_then(|pin| pin.attribute("key"))
        })
        .collect::<Vec<_>>();
    assert_eq!(position_inputs.len(), 2);
    for (source_key, position_input) in [office_key, contact_key].into_iter().zip(position_inputs) {
        assert!(document.descendants().any(|node| {
            node.has_tag_name("edge")
                && node.attribute("vertexkey") == Some(position_input)
                && node
                    .parent()
                    .and_then(|edges| edges.parent())
                    .is_some_and(|vertex| vertex.attribute("vertexkey") == Some(source_key))
        }));
    }

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let actual = engine::run(&imported.project, &source)?;
    let rows = |instance: &Instance| {
        instance
            .field("Contact")
            .and_then(Instance::as_repeated)
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        (
                            row.field("Name").and_then(Instance::as_scalar).cloned(),
                            row.field("OfficePosition")
                                .and_then(Instance::as_scalar)
                                .cloned(),
                            row.field("ContactPosition")
                                .and_then(Instance::as_scalar)
                                .cloned(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
    };
    assert_eq!(rows(&actual), rows(&expected));
    Ok(())
}

fn named_source_concatenation_project() -> Project {
    let catalog = SchemaNode::group(
        "Catalog",
        vec![
            repeated_group(
                "Person",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
            repeated_group(
                "Address",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
        ],
    );
    let entry = repeated_group(
        "Entry",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Branch", ScalarType::String),
        ],
    );
    let segment = |branch| Scope {
        iteration: ScopeIteration::Source(vec!["catalog".into(), "Address".into()]),
        bindings: vec![binding("Value", 1), binding("Branch", branch)],
        ..Scope::default()
    };
    Project {
        source: SchemaNode::group(
            "Driver",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("DriverValue", ScalarType::String),
                repeated_group(
                    "Person",
                    vec![SchemaNode::scalar("Name", ScalarType::String), entry],
                ),
            ],
        ),
        source_path: Some("driver.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "catalog".into(),
            path: "catalog.xml".into(),
            schema: catalog,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["catalog".into(), "Person".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["catalog".into(), "Address".into()]),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("primary".into()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("secondary".into()),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![binding("DriverValue", 4)],
            children: vec![Scope {
                target_field: "Person".into(),
                iteration: ScopeIteration::Source(vec!["catalog".into(), "Person".into()]),
                bindings: vec![binding("Name", 0)],
                children: vec![Scope {
                    target_field: "Entry".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        segment(2),
                        vec![segment(3)],
                    )),
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn nested_concatenation_can_switch_absolute_collections_in_one_named_source()
-> Result<(), Box<dyn Error>> {
    let project = named_source_concatenation_project();
    assert!(engine::validate(&project).is_empty());
    let primary = scalar_group([("Value", Value::String("unused".into()))]);
    let catalog = Instance::Group(vec![
        (
            "Person".into(),
            Instance::Repeated(vec![
                scalar_group([("Name", Value::String("Ada".into()))]),
                scalar_group([("Name", Value::String("Grace".into()))]),
            ]),
        ),
        (
            "Address".into(),
            Instance::Repeated(vec![
                scalar_group([("Value", Value::String("North".into()))]),
                scalar_group([("Value", Value::String("South".into()))]),
            ]),
        ),
    ]);
    let expected = engine::run_with_sources(
        &project,
        &primary,
        vec![("catalog".into(), catalog.clone())],
    )?;

    let dir = TempDir::new("named_source_concat")?;
    let design = dir.0.join("mapping.mfd");
    let warnings = mfd::export(&project, &design)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.source.name, "Catalog");
    assert!(
        imported
            .project
            .extra_sources
            .iter()
            .any(|source| source.name == "Driver")
    );
    assert_eq!(
        engine::run_with_sources(
            &imported.project,
            &catalog,
            vec![("Driver".into(), primary)],
        )?,
        expected
    );
    Ok(())
}
