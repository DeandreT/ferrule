//! Refusal controls: unsupported owned producers and consumers, older
//! generation profiles and directly constructed Programs. Expected errors are
//! authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, ProgramValidationError,
};
use mapping::{
    OwnedRecursiveCollect, OwnedSourceRef, SequenceExpr, SourceScopeLocation, SourceScopeStep,
    SourceScopeTarget,
};
use projects::{FIRST_NODE, LOOKUP, path};

fn named(name: &str) -> Owner {
    Owner::Named { name: name.into() }
}
fn iteration(owner: Owner, parts: &[&str]) -> Request {
    Request::Iteration {
        owner,
        path: path(parts),
    }
}
fn root() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
}
fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
}
/// The closed-consumer refusal of a reached owned producer at `node`.
pub fn closed_consumer(
    node: u32,
    scope: Option<SourceScopeLocation>,
    parts: &[&str],
) -> ProgramValidationError {
    capability(
        Location::Expression {
            namespace: Namespace::Main,
            node,
            scope,
        },
        Some(iteration(named(LOOKUP), parts)),
        Reason::GeneratedItem,
        Origin::Expression {
            namespace: Namespace::Main,
            node,
        },
    )
}
/// Every earlier profile refuses the first Primary binding's Lookup walk.
pub fn lookup_walk_closed() -> ProgramValidationError {
    closed_consumer(FIRST_NODE, Some(root()), &["Dir"])
}
fn diagnostic(error: ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("authored owned refusal");
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}
fn validation(location: &str, message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
fn lookup_walk(item: u32) -> SequenceExpr {
    SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner: projects::owner(LOOKUP),
            path: path(&["Dir"]),
        },
        children: path(&["Dir"]),
        descent_value: path(&["Note"]),
        values: path(&["file"]),
        value: path(&["Note"]),
        prefix: 1,
        separator: 2,
        item,
    })
}
fn generated_item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
/// R02 with its first binding replaced by `node` (id 60, item 61).
fn rebound(node: Node) -> Project {
    let mut p = projects::project("R02-named-dirs");
    p.graph.nodes.insert(60, node);
    p.graph.nodes.insert(61, generated_item());
    p.root.bindings[0].node = 60;
    p
}

/// Projects refused by the owned recursive-collect profile, with complete diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut result = Vec::new();
    // Private-item reducers stay closed for owned producers.
    let p = rebound(Node::SequenceExists {
        sequence: lookup_walk(61),
        predicate: 61,
    });
    result.push((
        "C01-exists-consumer",
        p,
        vec![diagnostic(closed_consumer(60, Some(root()), &["Dir"]))],
    ));
    let p = rebound(Node::SequenceAggregate {
        function: mapping::AggregateOp::Join,
        sequence: lookup_walk(61),
        predicate: None,
        expression: None,
        arg: None,
    });
    result.push((
        "C02-aggregate-consumer",
        p,
        vec![diagnostic(closed_consumer(60, Some(root()), &["Dir"]))],
    ));
    // A generated item never becomes an endpoint-bearing scope frame.
    let mut p = projects::project("R02-named-dirs");
    p.graph.nodes.insert(61, generated_item());
    p.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::scalar("Files", ScalarType::String).repeating()],
    );
    p.root = Scope {
        children: vec![Scope {
            target_field: "Files".into(),
            iteration: ScopeIteration::Sequence(lookup_walk(61)),
            construction: mapping::ScopeConstruction::Scalar { value: 61 },
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let files = root().descendant(SourceScopeStep::Child(0));
    result.push((
        "C03-scope-iteration",
        p,
        vec![diagnostic(capability(
            Location::Iteration {
                scope: files.clone(),
            },
            Some(iteration(named(LOOKUP), &["Dir"])),
            Reason::GeneratedItem,
            Origin::Scope { scope: files },
        ))],
    ));
    let mut p = projects::project("R02-named-dirs");
    p.graph.nodes.insert(61, generated_item());
    p.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Sequence {
            sequence: lookup_walk(61),
        },
        selection: mapping::FailureSelection::All,
        message: None,
    }];
    result.push((
        "C04-failure-iteration",
        p,
        vec![diagnostic(capability(
            Location::FailureIteration { index: 0 },
            Some(iteration(named(LOOKUP), &["Dir"])),
            Reason::FailureRule,
            Origin::Failure { index: 0 },
        ))],
    ));
    // A legacy recursive walk stays a closed ItemAt producer in every Basic profile.
    let p = rebound(Node::SequenceItemAt {
        sequence: SequenceExpr::RecursiveCollect {
            collection: path(&["Dir"]),
            children: path(&["Dir"]),
            descent_value: path(&["Note"]),
            values: path(&["file"]),
            value: path(&["Note"]),
            prefix: 1,
            separator: 2,
            item: 61,
        },
        index: 3,
    });
    result.push((
        "C05-legacy-producer",
        p,
        vec![diagnostic(capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 60,
                scope: None,
            },
            None,
            Reason::GeneratedItem,
            Origin::RetainedExpression {
                namespace: Namespace::Main,
                node: 60,
            },
        ))],
    ));
    // Engine validation owns undeclared and nonrepeating root collections.
    let mut p = projects::project("R02-named-dirs");
    let walk = |owner: &str, parts: &[&str]| {
        SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
            collection: OwnedSourceRef {
                owner: projects::owner(owner),
                path: path(parts),
            },
            ..match lookup_walk(FIRST_NODE + 100) {
                SequenceExpr::OwnedRecursiveCollect(collect) => collect,
                _ => unreachable!("authored owned walk"),
            }
        })
    };
    p.graph.nodes.insert(
        FIRST_NODE,
        Node::SequenceItemAt {
            sequence: walk("Missing", &["Dir"]),
            index: 3,
        },
    );
    result.push((
        "C06-unknown-owner",
        p,
        vec![validation(
            &format!("graph node {FIRST_NODE}"),
            "unknown source owner `Missing`",
        )],
    ));
    let mut p = projects::project("R02-named-dirs");
    p.graph.nodes.insert(
        FIRST_NODE,
        Node::SequenceItemAt {
            sequence: walk(LOOKUP, &["Note"]),
            index: 3,
        },
    );
    result.push((
        "C07-nonrepeating",
        p,
        vec![validation(
            &format!("graph node {FIRST_NODE}"),
            "owned collection `Note` is not a repeating path in its owner",
        )],
    ));
    result
}

