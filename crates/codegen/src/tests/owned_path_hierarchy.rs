use super::*;
use crate::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceStructureReason as Structure, TargetConstruction,
};
use mapping::{
    NamedSource, OwnedPathHierarchyPlan, OwnedSourceRef, SourceOwner, SourceScopeLocation,
    SourceScopeTarget,
};

fn directory() -> SchemaNode {
    SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![scalar("name")]).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            scalar("name"),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("Files", vec![scalar("File").repeating()]),
            scalar("Single"),
        ],
    )
}

fn owned_project(owner: SourceOwner) -> Project {
    let mut project = supported_project();
    project.source = input();
    project.target = directory();
    project.graph = Graph::default();
    project.extra_sources = vec![NamedSource {
        name: "audit".into(),
        path: "audit.json".into(),
        schema: input(),
        options: Default::default(),
        dynamic_path: None,
    }];
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
    project.root = Scope {
        construction: ScopeConstruction::OwnedPathHierarchy { plan },
        ..Scope::default()
    };
    project
}

fn request(owner: Owner, path: &[&str]) -> Request {
    Request::Iteration {
        owner,
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

#[test]
fn owned_hierarchy_lowers_exact_owner_and_stays_closed_by_default() {
    let named = SourceOwner::Named {
        name: "audit".into(),
    };
    let project = owned_project(named.clone());
    let program = crate::lower_with_basic_owned_path_hierarchy(&project).unwrap();
    assert_eq!(
        program.root.construction,
        TargetConstruction::OwnedPathHierarchy {
            collection: OwnedSourceRef {
                owner: named,
                path: vec!["Files".into(), "File".into()],
            },
            separator: "/".into(),
            directories: "directory".into(),
            files: "file".into(),
            name: "name".into(),
        }
    );
    assert!(program.expressions.is_empty());
    crate::validate_basic_owned_path_hierarchy_emission(&program).unwrap();

    let root = SourceScopeLocation::root(SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: Location::Iteration {
                scope: root.clone(),
            },
            request: Some(request(
                Owner::Named {
                    name: "audit".into(),
                },
                &["Files", "File"],
            )),
            reason: Capability::TreeOperator,
            origin: Origin::Scope { scope: root },
        }),
    };
    assert_eq!(validate_program(&program), Err(expected.clone()));
    assert_eq!(
        crate::validate_basic_owned_group_starting_with_emission(&program),
        Err(expected.clone())
    );
    let Err(error) = lower(&project) else {
        panic!("default lowering keeps owned hierarchies closed");
    };
    let ProgramValidationError::OwnedSource { diagnostic } = expected else {
        unreachable!();
    };
    assert_eq!(
        error.diagnostics(),
        [Diagnostic::OwnedSource { diagnostic }]
    );
}

#[test]
fn owned_hierarchy_structure_uses_the_endpoint_schema() {
    let program =
        crate::lower_with_basic_owned_path_hierarchy(&owned_project(SourceOwner::Primary)).unwrap();
    let with = |owner: SourceOwner, path: &[&str]| {
        let mut program = program.clone();
        program.root.construction = TargetConstruction::OwnedPathHierarchy {
            collection: OwnedSourceRef {
                owner,
                path: path.iter().map(|part| (*part).into()).collect(),
            },
            separator: "/".into(),
            directories: "directory".into(),
            files: "file".into(),
            name: "name".into(),
        };
        program
    };
    let structure = |owner: Owner, path: &[&str], reason| ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
            location: Location::Iteration {
                scope: SourceScopeLocation::root(SourceScopeTarget::Primary),
            },
            request: request(owner, path),
            reason,
        }),
    };
    let missing = SourceOwner::Named {
        name: "missing".into(),
    };
    assert_eq!(
        crate::validate_program_with_basic_owned_path_hierarchy(&with(missing, &["Files", "File"])),
        Err(structure(
            Owner::Named {
                name: "missing".into()
            },
            &["Files", "File"],
            Structure::UnknownOwner
        ))
    );
    assert_eq!(
        crate::validate_program_with_basic_owned_path_hierarchy(&with(
            SourceOwner::Primary,
            &["Nope", "File"]
        )),
        Err(structure(
            Owner::Primary,
            &["Nope", "File"],
            Structure::MissingOwnedPath { segment: 0 }
        ))
    );
    assert_eq!(
        crate::validate_program_with_basic_owned_path_hierarchy(&with(
            SourceOwner::Primary,
            &["Single"]
        )),
        Err(ProgramValidationError::InvalidPathHierarchyCollection {
            target_path: vec![],
            collection: vec!["Single".into()],
        })
    );
    // The same path in an endpoint whose schema lacks it does not borrow the
    // primary declaration.
    let mut narrowed = with(
        SourceOwner::Named {
            name: "audit".into(),
        },
        &["Files", "File"],
    );
    narrowed.extra_sources[0].source = SchemaNode::group("Input", vec![scalar("Single")]);
    assert_eq!(
        crate::validate_program_with_basic_owned_path_hierarchy(&narrowed),
        Err(structure(
            Owner::Named {
                name: "audit".into()
            },
            &["Files", "File"],
            Structure::MissingOwnedPath { segment: 0 }
        ))
    );
}
