//! Genuine public Project definitions for owned occurrence positions. Authored
//! source only: no mapping, position or occurrence is evaluated here.
use super::*;
use mapping::{NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

pub const NAMED: &str = "Ref";

/// Model order is the `model` index the generated hosts receive.
pub const PROGRAMS: [&str; 2] = ["primary", "named"];

fn owner(program: &str) -> SourceOwner {
    match program {
        "primary" => SourceOwner::Primary,
        "named" => SourceOwner::Named { name: NAMED.into() },
        _ => panic!("authored program"),
    }
}

fn endpoint() -> SchemaNode {
    SchemaNode::group("Input", vec![string("Value")])
}

/// Both programs iterate one owned endpoint and bind its `Value` and its
/// 1-based occurrence position. The named program's primary source carries a
/// colliding `Value` and `Ref` so only the owner selects the iterated rows.
pub fn project(program: &str) -> Project {
    let owned = owner(program);
    let mut graph = Graph::default();
    graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owned.clone(),
                path: vec!["Value".into()],
            },
            frame: Some(vec![]),
        }),
    );
    graph.nodes.insert(
        2,
        Node::OwnedPosition(OwnedPosition {
            owner: owned.clone(),
            collection: Some(vec![]),
        }),
    );
    let (source, extra_sources) = match program {
        "primary" => (endpoint(), vec![]),
        _ => (
            SchemaNode::group(
                "Primary",
                vec![
                    string("Value"),
                    SchemaNode::group(NAMED, vec![string("Value")]),
                ],
            ),
            vec![NamedSource {
                name: NAMED.into(),
                path: "unused-basic-occurrence-input.json".into(),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: None,
            }],
        ),
    };
    Project {
        source,
        target: SchemaNode::group("Output", vec![string("Value"), int("Position")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources,
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph,
        root: Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owned,
                    path: vec![],
                },
                output_path: None,
            },
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    }
}
