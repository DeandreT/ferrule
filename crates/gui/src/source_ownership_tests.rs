use super::*;
use crate::canvas::project_source_blocks;
use ir::ScalarType;

fn value_group(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

fn project(primary: SchemaNode, sources: Vec<NamedSource>) -> Project {
    Project {
        source: primary,
        target: value_group("Output"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: sources,
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Default::default(),
    }
}

fn reads(project: &Project) -> Vec<Vec<PinRead>> {
    let ownership = SourceOwnership::new(&project.source, &project.extra_sources)
        .with_project_usage(project_uses_ownership(project));
    project_source_blocks(project)
        .iter()
        .enumerate()
        .map(|(block, section)| {
            section
                .leaves
                .iter()
                .map(|leaf| ownership.pin_read(block, leaf).unwrap())
                .collect()
        })
        .collect()
}

fn owned(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> PinRead {
    let strings = |parts: &[&str]| parts.iter().map(|part| (*part).to_owned()).collect();
    PinRead::Owned(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: strings(path),
        },
        frame: frame.map(strings),
    })
}

fn ref_owner() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}

#[test]
fn unambiguous_projects_keep_authoring_legacy_pins_and_collections() {
    let mut rows = value_group("Rows");
    rows.repeating = true;
    let project = project(
        SchemaNode::group("Input", vec![value_group("Order"), rows.clone()]),
        vec![named("Ref", SchemaNode::group("Doc", vec![rows]))],
    );
    let ownership = SourceOwnership::new(&project.source, &project.extra_sources);
    assert!(!ownership.active());
    assert!(
        reads(&project)
            .iter()
            .flatten()
            .all(|read| *read == PinRead::Legacy)
    );
    assert!(ownership.connection_error(1).is_none());
}

#[test]
fn primary_shadowing_makes_the_named_alias_owned_and_primary_alias_paths_owned() {
    let mut rows = value_group("Rows");
    rows.repeating = true;
    let primary = SchemaNode::group(
        "Input",
        vec![
            value_group("Ref"),
            SchemaNode::scalar("Other", ScalarType::Int),
        ],
    );
    let project = project(
        primary,
        vec![named(
            "Ref",
            SchemaNode::group("Doc", vec![value_group("Head"), rows]),
        )],
    );
    assert!(SourceOwnership::new(&project.source, &project.extra_sources).active());
    assert_eq!(
        reads(&project),
        [
            vec![
                owned(SourceOwner::Primary, &["Ref", "Value"], None),
                PinRead::Legacy,
            ],
            vec![
                owned(ref_owner(), &["Head", "Value"], None),
                owned(ref_owner(), &["Rows", "Value"], Some(&["Rows"])),
            ],
        ]
    );
}

#[test]
fn repeating_named_roots_pin_the_root_frame_and_offer_owned_root_iteration() {
    let mut rows = value_group("Rows");
    rows.repeating = true;
    let project = project(value_group("Ref"), vec![named("Ref", rows)]);
    // A named schema containing its own alias shadows itself conservatively.
    let mut shadowing = project.clone();
    shadowing.source = SchemaNode::group("Input", vec![value_group("Ref")]);
    let ownership = SourceOwnership::new(&shadowing.source, &shadowing.extra_sources);
    assert_eq!(
        reads(&shadowing)[1],
        [owned(ref_owner(), &["Value"], Some(&[]))]
    );
    let labels = ownership
        .collections()
        .iter()
        .map(|choice| choice.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["primary: <rows>", "Ref: <rows>"]);
    assert!(!SourceOwnership::new(&project.source, &project.extra_sources).active());
}

#[test]
fn existing_ownership_makes_every_addressable_pin_owned() {
    let mut project = project(
        value_group("Input"),
        vec![named("Lookup", value_group("Doc"))],
    );
    assert_eq!(
        reads(&project),
        [vec![PinRead::Legacy], vec![PinRead::Legacy]]
    );
    project.graph.nodes.insert(
        3,
        mapping::Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
    );
    assert_eq!(
        reads(&project),
        [
            vec![owned(SourceOwner::Primary, &["Value"], None)],
            vec![owned(
                SourceOwner::Named {
                    name: "Lookup".into()
                },
                &["Value"],
                None
            )],
        ]
    );
    let ownership =
        SourceOwnership::new(&project.source, &project.extra_sources).with_project_usage(true);
    assert!(ownership.palette_position().is_some());
    assert!(
        SourceOwnership::new(&project.source, &project.extra_sources)
            .palette_position()
            .is_none()
    );
}

