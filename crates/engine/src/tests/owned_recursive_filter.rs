//! Owned recursive filtering: the scope's owned selector is the filter's only
//! owner. Items and child groups keep that owner and a same-shape collection
//! identity, so owned predicate reads and positions never fall back elsewhere.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Graph, IterationOutput, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef,
    Project, RecursiveFilterPlan, Scope, ScopeConstruction, ScopeIteration, SequenceExpr,
    SourceOwner,
};
use serde_json::{Value as Json, json};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

const PREDICATE: u32 = 20;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn directory() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            string("name"),
            SchemaNode::group(
                "file",
                vec![
                    string("name"),
                    SchemaNode::scalar("pos", ScalarType::Int),
                    SchemaNode::scalar("dirpos", ScalarType::Int),
                    string("dir"),
                    string("batch"),
                ],
            )
            .repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}

/// Primary and `Lookup` share this shape, so only the owner tells them apart.
fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Suffix"),
            directory().repeating(),
            SchemaNode::group(
                "Batches",
                vec![
                    string("Id"),
                    SchemaNode::recursive_group("Dir", "Dir").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn field(owner: &SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner.clone(),
            path: path(parts),
        },
        frame: None,
    })
}

fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

fn filter(owner: &SourceOwner, collection: &[&str]) -> Scope {
    Scope {
        target_field: "Dir".into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: owner.clone(),
                path: path(collection),
            },
            output_path: None,
        },
        construction: ScopeConstruction::RecursiveFilter {
            plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), PREDICATE)
                .expect("authored plan"),
        },
        ..Scope::default()
    }
}

/// Keeps files whose own name ends with the selected owner's root suffix.
fn suffix_project(selected: &SourceOwner) -> Project {
    let mut graph = Graph::default();
    graph.nodes.extend([
        (10, field(selected, &["Dir", "file", "name"])),
        (11, field(selected, &["Suffix"])),
        (PREDICATE, call("ends_with", &[10, 11])),
    ]);
    project(graph, filter(selected, &["Dir"]))
}

