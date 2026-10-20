//! Owned adjacency-tree construction: the row collection belongs to one exact
//! endpoint, key/parent stay row-relative, and the root expression keeps the
//! receiving context. Target key/children names only describe the output.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AdjacencyTreePlan, Graph, NamedSource, Node, OwnedAdjacencyTreePlan, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr, SourceOwner,
};
use serde_json::{Value as Json, json};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

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

fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            string("Key"),
            SchemaNode::group("Link", vec![string("Parent")]),
        ],
    )
    .repeating()
}

/// Every endpoint declares the same shape, so only the owner distinguishes rows.
fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Root"),
            SchemaNode::group("Rows", vec![row_schema()]),
            SchemaNode::group("Batch", vec![string("Start"), row_schema()]).repeating(),
        ],
    )
}

fn node(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("name"),
            SchemaNode::recursive_group("children", name).repeating(),
        ],
    )
}

fn tree(owner: SourceOwner, rows: &[&str], root: Option<u32>) -> Scope {
    let plan = OwnedAdjacencyTreePlan::new(
        OwnedSourceRef {
            owner,
            path: path(rows),
        },
        path(&["Key"]),
        path(&["Link", "Parent"]),
        "name".into(),
        "children".into(),
        root,
    )
    .expect("authored owned plan");
    Scope {
        construction: ScopeConstruction::OwnedAdjacencyTree { plan },
        ..Scope::default()
    }
}

fn project(root: Scope) -> Project {
    Project {
        source: input_schema(),
        target: node("node"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "audit".into(),
            path: "audit.json".into(),
            schema: input_schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root,
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

fn encode(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(Value::String(value)) => json!(value),
        Instance::Repeated(items) => Json::Array(items.iter().map(encode).collect()),
        Instance::Group(fields) => Json::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), encode(value)))
                .collect(),
        ),
        other => json!(format!("{other:?}")),
    }
}

fn row(key: &str, parent: Option<&str>) -> Json {
    json!({"Key": key, "Link": {"Parent": parent}})
}

fn input(root: &str, rows: Vec<Json>) -> Json {
    json!({"Root": root, "Rows": {"Row": rows}, "Batch": []})
}

fn run(project: &Project, source: Json, audit: Option<Json>) -> Result<Json, EngineError> {
    let inputs = audit
        .map(|audit| vec![("audit".to_string(), decode(&audit))])
        .unwrap_or_default();
    crate::run_with_sources(project, &decode(&source), inputs).map(|output| encode(&output))
}

fn leaf(name: &str) -> Json {
    json!({"name": name, "children": []})
}

#[test]
fn same_names_in_other_endpoints_never_merge_or_supply_parents() {
    // Both endpoints use the keys `a` and `b`; only the selected rows appear.
    let primary = input("p", vec![row("a", None), row("b", Some("a"))]);
    let audit = input(
        "n",
        vec![row("b", None), row("a", Some("b")), row("c", Some("a"))],
    );
    let selected = project(tree(owner(None), &["Rows", "Row"], None));
    assert!(crate::validate(&selected).is_empty());
    assert_eq!(
        run(&selected, primary.clone(), Some(audit.clone())),
        Ok(json!({"name": "a", "children": [leaf("b")]}))
    );
    let named = project(tree(owner(Some("audit")), &["Rows", "Row"], None));
    assert_eq!(
        run(&named, primary.clone(), Some(audit)),
        Ok(json!({"name": "b", "children": [
            {"name": "a", "children": [leaf("c")]}
        ]}))
    );
    // An unsupplied endpoint is never replaced by the primary rows.
    assert!(matches!(
        run(&named, primary, None),
        Err(EngineError::OwnedSourceAccess {
            source: OwnedSourceError::SourceUnavailable { .. },
            ..
        })
    ));
}

