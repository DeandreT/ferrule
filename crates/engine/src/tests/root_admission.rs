//! Owned capability admission reads construction roots through the scope-root
//! visitor. These checks compare it with the hand-written admission it
//! replaced, over every construction kind and context.

use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AdjacencyTreePlan, Binding, DynamicBinding, DynamicChild, DynamicSourcePath, Graph,
    NamedSource, Node, OwnedAdjacencyTreePlan, OwnedPathHierarchyPlan, OwnedSourceField,
    OwnedSourceRef, PathHierarchyPlan, Project, RecursiveFilterPlan, Scope, ScopeConstruction,
    ScopeIteration, SequenceWindow, SourceOwner,
};

use super::*;

/// Root admission as it read construction roots before the visitor admitted
/// them.
fn legacy_scope_roots<'a>(
    admission: &mut Admission<'a>,
    scope: &'a Scope,
    location: &SourceScopeLocation,
    context: OwnedCapabilityContext,
    ancestry: bool,
) -> Result<(), EngineError> {
    let mut child_keys = Vec::new();
    scope.visit_roots(&mut |root| match scope_slots::admitted(root.slot) {
        Some(AdmittedRoot::Scope) => admission.push(None, context, ancestry, [root.node]),
        Some(AdmittedRoot::DynamicBinding) => admission.push(
            None,
            OwnedCapabilityContext::DynamicTarget,
            ancestry,
            [root.node],
        ),
        Some(AdmittedRoot::DynamicChildKey) => child_keys.push(root.node),
        Some(AdmittedRoot::Construction) | None => {}
    });
    match &scope.construction {
        ScopeConstruction::Scalar { value } => admission.push(None, context, ancestry, [*value]),
        ScopeConstruction::RecursiveFilter { plan } => admission.push(
            None,
            if scope.owned_recursive_filter().is_some() {
                context
            } else {
                OwnedCapabilityContext::TreeOperator
            },
            ancestry,
            [plan.predicate()],
        ),
        ScopeConstruction::PathHierarchy { plan } => admission.path(plan.collection()),
        ScopeConstruction::OwnedPathHierarchy { plan } => {
            let source = plan.collection();
            admission.static_owner(&source.owner, &source.path, None, None, Some(location))?;
            admission.summary.used_owned = true;
        }
        ScopeConstruction::AdjacencyTree { plan } => {
            admission.path(plan.collection());
            admission.push(
                None,
                OwnedCapabilityContext::TreeOperator,
                ancestry,
                plan.root(),
            );
        }
        ScopeConstruction::OwnedAdjacencyTree { plan } => {
            let source = plan.collection();
            admission.static_owner(&source.owner, &source.path, None, None, Some(location))?;
            admission.summary.used_owned = true;
            admission.push(None, context, ancestry, plan.root());
        }
        _ => {}
    }
    admission.push(
        None,
        OwnedCapabilityContext::DynamicTarget,
        ancestry,
        child_keys,
    );
    Ok(())
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn owned_ref(owner: SourceOwner, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path(parts),
    }
}

const OWNED_READ: NodeId = 1;

/// Two legacy dynamic sources, one driven by the other, an owned-driver
/// dynamic source, a static named source, and one owned read that every
/// closed context refuses. Admission's verdict therefore names whichever
/// context reads that node first.
fn project() -> Project {
    let scalar = |name: &str| SchemaNode::scalar(name, ScalarType::String);
    let rows = |name: &str| SchemaNode::group(name, vec![scalar("Value")]).repeating();
    let named = |name: &str, dynamic_path| NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: SchemaNode::group("Other", vec![rows("Rows")]),
        options: Default::default(),
        dynamic_path,
    };
    Project {
        source: SchemaNode::group("Primary", vec![scalar("Value"), rows("Rows")]),
        target: SchemaNode::group("Output", vec![scalar("Value")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named(
                "Dyn",
                Some(DynamicSourcePath::legacy(OWNED_READ, path(&["Driver"]))),
            ),
            named("Driver", Some(DynamicSourcePath::legacy(2, Vec::new()))),
            named(
                "Loaded",
                Some(DynamicSourcePath::owned(
                    2,
                    owned_ref(SourceOwner::Primary, &["Rows"]),
                )),
            ),
            named("Static", None),
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    OWNED_READ,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: owned_ref(SourceOwner::Primary, &["Value"]),
                        frame: None,
                    }),
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("driver.json".into()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]),
        },
        root: Scope::default(),
    }
}