fn project(graph: Graph, scope: Scope) -> Project {
    Project {
        source: input_schema(),
        target: SchemaNode::group("Out", vec![directory().repeating()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: input_schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![scope],
            ..Scope::default()
        },
    }
}

fn decode(value: &Json) -> Instance {
    match value {
        Json::Null => Instance::Scalar(Value::Null),
        Json::Bool(value) => Instance::Scalar(Value::Bool(*value)),
        Json::Number(value) => Instance::Scalar(Value::Int(value.as_i64().expect("integer"))),
        Json::String(value) => Instance::Scalar(Value::String(value.clone())),
        Json::Array(items) => Instance::Repeated(items.iter().map(decode).collect()),
        Json::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), decode(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

fn run(project: &Project, primary: &Json, lookup: Option<&Json>) -> Result<Instance, EngineError> {
    let inputs = lookup
        .map(|lookup| vec![("Lookup".to_string(), decode(lookup))])
        .unwrap_or_default();
    crate::run_with_sources(project, &decode(primary), inputs)
}

fn file(name: &str) -> Json {
    json!({"name": name})
}

fn primary() -> Json {
    json!({"Suffix": ".keep", "Dir": [
        {"name": "p1", "file": [file("a.keep"), file("b.lk")], "Dir": [
            {"name": "p1.keep", "file": [file("c.keep"), {"pos": 2}], "Dir": []},
        ]},
        {"name": "p2", "file": [], "Dir": [{"name": "p3", "file": [file("d.lk")]}]},
    ], "Batches": []})
}

fn lookup() -> Json {
    json!({"Suffix": ".lk", "Dir": [
        {"name": "l1", "file": [file("a.keep"), file("b.lk")], "Dir": [
            // A missing item name never falls back to its group's matching name.
            {"name": "x.lk", "file": [{"pos": 1}, file("e.lk")]},
        ]},
    ], "Batches": []})
}

#[test]
fn colliding_owners_produce_complete_structures_from_the_selected_owner_only() {
    let expected_primary = json!({"Dir": [
        {"name": "p1", "file": [file("a.keep")], "Dir": [
            {"name": "p1.keep", "file": [file("c.keep")], "Dir": []},
        ]},
        {"name": "p2", "file": [], "Dir": [{"name": "p3", "file": []}]},
    ]});
    let project = suffix_project(&owner(None));
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    assert_eq!(
        run(&project, &primary(), Some(&lookup())),
        Ok(decode(&expected_primary))
    );
    let expected_lookup = json!({"Dir": [
        {"name": "l1", "file": [file("b.lk")], "Dir": [
            {"name": "x.lk", "file": [file("e.lk")]},
        ]},
    ]});
    let project = suffix_project(&owner(Some("Lookup")));
    assert!(crate::validate(&project).is_empty());
    assert_eq!(
        run(&project, &primary(), Some(&lookup())),
        Ok(decode(&expected_lookup))
    );

    // The legacy filter over the same data resolves names innermost-first and
    // falls back outward, so the nameless Lookup item reads its group's name.
    let mut legacy = suffix_project(&owner(None));
    legacy.root.children[0].iteration = ScopeIteration::Source(path(&["Lookup", "Dir"]));
    legacy.graph.nodes.extend([
        (
            10,
            Node::SourceField {
                path: path(&["name"]),
                frame: None,
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String(".lk".into()),
            },
        ),
    ]);
    let Ok(Instance::Group(fields)) = run(&legacy, &primary(), Some(&lookup())) else {
        panic!("legacy filter succeeds");
    };
    let nested = &fields[0].1.as_repeated().unwrap()[0]
        .field("Dir")
        .and_then(Instance::as_repeated)
        .unwrap()[0];
    assert_eq!(
        nested
            .field("file")
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(2)
    );
}

#[test]
fn unsupplied_and_empty_owners_never_borrow_another_endpoint() {
    let project = suffix_project(&owner(Some("Lookup")));
    assert_eq!(
        run(&project, &primary(), None),
        Err(EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(
                mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
                    .descendant(mapping::SourceScopeStep::Child(0))
            )),
            source: OwnedSourceError::SourceUnavailable {
                owner: owner(Some("Lookup")),
            },
        })
    );
    let empty = json!({"Suffix": ".lk", "Dir": []});
    assert_eq!(
        run(&project, &primary(), Some(&empty)),
        Ok(decode(&json!({"Dir": []})))
    );
}

/// Every item checks its own position, its group's position and its group's
/// name; authored expectations that disagree drop the item.
#[test]
fn positions_and_group_reads_follow_each_recursive_level() {
    let selected = owner(Some("Lookup"));
    let mut graph = Graph::default();
    graph.nodes.extend([
        (
            10,
            Node::OwnedPosition(OwnedPosition {
                owner: selected.clone(),
                collection: Some(path(&["Dir", "file"])),
            }),
        ),
        (11, field(&selected, &["Dir", "file", "pos"])),
        (12, call("equal", &[10, 11])),
        (
            13,
            Node::OwnedPosition(OwnedPosition {
                owner: selected.clone(),
                collection: Some(path(&["Dir"])),
            }),
        ),
        (14, field(&selected, &["Dir", "file", "dirpos"])),
        (15, call("equal", &[13, 14])),
        (16, field(&selected, &["Dir", "name"])),
        (17, field(&selected, &["Dir", "file", "dir"])),
        (18, call("equal", &[16, 17])),
        // The latest frame of the owner is the current item.
        (
            19,
            Node::OwnedPosition(OwnedPosition {
                owner: selected.clone(),
                collection: None,
            }),
        ),
        (21, call("equal", &[19, 11])),
        (PREDICATE, call("and", &[12, 15, 18, 21])),
    ]);
    let project = project(graph, filter(&selected, &["Dir"]));
    assert!(crate::validate(&project).is_empty());
    let item = |pos: i64, dirpos: i64, dir: &str| json!({"pos": pos, "dirpos": dirpos, "dir": dir});
    let lookup = json!({"Suffix": "-", "Dir": [
        {"name": "a", "file": [item(1, 1, "a"), item(9, 1, "a"), item(3, 1, "a")], "Dir": [
            {"name": "b", "file": [item(1, 1, "b"), item(2, 9, "b")], "Dir": [
                {"name": "c", "file": [item(1, 1, "a"), item(2, 1, "c")], "Dir": []},
            ]},
            {"name": "d", "file": [item(1, 2, "d")]},
        ]},
        {"name": "e", "file": [item(1, 2, "e"), item(2, 1, "e")]},
    ]});
    let expected = json!({"Dir": [
        {"name": "a", "file": [item(1, 1, "a"), item(3, 1, "a")], "Dir": [
            {"name": "b", "file": [item(1, 1, "b")], "Dir": [
                {"name": "c", "file": [item(2, 1, "c")], "Dir": []},
            ]},
            {"name": "d", "file": [item(1, 2, "d")]},
        ]},
        {"name": "e", "file": [item(1, 2, "e")]},
    ]});
    assert_eq!(
        run(&project, &primary(), Some(&lookup)),
        Ok(decode(&expected))
    );
}

/// Nested owned frames survive into the predicate: the outer batch frame
/// supplies its Id, and the filter walks the batch's own directories.
#[test]
fn nested_owned_frames_reach_every_recursive_predicate() {
    let selected = owner(None);
    let mut graph = Graph::default();
    graph.nodes.extend([
        (10, field(&selected, &["Batches", "Id"])),
        (11, field(&selected, &["Batches", "Dir", "file", "batch"])),
        (PREDICATE, call("equal", &[10, 11])),
    ]);
    let mut inner = filter(&selected, &["Batches", "Dir"]);
    inner.target_field = "Dir".into();
    let outer = Scope {
        target_field: "Batch".into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: selected.clone(),
                path: path(&["Batches"]),
            },
            output_path: None,
        },
        children: vec![inner],
        ..Scope::default()
    };
    let mut project = project(graph, outer);
    project.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::group("Batch", vec![directory().repeating()]).repeating()],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    let batch = |id: &str| json!({"batch": id});
    let source = json!({"Suffix": "", "Dir": [], "Batches": [
        {"Id": "b1", "Dir": [{"name": "x", "file": [batch("b1"), batch("b2")], "Dir": [
            {"name": "y", "file": [batch("b2"), batch("b1")]},
        ]}]},
        {"Id": "b2", "Dir": [{"name": "z", "file": [batch("b1"), batch("b2")], "Dir": []}]},
    ]});
    let expected = json!({"Batch": [
        {"Dir": [{"name": "x", "file": [batch("b1")], "Dir": [{"name": "y", "file": [batch("b1")]}]}]},
        {"Dir": [{"name": "z", "file": [batch("b2")], "Dir": []}]},
    ]});
    assert_eq!(run(&project, &source, None), Ok(decode(&expected)));
}