#[test]
fn root_expression_keeps_the_receiving_context() {
    // The rows belong to `audit`, while the root reads the primary Root field.
    let mut project = project(tree(owner(Some("audit")), &["Rows", "Row"], Some(1)));
    project.graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner(None),
                path: path(&["Root"]),
            },
            frame: None,
        }),
    );
    assert!(crate::validate(&project).is_empty());
    let primary = input("x", vec![row("x", None)]);
    let audit = input(
        "y",
        vec![row("r", None), row("x1", Some("x")), row("y1", Some("y"))],
    );
    assert_eq!(
        run(&project, primary.clone(), Some(audit.clone())),
        Ok(leaf("x1"))
    );
    // A legacy root expression resolves in the same parent frame.
    project.graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Root"]),
            frame: None,
        },
    );
    assert_eq!(run(&project, primary, Some(audit)), Ok(leaf("x1")));
}

#[test]
fn active_same_owner_batches_pin_their_rows_and_parent_root() {
    let batch_tree = |name: &str, owner: SourceOwner, root: Option<u32>| Scope {
        target_field: name.into(),
        ..tree(owner, &["Batch", "Row"], root)
    };
    let mut project = project(Scope {
        children: vec![Scope {
            target_field: "Batch".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner(None),
                    path: path(&["Batch"]),
                },
                output_path: None,
            },
            children: vec![
                batch_tree("Tree", owner(None), Some(1)),
                batch_tree("Audit", owner(Some("audit")), None),
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    });
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Batch", vec![node("Tree"), node("Audit")]).repeating()],
    );
    project.graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner(None),
                path: path(&["Batch", "Start"]),
            },
            frame: Some(path(&["Batch"])),
        }),
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    let primary = json!({"Root": "-", "Rows": {"Row": []}, "Batch": [
        {"Start": "s1", "Row": [row("a", None), row("b", Some("s1"))]},
        {"Start": "s2", "Row": [row("a", Some("s2")), row("c", Some("a"))]},
    ]});
    // The other endpoint's tree is complete: its batches are flattened.
    let audit = json!({"Root": "-", "Rows": {"Row": []}, "Batch": [
        {"Start": "-", "Row": [row("t", None)]},
        {"Start": "-", "Row": [row("u", Some("t"))]},
    ]});
    let other = json!({"name": "t", "children": [leaf("u")]});
    assert_eq!(
        run(&project, primary, Some(audit)),
        Ok(json!({"Batch": [
            {"Tree": leaf("b"), "Audit": other},
            {"Tree": {"name": "a", "children": [leaf("c")]}, "Audit": other},
        ]}))
    );
}

#[test]
fn row_errors_root_errors_and_bounds_match_the_legacy_builder() {
    let mut owned = project(tree(owner(None), &["Rows", "Row"], Some(1)));
    owned.graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Root"]),
            frame: None,
        },
    );
    let mut legacy = owned.clone();
    legacy.root.construction = ScopeConstruction::AdjacencyTree {
        plan: AdjacencyTreePlan::new(
            path(&["Rows", "Row"]),
            path(&["Key"]),
            path(&["Link", "Parent"]),
            "name".into(),
            "children".into(),
            Some(1),
        )
        .unwrap(),
    };
    let chain = std::iter::once(row("n0", Some("top")))
        .chain((1..=256).map(|index| row(&format!("n{index}"), Some(&format!("n{}", index - 1)))))
        .collect::<Vec<_>>();
    let cases = [
        (
            input("top", vec![row("a", Some("top")), row("a", Some("top"))]),
            EngineError::DuplicateAdjacencyKey("a".into()),
        ),
        (
            input("l", vec![row("l", Some("l"))]),
            EngineError::AdjacencyCycle("l".into()),
        ),
        (
            input("top", vec![json!({"Link": {"Parent": "top"}})]),
            EngineError::InvalidAdjacencyField {
                role: "key",
                path: "Key".into(),
                found: "missing value",
            },
        ),
        (
            input("top", vec![json!({"Key": "a", "Link": {"Parent": true}})]),
            EngineError::InvalidAdjacencyField {
                role: "parent",
                path: "Link/Parent".into(),
                found: "bool",
            },
        ),
        (
            json!({"Root": 7, "Rows": {"Row": [row("a", None)]}}),
            EngineError::InvalidAdjacencyRoot { found: "int" },
        ),
        (
            input("top", vec![row("a", Some("top")), row("b", Some("top"))]),
            EngineError::AdjacencyRootCount { count: 2 },
        ),
        (
            input("top", vec![]),
            EngineError::AdjacencyRootCount { count: 0 },
        ),
        (
            input("top", chain),
            EngineError::AdjacencyTreeDepth { limit: 256 },
        ),
    ];
    for (source, expected) in cases {
        let actual = run(&owned, source.clone(), None);
        assert_eq!(run(&legacy, source, None), actual);
        assert_eq!(actual, Err(expected));
    }
    // Only the legacy resolver reports an unresolved collection; an owned
    // endpoint without rows has no selected root.
    let missing = json!({"Root": "top"});
    assert_eq!(
        run(&legacy, missing.clone(), None),
        Err(EngineError::MissingAdjacencyCollection("Rows/Row".into()))
    );
    assert_eq!(
        run(&owned, missing, None),
        Err(EngineError::AdjacencyRootCount { count: 0 })
    );
}

