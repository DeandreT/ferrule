//! Refusal controls: unsupported owned filter contexts, older generation
//! profiles and directly constructed Programs. Expected errors are authored
//! here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, ProgramValidationError,
};
use mapping::{SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use projects::{LOOKUP, PREDICATE, path};

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
fn child() -> SourceScopeLocation {
    root().descendant(SourceScopeStep::Child(0))
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
/// A scope-level refusal of the Primary target's filter at `scope`.
fn scope_refusal(
    scope: SourceScopeLocation,
    request: Option<Request>,
    reason: Reason,
) -> ProgramValidationError {
    capability(
        Location::Iteration {
            scope: scope.clone(),
        },
        request,
        reason,
        Origin::Scope { scope },
    )
}
/// Every earlier profile refuses the Primary target's Lookup filter.
pub fn lookup_filter_closed() -> ProgramValidationError {
    scope_refusal(
        child(),
        Some(iteration(named(LOOKUP), &["Dir"])),
        Reason::TreeOperator,
    )
}
fn diagnostic(error: ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("authored owned refusal");
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}

/// Projects refused by the owned recursive-filter profile, with complete diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut result = Vec::new();
    // A legacy filter stays a closed tree operator beside admitted owned filters.
    let mut p = projects::project("F01-primary-dirs");
    p.root.children[0].iteration = ScopeIteration::Source(path(&["Dir"]));
    // Its owned predicate read is refused inside the closed legacy filter.
    result.push((
        "C01-legacy-filter",
        p,
        vec![diagnostic(capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 10,
                scope: Some(child()),
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Dir", "file", "name"]),
                frame: None,
            }),
            Reason::TreeOperator,
            Origin::Scope { scope: child() },
        ))],
    ));
    // A generated item parent never becomes an endpoint-bearing frame.
    let mut p = projects::project("F02-named-dirs");
    let filter = p.root.children.remove(0);
    p.graph.nodes.extend([
        (
            30,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            31,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            32,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    p.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::group("Item", vec![projects::directory_schema().repeating()]).repeating()],
    );
    p.root.children.push(Scope {
        target_field: "Item".into(),
        iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
            input: 30,
            delimiter: 31,
            item: 32,
        }),
        children: vec![filter],
        ..Scope::default()
    });
    let item = child().descendant(SourceScopeStep::Child(0));
    result.push((
        "C02-generated-parent",
        p,
        vec![diagnostic(capability(
            Location::Iteration {
                scope: item.clone(),
            },
            Some(iteration(named(LOOKUP), &["Dir"])),
            Reason::GeneratedItem,
            Origin::Scope { scope: child() },
        ))],
    ));
    // First output is not an admitted owned-filter control.
    let mut p = projects::project("F02-named-dirs");
    p.target = SchemaNode::group("Out", vec![projects::directory_schema()]);
    p.root.children[0].iteration_output = mapping::IterationOutput::First;
    result.push((
        "C03-first-output",
        p,
        vec![diagnostic(scope_refusal(
            child(),
            Some(iteration(named(LOOKUP), &["Dir"])),
            Reason::Document,
        ))],
    ));
    // Aggregates and user functions in the owned predicate keep their gates.
    let mut p = projects::project("F02-named-dirs");
    p.graph.nodes.extend([
        (
            40,
            Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: path(&["Dir", "file"]),
                value: Vec::new(),
                expression: None,
                arg: None,
            },
        ),
        (
            PREDICATE,
            Node::Call {
                function: "equal".into(),
                args: vec![40, 10],
            },
        ),
    ]);
    result.push((
        "C04-aggregate-predicate",
        p,
        vec![diagnostic(capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 40,
                scope: Some(child()),
            },
            None,
            Reason::AggregateLookup,
            Origin::Expression {
                namespace: Namespace::Main,
                node: 40,
            },
        ))],
    ));
    result
}

/// Programs constructed directly from a valid lowered owned-filter Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |owner: mapping::SourceOwner, parts: &[&str]| {
        let mut program = base.clone();
        program.root.children[0].iteration = Some(codegen::IterationPlan::owned_source(
            mapping::OwnedSourceRef {
                owner,
                path: path(parts),
            },
        ));
        program
    };
    // Ordinary structural precedence: an unresolved owner supplies no group.
    let no_group = ProgramValidationError::RecursiveFilterConstructionRequiresGroupSource {
        target_path: path(&["Dir"]),
    };
    let mut dynamic = base.clone();
    dynamic.expressions.push(codegen::ExpressionNode {
        id: 99,
        expression: codegen::Expression::Const {
            value: Value::String("lookup.json".into()),
        },
    });
    dynamic.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(path(&["Dir"])).into(),
    });
    vec![
        (
            "D01-unknown-owner",
            with(projects::owner("Missing"), &["Dir"]),
            no_group.clone(),
        ),
        (
            "D02-missing-path",
            with(projects::owner(LOOKUP), &["Missing"]),
            no_group.clone(),
        ),
        (
            "D03-empty-owner",
            with(
                mapping::SourceOwner::Named {
                    name: String::new(),
                },
                &["Dir"],
            ),
            no_group,
        ),
        (
            "D04-dynamic-owner",
            dynamic,
            capability(
                Location::Iteration { scope: child() },
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

/// Every generation profile that predates owned recursive filters.
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
        codegen::lower_with_basic_owned_recursive_collect(project),
        codegen::lower_with_basic_owned_sequence_exists(project),
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
        codegen::validate_program_with_basic_owned_recursive_collect(p),
        codegen::validate_basic_owned_recursive_collect_emission(p),
        codegen::validate_program_with_basic_owned_sequence_exists(p),
        codegen::validate_basic_owned_sequence_exists_emission(p),
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
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(p).map(drop),
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
        codegen_rust::emit_with_basic_owned_recursive_collect(p, options),
        codegen_rust::emit_with_basic_owned_sequence_exists(p, options),
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
        codegen_csharp::emit_with_basic_owned_recursive_collect(p),
        codegen_csharp::emit_with_basic_owned_sequence_exists(p),
    ]
}