/// Every construction kind, over legacy and owned selectors, each beside a
/// binding, a dynamic binding, a window and a dynamic child key. Roots read
/// either the owned node or a constant, so each construction root's context
/// decides admission's verdict in some case.
fn scopes() -> Vec<Scope> {
    let collections = [
        &["Dyn", "Rows"][..],
        &["Loaded", "Rows"],
        &["Static", "Rows"],
        &["Rows"],
    ];
    let mut constructions = Vec::new();
    for collection in collections {
        for root in [None, Some(OWNED_READ), Some(3)] {
            constructions.push(ScopeConstruction::AdjacencyTree {
                plan: AdjacencyTreePlan::new(
                    path(collection),
                    path(&["Id"]),
                    path(&["Parent"]),
                    "Id".into(),
                    "Children".into(),
                    root,
                )
                .expect("authored tree plan"),
            });
        }
        constructions.push(ScopeConstruction::PathHierarchy {
            plan: PathHierarchyPlan::new(
                path(collection),
                "/".into(),
                "Directories".into(),
                "Files".into(),
                "Name".into(),
            )
            .expect("authored hierarchy plan"),
        });
    }
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named {
            name: "Static".into(),
        },
        SourceOwner::Named {
            name: "Loaded".into(),
        },
        SourceOwner::Named {
            name: "Missing".into(),
        },
    ] {
        for root in [None, Some(OWNED_READ), Some(3)] {
            constructions.push(ScopeConstruction::OwnedAdjacencyTree {
                plan: OwnedAdjacencyTreePlan::new(
                    owned_ref(owner.clone(), &["Rows"]),
                    path(&["Id"]),
                    path(&["Parent"]),
                    "Id".into(),
                    "Children".into(),
                    root,
                )
                .expect("authored owned tree plan"),
            });
        }
        constructions.push(ScopeConstruction::OwnedPathHierarchy {
            plan: OwnedPathHierarchyPlan::new(
                owned_ref(owner, &["Rows", "Value"]),
                "/".into(),
                "Directories".into(),
                "Files".into(),
                "Name".into(),
            )
            .expect("authored owned hierarchy plan"),
        });
    }
    for node in [OWNED_READ, 3] {
        constructions.push(ScopeConstruction::Scalar { value: node });
        constructions.push(ScopeConstruction::RecursiveFilter {
            plan: RecursiveFilterPlan::new("Children".into(), "Items".into(), node)
                .expect("authored recursive filter plan"),
        });
    }
    constructions.push(ScopeConstruction::Constructed);

    let iterations = [
        ScopeIteration::None,
        ScopeIteration::Source(path(&["Dyn", "Rows"])),
        ScopeIteration::OwnedSource {
            source: owned_ref(SourceOwner::Primary, &["Rows"]),
            output_path: None,
        },
    ];
    let mut scopes = Vec::new();
    for construction in constructions {
        for iteration in &iterations {
            for read in [OWNED_READ, 3] {
                scopes.push(Scope {
                    iteration: iteration.clone(),
                    construction: construction.clone(),
                    ..Scope::default()
                });
                scopes.push(Scope {
                    iteration: iteration.clone(),
                    construction: construction.clone(),
                    bindings: vec![Binding {
                        target_field: "Value".into(),
                        node: read,
                    }],
                    windows: vec![SequenceWindow::First { count: 3 }],
                    dynamic_bindings: vec![DynamicBinding {
                        key: read,
                        value: 3,
                    }],
                    dynamic_children: vec![DynamicChild {
                        key: read,
                        scope: Scope::default(),
                    }],
                    ..Scope::default()
                });
            }
        }
    }
    scopes
}

#[test]
fn construction_roots_are_admitted_as_the_hand_written_admission_did() {
    let project = project();
    let scopes = scopes();
    let contexts = [
        OwnedCapabilityContext::Basic,
        OwnedCapabilityContext::DynamicTarget,
        OwnedCapabilityContext::TreeOperator,
        OwnedCapabilityContext::GeneratedItem,
    ];
    let mut refused = 0;
    let mut checked = 0;
    for scope in &scopes {
        for context in contexts {
            for ancestry in [false, true] {
                let location = SourceScopeLocation::root(SourceScopeTarget::Primary);
                let mut admitted = Admission::new(&project);
                let mut legacy = Admission::new(&project);
                let admitted_result = admitted.scope_roots(scope, &location, context, ancestry);
                let legacy_result =
                    legacy_scope_roots(&mut legacy, scope, &location, context, ancestry);
                assert_eq!(admitted_result, legacy_result, "{scope:?} in {context:?}");
                if admitted_result.is_err() {
                    refused += 1;
                    continue;
                }
                assert_eq!(admitted.pending, legacy.pending, "{scope:?} in {context:?}");
                assert_eq!(admitted.dynamic, legacy.dynamic);
                assert_eq!(admitted.summary.used_owned, legacy.summary.used_owned);
                let verdict = admitted.graph();
                if verdict.is_err() {
                    refused += 1;
                }
                assert_eq!(verdict, legacy.graph(), "{scope:?} in {context:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "checked {checked} admissions");
    assert!(refused > 100, "refused {refused} admissions");
}