#[test]
fn endpoint_root_collections_and_validation_use_the_owner_schema() {
    let mut project = project(tree(owner(Some("rows")), &[], None));
    project.extra_sources.push(NamedSource {
        name: "rows".into(),
        path: "rows.json".into(),
        schema: row_schema(),
        options: Default::default(),
        dynamic_path: None,
    });
    assert!(crate::validate(&project).is_empty());
    let rows = json!([row("r", None), row("s", Some("r"))]);
    assert_eq!(
        crate::run_with_sources(
            &project,
            &decode(&input("-", vec![])),
            vec![("rows".into(), decode(&rows))]
        )
        .map(|output| encode(&output)),
        Ok(json!({"name": "r", "children": [leaf("s")]}))
    );

    let issues = |scope: Scope| {
        let mut project = self::project(scope);
        project.extra_sources[0].schema = SchemaNode::group("Input", vec![string("Root")]);
        crate::validate(&project)
            .into_iter()
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
    };
    // The primary declaration never supplies the named endpoint's schema.
    assert!(
        issues(tree(owner(Some("audit")), &["Rows", "Row"], None))
            .iter()
            .any(|message| message.contains("owned adjacency-tree collection `Rows/Row`"))
    );
    assert!(
        issues(tree(owner(Some("missing")), &["Rows", "Row"], None))
            .iter()
            .any(|message| message.contains("unknown source owner `missing`"))
    );
    assert!(
        issues(tree(owner(None), &["Root"], None))
            .iter()
            .any(|message| message.contains("must be a repeating group in its owner"))
    );
    let mut wrong_key = tree(owner(None), &["Rows", "Row"], Some(9));
    let ScopeConstruction::OwnedAdjacencyTree { plan } = &mut wrong_key.construction else {
        unreachable!();
    };
    *plan = OwnedAdjacencyTreePlan::new(
        plan.collection().clone(),
        path(&["Link"]),
        path(&["Nope"]),
        "name".into(),
        "children".into(),
        Some(9),
    )
    .unwrap();
    let messages = issues(wrong_key);
    for expected in [
        "adjacency-tree key field `Link`",
        "adjacency-tree parent field `Nope`",
        "adjacency-tree root references missing node 9",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
}

#[test]
fn closed_contexts_refuse_owned_rows() {
    // A generated item context closes the owned read inside it.
    let mut project = project(Scope {
        children: vec![Scope {
            target_field: "Item".into(),
            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                input: 0,
                delimiter: 1,
                item: 2,
            }),
            children: vec![Scope {
                target_field: "Tree".into(),
                ..tree(owner(None), &["Rows", "Row"], None)
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    });
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Item", vec![node("Tree")]).repeating()],
    );
    project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    assert!(crate::validate(&project).is_empty());
    assert!(matches!(
        run(&project, input("r", vec![row("r", None)]), None),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::GeneratedItem,
            ..
        })
    ));

    // A dynamic named owner never supplies exact owned rows.
    let mut dynamic = self::project(tree(owner(Some("audit")), &["Rows", "Row"], None));
    dynamic.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("audit.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 0,
        iteration: vec!["Batch".into()],
    });
    assert!(matches!(
        run(&dynamic, input("r", vec![row("r", None)]), None),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::DynamicOwner,
            ..
        })
    ));
}
