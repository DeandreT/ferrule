use super::{ExportProfile, export, export_with_profile, preflight_export};
use crate::MfdError;
use ir::SchemaNode;
use mapping::{
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner, SourceScopeStep, SourceScopeTarget,
};
use std::collections::BTreeMap;
use std::path::Path;
fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Default::default(),
        root: Scope {
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: vec![],
                },
                output_path: None,
            },
            ..Scope::default()
        },
    }
}
fn check(error: MfdError, route: Vec<SourceScopeStep>) {
    assert!(
        matches!(error, MfdError::UnsupportedSourceIteration { scope } if scope.target == SourceScopeTarget::Primary && scope.path == route)
    );
}
#[test]
fn every_export_profile_rejects_owned_scope_before_artifact_io() {
    let p = project();
    let path = Path::new("nonexistent-owned-collection-export-parent/output.mfd");
    check(preflight_export(&p, path).unwrap_err(), vec![]);
    check(export(&p, path).unwrap_err(), vec![]);
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        check(export_with_profile(&p, path, profile).unwrap_err(), vec![]);
    }
    assert!(!path.exists());
}
#[test]
fn qualified_early_route_and_nested_segments_cannot_omit_owned_selector() {
    let mut p = project();
    p.source_options.xml_root_view_read_policy = true;
    let owned = std::mem::take(&mut p.root);
    p.root.iteration =
        ScopeIteration::Concatenate(mapping::ScopeSequence::new(Scope::default(), vec![owned]));
    check(
        super::prepare_export(&p, Path::new("output.mfd"))
            .err()
            .unwrap(),
        vec![SourceScopeStep::Segment(1)],
    );
}
#[test]
fn owned_path_hierarchy_collection_is_refused_before_artifact_io() {
    let mut p = project();
    let plan = mapping::OwnedPathHierarchyPlan::new(
        OwnedSourceRef {
            owner: SourceOwner::Named {
                name: "audit".into(),
            },
            path: vec!["File".into()],
        },
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .unwrap();
    p.root = Scope {
        children: vec![Scope {
            target_field: "Tree".into(),
            construction: mapping::ScopeConstruction::OwnedPathHierarchy { plan },
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let path = Path::new("nonexistent-owned-hierarchy-export-parent/output.mfd");
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        check(
            export_with_profile(&p, path, profile).unwrap_err(),
            vec![SourceScopeStep::Child(0)],
        );
    }
    assert!(!path.exists());
}
#[test]
fn owned_adjacency_tree_rows_are_refused_before_artifact_io() {
    let mut p = project();
    let plan = mapping::OwnedAdjacencyTreePlan::new(
        OwnedSourceRef {
            owner: SourceOwner::Named {
                name: "audit".into(),
            },
            path: vec!["Row".into()],
        },
        vec!["Key".into()],
        vec!["Parent".into()],
        "name".into(),
        "children".into(),
        None,
    )
    .unwrap();
    p.root = Scope {
        children: vec![Scope {
            target_field: "Tree".into(),
            construction: mapping::ScopeConstruction::OwnedAdjacencyTree { plan },
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let path = Path::new("nonexistent-owned-adjacency-export-parent/output.mfd");
    check(
        preflight_export(&p, path).unwrap_err(),
        vec![SourceScopeStep::Child(0)],
    );
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        check(
            export_with_profile(&p, path, profile).unwrap_err(),
            vec![SourceScopeStep::Child(0)],
        );
    }
    assert!(!path.exists());
}
#[test]
fn every_export_profile_rejects_owned_join_before_artifact_io() {
    let rows = |owner| OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    };
    let named = || SourceOwner::Named {
        name: "lookup".into(),
    };
    let plan = mapping::OwnedJoinPlan::new(
        mapping::OwnedJoinSource::new(rows(SourceOwner::Primary)),
        mapping::OwnedJoinSource::new(rows(named())),
        mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
            rows(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    let mut p = project();
    p.root.iteration = ScopeIteration::None;
    p.root.children = vec![Scope {
        target_field: "Row".into(),
        iteration: ScopeIteration::OwnedInnerJoin {
            id: mapping::JoinId::new(4),
            plan,
        },
        ..Scope::default()
    }];
    let path = Path::new("nonexistent-owned-join-export-parent/output.mfd");
    let route = vec![SourceScopeStep::Child(0)];
    check(preflight_export(&p, path).unwrap_err(), route.clone());
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        check(
            export_with_profile(&p, path, profile).unwrap_err(),
            route.clone(),
        );
    }
    assert!(!path.exists());
}
