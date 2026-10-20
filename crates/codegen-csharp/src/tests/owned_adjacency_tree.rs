//! Emission coverage for the explicit owned adjacency-tree companion.

use super::*;
use ::mapping::{
    NamedSource, NamedTarget, OwnedAdjacencyTreePlan, OwnedSourceField, OwnedSourceRef, Project,
    Scope, ScopeConstruction, SourceOwner,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn node() -> SchemaNode {
    SchemaNode::group(
        "node",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::recursive_group("children", "node").repeating(),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Root", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Row",
                        vec![
                            SchemaNode::scalar("Key", ScalarType::String),
                            SchemaNode::scalar("Parent", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            ),
        ],
    )
}

fn tree(owner: SourceOwner, root: Option<u32>) -> Scope {
    let plan = OwnedAdjacencyTreePlan::new(
        OwnedSourceRef {
            owner,
            path: path(&["Rows", "Row"]),
        },
        path(&["Key"]),
        path(&["Parent"]),
        "name".into(),
        "children".into(),
        root,
    )
    .expect("valid owned plan");
    Scope {
        construction: ScopeConstruction::OwnedAdjacencyTree { plan },
        ..Scope::default()
    }
}

/// The primary tree reads `audit` rows and selects its root from the primary
/// endpoint; the named target reads its own rows without a root.
fn project() -> Project {
    let audit = SourceOwner::Named {
        name: "audit".into(),
    };
    let mut graph = ::mapping::Graph::default();
    graph.nodes.insert(
        4,
        ::mapping::Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Root"]),
            },
            frame: None,
        }),
    );
    Project {
        source: input(),
        target: node(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "audit".into(),
            path: "audit.json".into(),
            schema: input(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: None,
            schema: node(),
            options: Default::default(),
            root: tree(audit.clone(), None),
        }],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: tree(audit, Some(4)),
    }
}

#[test]
fn emits_exact_owner_calls_and_parent_roots_for_each_target() {
    let program = codegen::lower_with_basic_owned_adjacency_tree(&project()).unwrap();
    let artifacts = emit_with_basic_owned_adjacency_tree(&program).unwrap();
    let mapping = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(mapping.contains(
        "return global::Ferrule.Runtime.FerruleAdjacencyTree.BuildOwned(\n            context,\n            global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"audit\"),"
    ));
    assert!(
        mapping
            .contains(",\n            \"name\",\n            \"children\",\n            Node_4);")
    );
    assert!(
        mapping.contains(",\n            \"name\",\n            \"children\",\n            null);")
    );
    assert!(mapping.contains("ExecuteWithBasicOwnedSources("));
}

#[test]
fn older_csharp_entry_points_keep_refusing_owned_trees() {
    let program = codegen::lower_with_basic_owned_adjacency_tree(&project()).unwrap();
    let expected = codegen::validate_program(&program).unwrap_err();
    for result in [
        emit(&program),
        emit_with_basic_owned_sources(&program),
        emit_with_basic_owned_group_ending_with(&program),
        emit_with_basic_owned_failure_sources(&program),
        emit_with_basic_owned_path_hierarchy(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(error)) if error == expected));
    }
}