#[test]
fn malformed_children_predicates_and_depth_keep_legacy_errors() {
    let project = suffix_project(&owner(None));
    let with = |dir: Json| json!({"Suffix": ".keep", "Dir": [dir]});
    assert_eq!(
        run(
            &project,
            &with(json!({"name": "a", "file": [], "Dir": [{"name": "b", "Dir": "bad"}]})),
            None
        ),
        Err(EngineError::RecursiveFilterRequiresCollection {
            field: "Dir".into(),
            found: "scalar",
        })
    );
    assert_eq!(
        run(
            &project,
            &with(json!({"name": "a", "file": {"name": "x"}})),
            None
        ),
        Err(EngineError::RecursiveFilterRequiresCollection {
            field: "file".into(),
            found: "group",
        })
    );
    assert_eq!(
        run(
            &project,
            &with(json!({"name": "a", "Dir": [{"name": "b", "Dir": ["leaf"]}]})),
            None
        ),
        Err(EngineError::RecursiveFilterRequiresGroup { found: "scalar" })
    );

    let mut not_bool = suffix_project(&owner(None));
    not_bool
        .graph
        .nodes
        .insert(PREDICATE, field(&owner(None), &["Dir", "file", "name"]));
    assert_eq!(
        run(
            &not_bool,
            &with(json!({"name": "a", "file": [file("x")]})),
            None
        ),
        Err(EngineError::NotABool {
            node: PREDICATE,
            found: "string",
        })
    );

    // Owned reads of another owner's repetition never borrow this item.
    let mut foreign = suffix_project(&owner(None));
    foreign
        .graph
        .nodes
        .insert(10, field(&owner(Some("Lookup")), &["Dir", "file", "name"]));
    assert!(matches!(
        run(
            &foreign,
            &with(json!({"name": "a", "file": [file("x")]})),
            Some(&lookup())
        ),
        Err(EngineError::OwnedSourceAccess {
            node: Some(10),
            source: OwnedSourceError::UniteratedOwnedRepetition { .. },
            ..
        })
    ));

    let nest = |levels: usize| {
        let mut dir = json!({"name": "leaf", "file": [file("x.keep")]});
        for _ in 0..levels {
            dir = json!({"name": "n", "Dir": [dir]});
        }
        with(dir)
    };
    assert!(run(&project, &nest(255), None).is_ok());
    assert_eq!(
        run(&project, &nest(256), None),
        Err(EngineError::RecursiveFilterDepth { limit: 256 })
    );
}

