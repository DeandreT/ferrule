use super::*;
use crate::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceStructureReason as Structure, TargetConstruction,
};
use mapping::{
    NamedSource, NamedTarget, OwnedAdjacencyTreePlan, OwnedSourceField, OwnedSourceRef,
    SourceOwner, SourceScopeLocation, SourceScopeTarget,
};

fn node_schema() -> SchemaNode {
    SchemaNode::group(
        "node",
        vec![
            scalar("name"),
            SchemaNode::recursive_group("children", "node").repeating(),
        ],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            scalar("Root"),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::group("Row", vec![scalar("Key"), scalar("Parent")]).repeating()],
            ),
        ],
    )
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn owned_project(owner: SourceOwner) -> Project {
    let mut project = supported_project();
    project.source = input();
    project.target = node_schema();
    // The root reads the primary endpoint in the receiving context.
    project.graph = Graph {
        nodes: BTreeMap::from([(
            5,
            Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Root"]),
                },
                frame: None,
            }),
        )]),
    };
    project.extra_sources = vec![NamedSource {
        name: "audit".into(),
        path: "audit.json".into(),
        schema: input(),
        options: Default::default(),
        dynamic_path: None,
    }];
    let plan = OwnedAdjacencyTreePlan::new(
        OwnedSourceRef {
            owner,
            path: path(&["Rows", "Row"]),
        },
        path(&["Key"]),
        path(&["Parent"]),
        "name".into(),
        "children".into(),
        Some(5),
    )
    .expect("valid owned plan");
    project.root = Scope {
        construction: ScopeConstruction::OwnedAdjacencyTree { plan },
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

fn construction(owner: SourceOwner, rows: &[&str], key: &[&str]) -> TargetConstruction {
    TargetConstruction::OwnedAdjacencyTree {
        collection: OwnedSourceRef {
            owner,
            path: path(rows),
        },
        key: path(key),
        parent: path(&["Parent"]),
        target_key: "name".into(),
        target_children: "children".into(),
        root: Some(5),
    }
}

#[test]
fn owned_tree_lowers_exact_owner_and_parent_root_and_stays_closed_by_default() {
    let named = SourceOwner::Named {
        name: "audit".into(),
    };
    let project = owned_project(named.clone());
    let program = crate::lower_with_basic_owned_adjacency_tree(&project).unwrap();
    assert_eq!(
        program.root.construction,
        construction(named, &["Rows", "Row"], &["Key"])
    );
    // The root expression is retained as an ordinary target expression.
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [5]
    );
    crate::validate_basic_owned_adjacency_tree_emission(&program).unwrap();

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
                &["Rows", "Row"],
            )),
            reason: Capability::TreeOperator,
            origin: Origin::Scope { scope: root },
        }),
    };
    assert_eq!(validate_program(&program), Err(expected.clone()));
    assert_eq!(
        crate::validate_basic_owned_failure_source_emission(&program),
        Err(expected.clone())
    );
    assert_eq!(
        crate::validate_program_with_basic_owned_failure_sources(&program),
        Err(expected.clone())
    );
    let Err(error) = lower(&project) else {
        panic!("default lowering keeps owned trees closed");
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
fn owned_tree_structure_uses_the_endpoint_schema() {
    let program =
        crate::lower_with_basic_owned_adjacency_tree(&owned_project(SourceOwner::Primary)).unwrap();
    let with = |construction: TargetConstruction| {
        let mut program = program.clone();
        program.root.construction = construction;
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
    let validate = crate::validate_program_with_basic_owned_adjacency_tree;
    let missing = SourceOwner::Named {
        name: "missing".into(),
    };
    assert_eq!(
        validate(&with(construction(missing, &["Rows", "Row"], &["Key"]))),
        Err(structure(
            Owner::Named {
                name: "missing".into()
            },
            &["Rows", "Row"],
            Structure::UnknownOwner
        ))
    );
    assert_eq!(
        validate(&with(construction(
            SourceOwner::Primary,
            &["Nope", "Row"],
            &["Key"]
        ))),
        Err(structure(
            Owner::Primary,
            &["Nope", "Row"],
            Structure::MissingOwnedPath { segment: 0 }
        ))
    );
    assert_eq!(
        validate(&with(construction(
            SourceOwner::Primary,
            &["Rows"],
            &["Key"]
        ))),
        Err(ProgramValidationError::InvalidAdjacencyTreeCollection {
            target_path: vec![],
            collection: path(&["Rows"]),
        })
    );
    assert_eq!(
        validate(&with(construction(
            SourceOwner::Primary,
            &["Rows", "Row"],
            &["Nope"]
        ))),
        Err(ProgramValidationError::InvalidAdjacencyTreeField {
            target_path: vec![],
            role: "key",
            path: path(&["Nope"]),
        })
    );
    assert_eq!(
        validate(&with(construction(
            SourceOwner::Primary,
            &["Rows", "Row"],
            &["Parent"]
        ))),
        Err(ProgramValidationError::InvalidAdjacencyTreeConstruction {
            target_path: vec![]
        })
    );
    // The same rows in an endpoint whose schema lacks them do not borrow the
    // primary declaration.
    let mut narrowed = with(construction(
        SourceOwner::Named {
            name: "audit".into(),
        },
        &["Rows", "Row"],
        &["Key"],
    ));
    narrowed.extra_sources[0].source = SchemaNode::group("Input", vec![scalar("Root")]);
    assert_eq!(
        validate(&narrowed),
        Err(structure(
            Owner::Named {
                name: "audit".into()
            },
            &["Rows", "Row"],
            Structure::MissingOwnedPath { segment: 0 }
        ))
    );
    // A missing root node keeps the legacy diagnostic.
    let mut rootless = program.clone();
    rootless.expressions.clear();
    assert_eq!(
        validate(&rootless),
        Err(ProgramValidationError::MissingAdjacencyTreeRoot {
            target_path: vec![],
            expression: 5,
        })
    );
}

#[test]
fn adjacency_profile_composes_owned_path_hierarchies() {
    let mut project = owned_project(SourceOwner::Named {
        name: "audit".into(),
    });
    let mut source = input();
    let SchemaKind::Group { children, .. } = &mut source.kind else {
        unreachable!("input is a group");
    };
    children.push(scalar("Path").repeating());
    project.source = source.clone();
    project.extra_sources[0].schema = source;
    let directory = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![scalar("name")]).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            scalar("name"),
        ],
    );
    let hierarchy = mapping::OwnedPathHierarchyPlan::new(
        OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Path"]),
        },
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .expect("valid owned hierarchy");
    project.extra_targets = vec![NamedTarget {
        name: "files".into(),
        path: None,
        schema: directory,
        options: Default::default(),
        root: Scope {
            construction: ScopeConstruction::OwnedPathHierarchy { plan: hierarchy },
            ..Scope::default()
        },
    }];
    let program = crate::lower_with_basic_owned_adjacency_tree(&project).unwrap();
    crate::validate_basic_owned_adjacency_tree_emission(&program).unwrap();
    // The path-hierarchy companion admits only its own construction.
    let root = SourceScopeLocation::root(SourceScopeTarget::Primary);
    assert_eq!(
        crate::validate_basic_owned_path_hierarchy_emission(&program),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: Location::Iteration {
                    scope: root.clone(),
                },
                request: Some(request(
                    Owner::Named {
                        name: "audit".into(),
                    },
                    &["Rows", "Row"],
                )),
                reason: Capability::TreeOperator,
                origin: Origin::Scope { scope: root },
            }),
        })
    );
}
