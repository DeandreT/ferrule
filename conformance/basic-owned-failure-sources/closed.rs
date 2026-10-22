//! Refusal controls: unsupported owned failure walks, older generation profiles
//! and directly constructed Programs. Expected errors are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceSchemaShape as Shape, OwnedSourceStructureReason as Structure,
    ProgramValidationError,
};
use projects::{LOOKUP, path, rows, rule, when_true};

fn named(name: &str) -> Owner {
    Owner::Named { name: name.into() }
}
fn iteration(owner: Owner, parts: &[&str]) -> Request {
    Request::Iteration {
        owner,
        path: path(parts),
    }
}

/// The complete closed-walk refusal for zero-based failure rule `index`.
pub fn closed_walk(index: usize, owner: Owner, parts: &[&str], reason: Reason) -> ProgramValidationError {
    let origin = match (&owner, reason) {
        (Owner::Named { name }, Reason::DynamicOwner) => Origin::DynamicSource {
            index: 0,
            name: name.clone(),
        },
        _ => Origin::Failure { index },
    };
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::FailureIteration { index },
            request: Some(iteration(owner, parts)),
            reason,
            origin,
        }),
    }
}
pub fn lookup_walk_closed() -> ProgramValidationError {
    closed_walk(0, named(LOOKUP), &["Rows"], Reason::FailureRule)
}
fn diagnostic(error: ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("authored owned refusal");
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}
fn structure(owner: Owner, parts: &[&str], reason: Structure) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Structure {
            location: Location::FailureIteration { index: 0 },
            request: iteration(owner, parts),
            reason,
        }),
    }
}

/// Projects refused by the owned failure-source profile, with complete diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut result = Vec::new();
    // ItemAt and aggregates stay closed in owned failure predicates and messages.
    let mut p = projects::project("F02-named-when-true");
    p.graph.nodes.insert(
        60,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 61,
                item: 62,
            },
            index: 61,
        },
    );
    p.graph.nodes.insert(61, Node::Const { value: Value::Int(1) });
    p.graph.nodes.insert(
        62,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(63, Node::Call { function: "equal".into(), args: vec![60, 61] });
    p.failure_rules[0].selection = when_true(63);
    result.push(("R01-item-at-predicate", p, vec![diagnostic(lookup_walk_closed())]));
    let mut p = projects::project("F02-named-when-true");
    p.graph.nodes.insert(
        60,
        Node::Aggregate {
            function: mapping::AggregateOp::Count,
            collection: path(&["Rows"]),
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    p.failure_rules[0].message = Some(60);
    result.push(("R02-aggregate-message", p, vec![diagnostic(lookup_walk_closed())]));
    // Generated-sequence rules keep their closed Basic policy beside owned walks.
    let mut p = projects::project("F02-named-when-true");
    p.graph.nodes.insert(60, Node::Const { value: Value::Int(2) });
    p.graph.nodes.insert(
        61,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.failure_rules.push(rule(
        mapping::FailureIteration::Sequence {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 60,
                item: 61,
            },
        },
        mapping::FailureSelection::All,
        None,
    ));
    result.push((
        "R03-generated-rule",
        p,
        vec![diagnostic(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Capability {
                location: Location::Iteration {
                    scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
                },
                request: None,
                reason: Reason::FailureRule,
                origin: Origin::Failure { index: 1 },
            }),
        })],
    ));
    // Engine validation owns undeclared, nonrepeating and dynamic endpoints.
    let mut p = projects::project("F02-named-when-true");
    p.failure_rules[0].iteration = rows("Missing", &["Rows"]);
    result.push(("R04-unknown-owner", p, vec![validation("unknown source owner `Missing`")]));
    let mut p = projects::project("F01-primary-when-true");
    p.failure_rules[0].iteration = rows("Primary", &["Note"]);
    result.push((
        "R05-nonrepeating",
        p,
        vec![validation("owned collection `Note` is not a repeating path in its owner")],
    ));
    let mut p = projects::project("F02-named-when-true");
    p.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(41, path(&["Rows"])));
    p.extra_targets[0].root.bindings[0].node = 41;
    result.push((
        "R06-dynamic-owner",
        p,
        vec![validation(
            "depends on dynamic extra source `Lookup`, which is unavailable before target evaluation",
        )],
    ));
    result
}
fn validation(message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: "failure rule 1".into(),
        message: message.into(),
    }
}

/// Programs constructed directly from a valid lowered owned-walk Program.
pub fn programs(base: &codegen::Program) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |source: mapping::OwnedSourceRef| {
        let mut program = base.clone();
        program.failure_rules[0].iteration =
            codegen::FailureIteration::OwnedSource(codegen::OwnedSourceIteration::new(source));
        program
    };
    let source = |name: &str, parts: &[&str]| mapping::OwnedSourceRef {
        owner: projects::owner(name),
        path: path(parts),
    };
    let mut dynamic = base.clone();
    // The audit binding becomes a constant dynamic path, so only the walk is owned.
    dynamic.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 22,
        driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
    });
    for node in &mut dynamic.expressions {
        if node.id == 22 {
            node.expression = codegen::Expression::Const {
                value: Value::String("audit".into()),
            };
        }
    }
    vec![
        (
            "D01-unknown-owner",
            with(source("Missing", &["Rows"])),
            structure(named("Missing"), &["Rows"], Structure::UnknownOwner),
        ),
        (
            "D02-empty-owner",
            with(mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Named { name: String::new() },
                path: path(&["Rows"]),
            }),
            structure(named(""), &["Rows"], Structure::InvalidNamedOwner),
        ),
        (
            "D03-missing-path",
            with(source(LOOKUP, &["Missing"])),
            structure(named(LOOKUP), &["Missing"], Structure::MissingOwnedPath { segment: 0 }),
        ),
        (
            "D04-nonrepeating",
            with(source(LOOKUP, &["Note"])),
            structure(
                named(LOOKUP),
                &["Note"],
                Structure::InvalidOwnedCollection {
                    found: Shape::Scalar { repeating: false },
                },
            ),
        ),
        (
            "D05-dynamic-owner",
            dynamic,
            closed_walk(0, named(LOOKUP), &["Rows"], Reason::DynamicOwner),
        ),
    ]
}

/// Every generation profile that predates owned failure walks.
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
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p).map(drop),
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
    ]
}
pub fn old_csharp(p: &codegen::Program) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
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
    ]
}
