//! Emission coverage for the explicit owned path-hierarchy companion.

use super::*;
use ::mapping::{
    NamedSource, NamedTarget, OwnedPathHierarchyPlan, OwnedSourceRef, Project, Scope,
    ScopeConstruction, SourceOwner,
};

fn directory() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "Files",
            vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
        )],
    )
}

fn hierarchy(owner: SourceOwner) -> Scope {
    let plan = OwnedPathHierarchyPlan::new(
        OwnedSourceRef {
            owner,
            path: vec!["Files".into(), "File".into()],
        },
        "::".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .expect("valid owned plan");
    Scope {
        construction: ScopeConstruction::OwnedPathHierarchy { plan },
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: input(),
        target: directory(),
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
            schema: directory(),
            options: Default::default(),
            root: hierarchy(SourceOwner::Named {
                name: "audit".into(),
            }),
        }],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: hierarchy(SourceOwner::Primary),
    }
}

#[test]
fn emits_exact_owner_calls_for_each_target() {
    let program = codegen::lower_with_basic_owned_path_hierarchy(&project()).unwrap();
    let artifacts = emit_with_basic_owned_path_hierarchy(&program).unwrap();
    let mapping = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(mapping.contains(
        "return global::Ferrule.Runtime.FerrulePathHierarchy.BuildOwned(\n            context,\n            global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary,"
    ));
    assert!(mapping.contains("global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"audit\"),"));
    assert!(mapping.contains("\"::\""));
    assert!(mapping.contains("ExecuteWithBasicOwnedSources("));
}

#[test]
fn older_csharp_entry_points_keep_refusing_owned_hierarchies() {
    let program = codegen::lower_with_basic_owned_path_hierarchy(&project()).unwrap();
    let expected = codegen::validate_program(&program).unwrap_err();
    for result in [
        emit(&program),
        emit_with_basic_owned_sources(&program),
        emit_with_basic_owned_group_starting_with(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(error)) if error == expected));
    }
}
