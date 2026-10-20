//! Emission coverage for the explicit owned path-hierarchy companion.

use super::*;
use mapping::{
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
        "/".into(),
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

fn options() -> Options {
    Options {
        package_name: "owned-path-hierarchy".into(),
        runtime_dependency: RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn emits_exact_owner_calls_for_each_target() {
    let program = codegen::lower_with_basic_owned_path_hierarchy(&project()).unwrap();
    let artifacts = emit_with_basic_owned_path_hierarchy(&program, &options()).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(source.contains(
        "codegen_runtime::owned_path_hierarchy(\n        context,\n        codegen_runtime::SourceOwnerRef::Primary,\n        &[\"Files\", \"File\"],"
    ));
    assert!(source.contains(
        "codegen_runtime::owned_path_hierarchy(\n        context,\n        codegen_runtime::SourceOwnerRef::Named(\"audit\"),"
    ));
    assert!(source.contains("pub fn execute_with_basic_owned_sources"));
}

#[test]
fn older_rust_entry_points_keep_refusing_owned_hierarchies() {
    let program = codegen::lower_with_basic_owned_path_hierarchy(&project()).unwrap();
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
        emit_with_basic_owned_group_starting_with(&program, &options()),
    ] {
        assert!(matches!(result, Err(EmitError::InvalidProgram(error)) if error == expected));
    }
}
