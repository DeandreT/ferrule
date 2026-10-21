//! Emission coverage for the explicit owned recursive-filter companion.

use super::*;
use mapping::{
    NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, RecursiveFilterPlan,
    Scope, ScopeConstruction, ScopeIteration, SourceOwner,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn directory() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Suffix", ScalarType::String),
            directory().repeating(),
        ],
    )
}

/// The Lookup filter keeps files ending with the Primary suffix that are the
/// first item of their group.
fn project() -> Project {
    let lookup = SourceOwner::Named {
        name: "Lookup".into(),
    };
    let field = |owner: &SourceOwner, parts: &[&str]| {
        mapping::Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner.clone(),
                path: path(parts),
            },
            frame: None,
        })
    };
    let call = |function: &str, args: Vec<u32>| mapping::Node::Call {
        function: function.into(),
        args,
    };
    let mut graph = mapping::Graph::default();
    graph.nodes.extend([
        (1, field(&lookup, &["Dir", "file", "name"])),
        (2, field(&SourceOwner::Primary, &["Suffix"])),
        (3, call("ends_with", vec![1, 2])),
        (
            4,
            mapping::Node::OwnedPosition(OwnedPosition {
                owner: lookup.clone(),
                collection: Some(path(&["Dir", "file"])),
            }),
        ),
        (
            5,
            mapping::Node::Const {
                value: ir::Value::Int(1),
            },
        ),
        (6, call("equal", vec![4, 5])),
        (7, call("and", vec![3, 6])),
    ]);
    Project {
        source: input(),
        target: SchemaNode::group("Out", vec![directory().repeating()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: input(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Dir".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: lookup,
                        path: path(&["Dir"]),
                    },
                    output_path: None,
                },
                construction: ScopeConstruction::RecursiveFilter {
                    plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), 7)
                        .expect("valid plan"),
                },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn options() -> Options {
    Options {
        package_name: "owned-recursive-filter".into(),
        runtime_dependency: RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn emits_the_owned_filter_call_and_paired_predicate() {
    let program = codegen::lower_with_basic_owned_recursive_filter(&project()).unwrap();
    let artifacts = emit_with_basic_owned_recursive_filter(&program, &options()).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(source.contains(
        "context.walk_owned_source_cohort(SourceOwnerRef::Named(\"Lookup\"), &[\"Dir\"])?"
    ));
    assert!(source.contains(
        "codegen_runtime::owned_recursive_filter(\n            item_context,\n            codegen_runtime::SourceOwnerRef::Named(\"Lookup\"),\n            &[\"Dir\"],\n            \"Dir\",\n            \"file\",\n            7,\n            expression_7,\n        )?;"
    ));
    assert!(source.contains("fn expression_7(context: &BasicScopeContext<'_>)"));
    assert!(!source.contains(" recursive_filter("));
    assert!(source.contains("pub fn execute_with_basic_owned_sources"));
}

#[test]
fn older_rust_entry_points_keep_refusing_owned_filters() {
    let program = codegen::lower_with_basic_owned_recursive_filter(&project()).unwrap();
    let expected = codegen::validate_program(&program).unwrap_err();
    assert!(matches!(
        &expected,
        ProgramValidationError::OwnedSource { diagnostic }
            if matches!(**diagnostic, codegen::OwnedSourceDiagnostic::Capability {
                reason: codegen::OwnedSourceCapabilityReason::TreeOperator,
                ..
            })
    ));
    for result in [
        emit(&program, &options()),
        emit_with_basic_owned_sources(&program, &options()),
        emit_with_basic_owned_group_ending_with(&program, &options()),
        emit_with_basic_owned_group_into_blocks(&program, &options()),
        emit_with_basic_owned_failure_sources(&program, &options()),
        emit_with_basic_owned_path_hierarchy(&program, &options()),
        emit_with_basic_owned_adjacency_tree(&program, &options()),
        emit_with_basic_owned_recursive_collect(&program, &options()),
        emit_with_basic_owned_sequence_exists(&program, &options()),
    ] {
        assert!(matches!(result, Err(EmitError::InvalidProgram(error)) if error == expected));
    }
}