/// Programs constructed directly from a valid lowered owned-walk Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |collection: mapping::OwnedSourceRef| {
        let mut program = base.clone();
        for node in &mut program.expressions {
            if node.id == FIRST_NODE
                && let codegen::Expression::SequenceItemAt { sequence, .. } = &mut node.expression
                && let codegen::GeneratedSequence::OwnedRecursiveCollect {
                    collection: owned, ..
                } = sequence
            {
                *owned = collection.clone();
            }
        }
        program
    };
    let source = |name: &str, parts: &[&str]| mapping::OwnedSourceRef {
        owner: projects::owner(name),
        path: path(parts),
    };
    let recursive_path = |parts: &[&str]| ProgramValidationError::InvalidRecursiveSequencePath {
        owner: codegen::SequenceOwner::Expression(FIRST_NODE),
        role: codegen::RecursiveSequencePathRole::Collection,
        path: path(parts),
    };
    let mut dynamic = base.clone();
    // The audit binding becomes a constant dynamic path, so Lookup is dynamic.
    dynamic.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 3,
        driver: codegen::SourceIteration::new(path(&["Dir"])),
    });
    vec![
        // Recursive path validation keeps its precedence for unresolved roots.
        (
            "D01-unknown-owner",
            with(source("Missing", &["Dir"])),
            recursive_path(&["Dir"]),
        ),
        (
            "D02-missing-path",
            with(source(LOOKUP, &["Missing"])),
            recursive_path(&["Missing"]),
        ),
        (
            "D03-empty-owner",
            with(mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Named {
                    name: String::new(),
                },
                path: path(&["Dir"]),
            }),
            recursive_path(&["Dir"]),
        ),
        (
            "D05-dynamic-owner",
            dynamic,
            capability(
                Location::Expression {
                    namespace: Namespace::Main,
                    node: FIRST_NODE,
                    scope: Some(root()),
                },
                Some(iteration(named(LOOKUP), &["Dir"])),
                Reason::DynamicOwner,
                Origin::DynamicSource {
                    index: 0,
                    name: LOOKUP.into(),
                },
            ),
        ),
    ]
}