#[test]
fn ordered_xml_metadata_follows_the_owned_filter_like_the_legacy_one() {
    use ir::{XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD};
    let entry = |name: &str, value: Json| json!({XML_NODE_NAME_FIELD: name, XML_MIXED_CONTENT_VALUE_FIELD: value});
    let nested = json!({"name": "n", "file": [file("k.keep"), file("d.txt")], "Dir": [],
        XML_MIXED_CONTENT_FIELD: [entry("file", file("k.keep")), entry("file", file("d.txt"))]});
    let source = json!({"Suffix": ".keep", "Dir": [{"name": "r", "file": [file("d.txt"), file("r.keep")],
        "Dir": [nested],
        XML_MIXED_CONTENT_FIELD: [entry("file", file("d.txt")), entry("Dir", json!(null)),
            entry("file", file("r.keep"))]}]});
    let owned = run(&suffix_project(&owner(None)), &source, None);
    let mut legacy = suffix_project(&owner(None));
    legacy.root.children[0].iteration = ScopeIteration::Source(path(&["Dir"]));
    legacy.graph.nodes.extend([
        (
            10,
            Node::SourceField {
                path: path(&["name"]),
                frame: None,
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String(".keep".into()),
            },
        ),
    ]);
    assert_eq!(owned, run(&legacy, &source, None));
    let Ok(Instance::Group(fields)) = owned else {
        panic!("owned filter succeeds");
    };
    let root = &fields[0].1.as_repeated().unwrap()[0];
    let ordered = root
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(ordered.len(), 2);
}

#[test]
fn closed_contexts_and_legacy_islands_keep_refusing() {
    // Mapped-sequence output is an XML operator, never a Basic filter.
    let mut mapped = suffix_project(&owner(None));
    mapped.root.children[0].iteration_output = IterationOutput::MappedSequence;
    assert!(matches!(
        run(&mapped, &primary(), None),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::XmlOperator,
            ..
        })
    ));

    // A generated item parent closes the owned filter inside it.
    let mut generated = suffix_project(&owner(None));
    let inner = generated.root.children.remove(0);
    generated.root.children.push(Scope {
        target_field: "Item".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
            input: 30,
            delimiter: 31,
            item: 32,
        }),
        children: vec![inner],
        ..Scope::default()
    });
    generated.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::group("Item", vec![directory().repeating()]).repeating()],
    );
    generated.graph.nodes.extend([
        (
            30,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            31,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            32,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    assert!(crate::validate(&generated).is_empty());
    assert!(matches!(
        run(&generated, &primary(), None),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::GeneratedItem,
            ..
        })
    ));

    // A legacy filter under owned ancestry stays a closed tree operator.
    let mut island = suffix_project(&owner(None));
    let mut legacy = island.root.children.remove(0);
    legacy.iteration = ScopeIteration::Source(path(&["Batches", "Dir"]));
    island.root.children.push(Scope {
        target_field: "Batch".into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: owner(None),
                path: path(&["Batches"]),
            },
            output_path: None,
        },
        children: vec![legacy],
        ..Scope::default()
    });
    island.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::group("Batch", vec![directory().repeating()]).repeating()],
    );
    assert!(matches!(
        run(&island, &primary(), None),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::TreeOperator,
            ..
        })
    ));

    // A dynamic named owner never supplies the filtered groups.
    let mut dynamic = suffix_project(&owner(Some("Lookup")));
    dynamic.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::String("lookup.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(30, vec!["Dir".into()]));
    assert!(matches!(
        run(&dynamic, &primary(), Some(&lookup())),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::DynamicOwner,
            ..
        })
    ));
}
