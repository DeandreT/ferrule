//! Complete typed Project refusals, distinct from mapping execution cases.
use super::*;
fn validation(location: &str, message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
fn route() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0))
}
pub fn expression_error(
    node: u32,
    reason: codegen::OwnedSourceCapabilityReason,
) -> codegen::OwnedSourceDiagnostic {
    codegen::OwnedSourceDiagnostic::Capability {
        location: codegen::OwnedSourceDiagnosticLocation::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
            scope: Some(route()),
        },
        request: None,
        reason,
        origin: codegen::OwnedSourceCapabilityOrigin::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
        },
    }
}
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut controls = Vec::new();
    let mut unknown = projects::base();
    unknown
        .graph
        .nodes
        .insert(30, projects::field("Cfg💠", &["Cfg", "Index"], Some(&[])));
    controls.push((
        "unknown-exact-owner",
        unknown,
        vec![validation("graph node 30", "unknown source owner `Cfg💠`")],
    ));
    let mut duplicate = projects::base();
    duplicate.extra_sources[1].name = "Cfg".into();
    // Avoid a second intentionally dangling exact-owner request in this declaration control.
    duplicate
        .graph
        .nodes
        .insert(70, projects::field("Cfg", &["Cfg", "Index"], Some(&[])));
    // Declaration validation accumulates first, then every stored owned graph
    // node is checked in BTreeMap ID order, including unreferenced row helpers.
    controls.push((
        "duplicate-stored-owner",
        duplicate,
        vec![
            validation("extra source `Cfg`", "extra source name is duplicated"),
            validation("graph node 30", "source owner `Cfg` is ambiguous"),
            validation("graph node 31", "source owner `Cfg` is ambiguous"),
            validation("graph node 32", "source owner `Cfg` is ambiguous"),
            validation("graph node 33", "source owner `Cfg` is ambiguous"),
            validation("graph node 70", "source owner `Cfg` is ambiguous"),
        ],
    ));
    for (id, input, label) in [
        ("missing-index", false, "index references missing node 999"),
        (
            "missing-producer",
            true,
            "sequence argument 0 references missing node 999",
        ),
    ] {
        let mut p = projects::base();
        p.graph.nodes.insert(
            40,
            projects::tokenize(
                if input { 999 } else { 10 },
                if input { 30 } else { 999 },
                90,
            ),
        );
        controls.push((id, p, vec![validation("graph node 40", label)]));
    }
    let mut cycle = projects::base();
    cycle.graph.nodes.insert(40, projects::tokenize(10, 40, 90));
    controls.push((
        "cycle-before-capability",
        cycle,
        vec![validation("graph node 40", "cycle reaches node 40")],
    ));
    for (id, input, label) in [
        (
            "own-item-in-index",
            false,
            "index depends on its own sequence item node 90 before that item exists",
        ),
        (
            "own-item-in-producer",
            true,
            "sequence argument 0 depends on its own sequence item node 90 before that item exists",
        ),
    ] {
        let mut p = projects::base();
        p.graph.nodes.insert(
            40,
            projects::tokenize(if input { 90 } else { 10 }, if input { 30 } else { 90 }, 90),
        );
        controls.push((id,p,vec![validation("graph node 40","expression 40 references generated sequence item node 90 outside its owning context"),validation("graph node 40",label)]));
    }
    for (id, node, reason) in [
        (
            "aggregate-does-not-inherit-ItemAt",
            Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: projects::path(&["Rows"]),
                value: vec![],
                expression: Some(40),
                arg: None,
            },
            codegen::OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            "lookup-does-not-inherit-ItemAt",
            Node::Lookup {
                collection: projects::path(&["Rows"]),
                key: projects::path(&["Key"]),
                matches: 40,
                value: projects::path(&["Index"]),
            },
            codegen::OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            "sequence-exists-stays-closed",
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 14,
                    item: 95,
                },
                predicate: 17,
            },
            codegen::OwnedSourceCapabilityReason::GeneratedItem,
        ),
        (
            "sequence-aggregate-stays-closed",
            Node::SequenceAggregate {
                function: mapping::AggregateOp::Count,
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 14,
                    item: 95,
                },
                predicate: None,
                expression: Some(40),
                arg: None,
            },
            codegen::OwnedSourceCapabilityReason::GeneratedItem,
        ),
    ] {
        let mut p = projects::project("rows-index");
        p.graph.nodes.insert(50, node);
        if id.starts_with("sequence-") {
            p.graph.nodes.insert(
                95,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
        }
        p.root.children[0].bindings[0].node = 50;
        controls.push((
            id,
            p,
            vec![codegen::Diagnostic::OwnedSource {
                diagnostic: Box::new(expression_error(50, reason)),
            }],
        ));
    }
    let mut failure = projects::base();
    failure.graph.nodes.insert(
        55,
        Node::Call {
            function: "equal".into(),
            args: vec![40, 10],
        },
    );
    failure.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::WhenTrue { predicate: 55 },
        message: None,
    }];
    controls.push((
        "owned-ItemAt-failure-predicate-stays-closed",
        failure,
        vec![codegen::Diagnostic::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 30,
                    scope: None,
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Named { name: "Cfg".into() },
                    path: projects::path(&["Cfg", "Index"]),
                    frame: Some(vec![]),
                }),
                reason: codegen::OwnedSourceCapabilityReason::FailureRule,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        }],
    ));
    let mut recursive = projects::base();
    recursive.graph.nodes.insert(
        40,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::RecursiveCollect {
                collection: projects::path(&["Rows"]),
                children: vec![],
                descent_value: projects::path(&["Key"]),
                values: vec![],
                value: projects::path(&["Index"]),
                prefix: 110,
                separator: 11,
                item: 90,
            },
            index: 30,
        },
    );
    let root = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    let refusal = |origin| codegen::Diagnostic::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 30,
                scope: Some(root.clone()),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named { name: "Cfg".into() },
                path: projects::path(&["Cfg", "Index"]),
                frame: Some(vec![]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::GeneratedItem,
            origin,
        }),
    };
    controls.push((
        "recursive-producer-does-not-inherit-simple-permission",
        recursive,
        vec![refusal(codegen::OwnedSourceCapabilityOrigin::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node: 40,
        })],
    ));
    let mut generated = projects::base();
    generated.target.repeating = true;
    generated.root.iteration = ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
        from: None,
        to: 14,
        item: 95,
    });
    generated.graph.nodes.insert(
        95,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    controls.push((
        "generated-scope-does-not-inherit-scalar-consumer",
        generated,
        vec![refusal(codegen::OwnedSourceCapabilityOrigin::Scope {
            scope: root.clone(),
        })],
    ));
    controls
}