#[test]
fn duplicate_and_dynamic_shadowed_aliases_stay_unavailable() {
    let primary = SchemaNode::group("Input", vec![value_group("Ref")]);
    let duplicate = project(
        primary.clone(),
        vec![
            named("Ref", value_group("A")),
            named(" Ref", value_group("B")),
        ],
    );
    let ownership = SourceOwnership::new(&duplicate.source, &duplicate.extra_sources);
    for block in [1, 2] {
        assert!(!ownership.block_reads_owned(block));
        let error = ownership.connection_error(block).unwrap();
        assert!(error.contains("another named input"), "{error}");
    }
    let mut dynamic = named("Ref", value_group("A"));
    dynamic.dynamic_path = Some(mapping::DynamicSourcePath::legacy(0, vec![]));
    let dynamic = project(primary, vec![dynamic]);
    let ownership = SourceOwnership::new(&dynamic.source, &dynamic.extra_sources);
    assert!(
        ownership
            .connection_error(1)
            .unwrap()
            .contains("primary input")
    );
    assert_eq!(ownership.collections().len(), 1);
}

#[test]
fn owned_dynamic_inputs_are_unaddressable_and_keep_their_driver() {
    let primary = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Files",
                vec![SchemaNode::scalar("Path", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let driver = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: vec!["Files".into()],
    };
    let mut docs = named("Docs", value_group("Doc"));
    docs.dynamic_path = Some(mapping::DynamicSourcePath::owned(4, driver.clone()));
    let mut legacy = named("Legacy", value_group("Doc"));
    legacy.dynamic_path = Some(mapping::DynamicSourcePath::legacy(4, vec!["Files".into()]));
    let project = project(primary, vec![docs, legacy]);
    let ownership =
        SourceOwnership::new(&project.source, &project.extra_sources).with_project_usage(true);
    let error = ownership.connection_error(1).unwrap();
    assert!(error.contains("owned dynamic driver"), "{error}");
    assert!(!ownership.block_reads_owned(1));
    assert!(ownership.connection_error(2).is_none());
    let docs_owner = SourceOwner::Named {
        name: "Docs".into(),
    };
    let saved = ownership.owned_dynamic_driver(&docs_owner).unwrap();
    assert_eq!(saved.node, 4);
    assert_eq!(saved.owned_driver(), Some(&driver));
    assert!(
        ownership
            .owned_dynamic_driver(&SourceOwner::Named {
                name: "Legacy".into()
            })
            .is_none()
    );
    assert!(
        ownership
            .owned_dynamic_driver(&SourceOwner::Primary)
            .is_none()
    );
    // Neither dynamic endpoint is offered as an owned collection choice.
    assert!(
        ownership
            .collections()
            .iter()
            .all(|choice| choice.source.owner == SourceOwner::Primary)
    );
}

#[test]
fn lookup_owners_include_shadowed_static_aliases_but_not_ambiguous_or_dynamic_inputs() {
    let primary = SchemaNode::group("Root", vec![value_group("Ref")]);
    let mut dynamic = named("Dynamic", value_group("Doc"));
    dynamic.dynamic_path = Some(mapping::DynamicSourcePath::legacy(3, vec![]));
    let sources = vec![
        named("Ref", value_group("Doc")),
        dynamic,
        named("duplicate", value_group("A")),
        named("duplicate", value_group("B")),
        named(" ", value_group("Blank")),
    ];
    assert_eq!(
        SourceOwnership::new(&primary, &sources).lookup_owners(),
        [SourceOwner::Primary, ref_owner()]
    );
}

#[test]
fn lookup_editing_refuses_shared_specialized_contexts_without_changing_nodes() {
    use mapping::{Binding, Node, Scope, ScopeConstruction};
    let mut project = project(value_group("Input"), vec![]);
    project.graph.nodes.insert(
        3,
        Node::Lookup {
            collection: vec![],
            key: vec!["Value".into()],
            matches: 4,
            value: vec!["Value".into()],
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: ir::Value::String("key".into()),
        },
    );
    project.graph.nodes.insert(
        5,
        Node::Call {
            function: "concat".into(),
            args: vec![3],
        },
    );
    project.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 3,
    }];
    assert!(!lookup_read_only_nodes(&project).contains(&3));
    project.extra_targets.push(mapping::NamedTarget {
        name: "Special".into(),
        path: None,
        schema: value_group("Output"),
        options: Default::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 5,
            }],
            ..Default::default()
        },
    });
    let before = serde_json::to_value(&project).unwrap();
    assert!(lookup_read_only_nodes(&project).contains(&3));
    assert_eq!(serde_json::to_value(&project).unwrap(), before);
    project.extra_targets.clear();
    project.graph.nodes.insert(
        6,
        Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection: vec![],
            value: vec![],
            expression: Some(5),
            arg: None,
        },
    );
    assert!(lookup_read_only_nodes(&project).contains(&3));
}