/// Every generation profile that predates owned recursive walks.
pub fn old_lower(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
        codegen::lower(project),
        codegen::lower_with_basic_owned_filters(project),
        codegen::lower_with_basic_owned_sort(project),
        codegen::lower_with_basic_owned_windows(project),
        codegen::lower_with_basic_owned_row_failures(project),
        codegen::lower_with_basic_owned_group_by(project),
        codegen::lower_with_basic_owned_sequence_item_at(project),
        codegen::lower_with_basic_owned_group_adjacent_by(project),
        codegen::lower_with_basic_owned_group_starting_with(project),
        codegen::lower_with_basic_owned_group_ending_with(project),
        codegen::lower_with_basic_owned_failure_sources(project),
        codegen::lower_with_basic_owned_path_hierarchy(project),
        codegen::lower_with_basic_owned_adjacency_tree(project),
        codegen::lower_with_basic_owned_group_into_blocks(project),
    ]
}
pub fn old_validators(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
        codegen::validate_program(p),
        codegen::validate_owned_emission(p),
        codegen::validate_basic_owned_emission(p),
        codegen::validate_program_with_basic_owned_filters(p),
        codegen::validate_basic_owned_filter_emission(p),
        codegen::validate_program_with_basic_owned_sort(p),
        codegen::validate_basic_owned_sort_emission(p),
        codegen::validate_program_with_basic_owned_windows(p),
        codegen::validate_basic_owned_window_emission(p),
        codegen::validate_program_with_basic_owned_row_failures(p),
        codegen::validate_basic_owned_row_failure_emission(p),
        codegen::validate_program_with_basic_owned_group_by(p),
        codegen::validate_basic_owned_group_by_emission(p),
        codegen::validate_program_with_basic_owned_sequence_item_at(p),
        codegen::validate_basic_owned_sequence_item_at_emission(p),
        codegen::validate_program_with_basic_owned_group_adjacent_by(p),
        codegen::validate_basic_owned_group_adjacent_by_emission(p),
        codegen::validate_program_with_basic_owned_group_starting_with(p),
        codegen::validate_basic_owned_group_starting_with_emission(p),
        codegen::validate_program_with_basic_owned_group_ending_with(p),
        codegen::validate_basic_owned_group_ending_with_emission(p),
        codegen::validate_program_with_basic_owned_failure_sources(p),
        codegen::validate_basic_owned_failure_source_emission(p),
        codegen::validate_program_with_basic_owned_path_hierarchy(p),
        codegen::validate_basic_owned_path_hierarchy_emission(p),
        codegen::validate_program_with_basic_owned_adjacency_tree(p),
        codegen::validate_basic_owned_adjacency_tree_emission(p),
        codegen::validate_program_with_basic_owned_group_into_blocks(p),
        codegen::validate_basic_owned_group_into_blocks_emission(p),
    ]
}
pub fn old_catalogues(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    [
        codegen::legacy_named_input_admission_plan(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p)
            .map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(p).map(drop),
    ]
    .into()
}
pub fn old_rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
        codegen_rust::emit(p, options),
        codegen_rust::emit_with_basic_owned_sources(p, options),
        codegen_rust::emit_with_basic_owned_filters(p, options),
        codegen_rust::emit_with_basic_owned_sort(p, options),
        codegen_rust::emit_with_basic_owned_windows(p, options),
        codegen_rust::emit_with_basic_owned_row_failures(p, options),
        codegen_rust::emit_with_basic_owned_group_by(p, options),
        codegen_rust::emit_with_basic_owned_sequence_item_at(p, options),
        codegen_rust::emit_with_basic_owned_group_adjacent_by(p, options),
        codegen_rust::emit_with_basic_owned_group_starting_with(p, options),
        codegen_rust::emit_with_basic_owned_group_ending_with(p, options),
        codegen_rust::emit_with_basic_owned_failure_sources(p, options),
        codegen_rust::emit_with_basic_owned_path_hierarchy(p, options),
        codegen_rust::emit_with_basic_owned_adjacency_tree(p, options),
        codegen_rust::emit_with_basic_owned_group_into_blocks(p, options),
    ]
}
pub fn old_csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
        codegen_csharp::emit(p),
        codegen_csharp::emit_with_basic_owned_sources(p),
        codegen_csharp::emit_with_basic_owned_filters(p),
        codegen_csharp::emit_with_basic_owned_sort(p),
        codegen_csharp::emit_with_basic_owned_windows(p),
        codegen_csharp::emit_with_basic_owned_row_failures(p),
        codegen_csharp::emit_with_basic_owned_group_by(p),
        codegen_csharp::emit_with_basic_owned_sequence_item_at(p),
        codegen_csharp::emit_with_basic_owned_group_adjacent_by(p),
        codegen_csharp::emit_with_basic_owned_group_starting_with(p),
        codegen_csharp::emit_with_basic_owned_group_ending_with(p),
        codegen_csharp::emit_with_basic_owned_failure_sources(p),
        codegen_csharp::emit_with_basic_owned_path_hierarchy(p),
        codegen_csharp::emit_with_basic_owned_adjacency_tree(p),
        codegen_csharp::emit_with_basic_owned_group_into_blocks(p),
    ]
}
