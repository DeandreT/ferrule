//! Typed genuine-Project structural/capability controls, separately from execution.
use super::*;
pub const COUNT: usize = 10;
fn route() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0))
}
pub fn scope_error(reason: codegen::OwnedSourceCapabilityReason) -> codegen::OwnedSourceDiagnostic {
    codegen::OwnedSourceDiagnostic::Capability {
        location: codegen::OwnedSourceDiagnosticLocation::Iteration { scope: route() },
        request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
            owner: codegen::OwnedSourceDiagnosticOwner::Primary,
            path: projects::path(&["Rows"]),
        }),
        reason,
        origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route() },
    }
}
fn expression_error(node: u32) -> codegen::OwnedSourceDiagnostic {
    codegen::OwnedSourceDiagnostic::Capability {
        location: codegen::OwnedSourceDiagnosticLocation::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
            scope: Some(route()),
        },
        request: None,
        reason: codegen::OwnedSourceCapabilityReason::AggregateLookup,
        origin: codegen::OwnedSourceCapabilityOrigin::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
        },
    }
}
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut controls = Vec::new();
    for mode in ["adjacent", "starting", "ending", "blocks"] {
        let mut project = projects::base();
        let scope = &mut project.root.children[0];
        scope.group_by = None;
        match mode {
            "adjacent" => scope.group_adjacent_by = Some(101),
            "starting" => scope.group_starting_with = Some(103),
            "ending" => scope.group_ending_with = Some(103),
            "blocks" => scope.group_into_blocks = Some(107),
            _ => unreachable!(),
        }
        controls.push((
            mode,
            project,
            vec![codegen::Diagnostic::OwnedSource {
                diagnostic: Box::new(scope_error(codegen::OwnedSourceCapabilityReason::Grouping)),
            }],
        ));
    }
    for mode in ["aggregate", "lookup", "find"] {
        let mut project = projects::base();
        project.root.children[0].bindings[0].node = 710;
        let node = match mode {
            "aggregate" => Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: projects::path(&["Rows"]),
                value: vec![],
                expression: None,
                arg: None,
            },
            "lookup" => Node::Lookup {
                collection: projects::path(&["Rows"]),
                key: projects::path(&["Key"]),
                matches: 101,
                value: projects::path(&["Label"]),
            },
            "find" => Node::CollectionFind {
                collection: projects::path(&["Rows"]),
                predicate: 103,
                value: 102,
            },
            _ => unreachable!(),
        };
        project.graph.nodes.insert(710, node);
        controls.push((
            mode,
            project,
            vec![codegen::Diagnostic::OwnedSource {
                diagnostic: Box::new(expression_error(710)),
            }],
        ));
    }
    for (id, message) in [
        (
            "post-without-group",
            "post-group filter requires one grouping control",
        ),
        (
            "conflicting-grouping",
            "scope grouping modes are mutually exclusive",
        ),
    ] {
        let mut project = projects::base();
        let scope = &mut project.root.children[0];
        if id == "post-without-group" {
            scope.group_by = None;
            scope.post_group_filter = Some(103);
        } else {
            scope.group_adjacent_by = Some(101);
        }
        controls.push((
            id,
            project,
            vec![codegen::Diagnostic::Validation {
                location: "scope `Groups`".into(),
                message: message.into(),
            }],
        ));
    }
    let mut project = projects::base();
    project.graph.nodes.insert(
        101,
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Named {
                    name: "Røws💠".into(),
                },
                path: projects::path(&["Rows", "Key"]),
            },
            frame: Some(projects::path(&["Rows"])),
        }),
    );
    // Public Project lowering performs engine schema validation before a
    // Program exists; retain that earlier complete diagnostic, not a fabricated
    // Program-only structure error from a later capability boundary.
    controls.push((
        "exact-owner-unknown-without-spaces",
        project,
        vec![codegen::Diagnostic::Validation {
            location: "graph node 101".into(),
            message: "unknown source owner `Røws💠`".into(),
        }],
    ));
    controls
}
