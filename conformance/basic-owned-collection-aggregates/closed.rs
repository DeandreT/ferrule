//! Refusal controls: unsupported owned aggregates, older generation profiles
//! and directly constructed Programs. Expected errors are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceSchemaShape as Shape,
    OwnedSourceStructureReason as Structure, ProgramValidationError,
};
use mapping::AggregateOp;
use projects::{REF, path};

fn owner(name: &str) -> Owner {
    match name {
        "Primary" => Owner::Primary,
        name => Owner::Named { name: name.into() },
    }
}
fn iteration(name: &str, parts: &[&str]) -> Request {
    Request::Iteration {
        owner: owner(name),
        path: path(parts),
    }
}
fn at(node: u32) -> Location {
    Location::Expression {
        namespace: Namespace::Main,
        node,
        scope: None,
    }
}

/// Every earlier profile refuses the Primary owned aggregate at node 90.
pub fn aggregate_closed() -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: at(90),
            request: Some(iteration("Primary", &["Rows"])),
            reason: Reason::AggregateLookup,
            origin: Origin::RetainedExpression {
                namespace: Namespace::Main,
                node: 90,
            },
        }),
    }
}
fn structure(request: Request, reason: Structure) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Structure {
            location: at(90),
            request,
            reason,
        }),
    }
}
fn item_field(parts: &[&str]) -> Request {
    Request::Field {
        owner: Owner::Primary,
        path: path(parts),
        frame: Some(path(&["Rows"])),
    }
}
fn validation(location: &str, message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
/// The capability reason of a single owned-source diagnostic.
pub fn reason(diagnostics: &[codegen::Diagnostic]) -> Option<Reason> {
    match diagnostics {
        [codegen::Diagnostic::OwnedSource { diagnostic }] => match diagnostic.as_ref() {
            Diagnostic::Capability { reason, .. } => Some(*reason),
            Diagnostic::Structure { .. } => None,
        },
        _ => None,
    }
}

pub enum Refusal {
    /// A closed capability, with the owned request it refuses.
    Capability(Reason),
    /// Complete engine validation diagnostics.
    Validation(Vec<codegen::Diagnostic>),
}

/// Projects refused by the owned aggregate profile.
pub fn projects() -> Vec<(&'static str, Project, Refusal)> {
    let mut result = Vec::new();
    // Owned failure predicates keep their closed scalar subset.
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        60,
        projects::aggregate(AggregateOp::Count, REF, &["Rows"], &[], None),
    );
    p.graph.nodes.insert(
        61,
        Node::Call {
            function: "equal".into(),
            args: vec![60, 44],
        },
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::OwnedSource {
            source: projects::source(REF, &["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 61 },
        message: None,
    });
    result.push((
        "R01-failure-predicate",
        p,
        Refusal::Capability(Reason::FailureRule),
    ));
    // A legacy aggregate item expression stays closed to owned aggregates.
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        60,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: path(&["Rows"]),
            value: vec![],
            expression: Some(91),
            arg: None,
        },
    );
    p.extra_targets[0].root.bindings[0].node = 60;
    result.push((
        "R02-legacy-aggregate-item",
        p,
        Refusal::Capability(Reason::AggregateLookup),
    ));
    // The ordinary legacy aggregate remains closed in every Basic owned profile.
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        91,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: path(&[REF, "Rows"]),
            value: path(&["Rank"]),
            expression: None,
            arg: None,
        },
    );
    result.push((
        "R03-legacy-aggregate",
        p,
        Refusal::Capability(Reason::AggregateLookup),
    ));
    // Engine validation owns value paths and collections.
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        90,
        projects::aggregate(
            AggregateOp::Sum,
            "Primary",
            &["Rows"],
            &["Lines", "Qty"],
            None,
        ),
    );
    result.push((
        "R04-value-crosses-lines",
        p,
        Refusal::Validation(vec![validation(
            "graph node 90",
            "owned aggregate value `Lines/Qty` is not a nonrepeating scalar under owned collection `Rows`",
        )]),
    ));
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        90,
        projects::aggregate(AggregateOp::Join, "Primary", &["Rows"], &[], None),
    );
    result.push((
        "R05-whole-group-value",
        p,
        Refusal::Validation(vec![validation(
            "graph node 90",
            "owned aggregate value `` is not a nonrepeating scalar under owned collection `Rows`",
        )]),
    ));
    let mut p = projects::project("C01-every-operator-root");
    p.graph.nodes.insert(
        90,
        projects::aggregate(AggregateOp::Count, "Primary", &["Note"], &[], None),
    );
    result.push((
        "R06-nonrepeating-collection",
        p,
        Refusal::Validation(vec![validation(
            "graph node 90",
            "owned collection `Note` is not a repeating path in its owner",
        )]),
    ));
    result
}

/// Programs constructed directly from the lowered C01 Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let with = |function: AggregateOp, name: &str, collection: &[&str], value: &[&str]| {
        let mut program = base.clone();
        for node in &mut program.expressions {
            if node.id == 90 {
                node.expression = codegen::Expression::OwnedAggregate(mapping::OwnedAggregate {
                    function,
                    collection: projects::source(name, collection),
                    value: mapping::OwnedAggregateValue::Path(path(value)),
                    arg: None,
                });
            }
        }
        program
    };
    vec![
        (
            "D01-unknown-owner",
            with(AggregateOp::Sum, "Missing", &["Rows"], &["Rank"]),
            structure(iteration("Missing", &["Rows"]), Structure::UnknownOwner),
        ),
        (
            "D02-nonrepeating",
            with(AggregateOp::Count, "Primary", &["Note"], &[]),
            structure(
                iteration("Primary", &["Note"]),
                Structure::InvalidOwnedCollection {
                    found: Shape::Scalar { repeating: false },
                },
            ),
        ),
        (
            "D03-value-crosses-lines",
            with(AggregateOp::Sum, "Primary", &["Rows"], &["Lines", "Qty"]),
            structure(
                item_field(&["Rows", "Lines", "Qty"]),
                Structure::InvalidOwnedField {
                    found: Shape::Group { repeating: true },
                },
            ),
        ),
        (
            "D04-missing-value",
            with(AggregateOp::Max, "Primary", &["Rows"], &["Missing"]),
            structure(
                item_field(&["Rows", "Missing"]),
                Structure::MissingOwnedPath { segment: 1 },
            ),
        ),
        (
            "D05-whole-group-value",
            with(AggregateOp::Join, "Primary", &["Rows"], &[]),
            structure(
                item_field(&["Rows"]),
                Structure::InvalidOwnedField {
                    found: Shape::Group { repeating: true },
                },
            ),
        ),
    ]
}

/// Every other generation profile, including the dynamic-field and XML
/// serialization companions beside owned search and the generated-sequence
/// aggregate companion.
pub fn old_lower(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
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
        codegen::lower_with_basic_owned_recursive_filter(project),
        codegen::lower_with_basic_owned_lookup_find(project),
        codegen::lower_with_basic_owned_dynamic_field(project),
        codegen::lower_with_basic_owned_xml_serialize(project),
        codegen::lower_with_basic_owned_sequence_aggregate(project),
    ]
}
pub fn old_validators(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
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
        codegen::validate_program_with_basic_owned_recursive_filter(p),
        codegen::validate_basic_owned_recursive_filter_emission(p),
        codegen::validate_program_with_basic_owned_lookup_find(p),
        codegen::validate_basic_owned_lookup_find_emission(p),
        codegen::validate_program_with_basic_owned_dynamic_field(p),
        codegen::validate_basic_owned_dynamic_field_emission(p),
        codegen::validate_program_with_basic_owned_xml_serialize(p),
        codegen::validate_basic_owned_xml_serialize_emission(p),
        codegen::validate_program_with_basic_owned_sequence_aggregate(p),
        codegen::validate_basic_owned_sequence_aggregate_emission(p),
    ]
}
pub fn old_catalogues(p: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    [
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
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(p).map(drop),
    ]
    .into()
}
pub fn old_rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
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
        codegen_rust::emit_with_basic_owned_recursive_filter(p, options),
        codegen_rust::emit_with_basic_owned_lookup_find(p, options),
        codegen_rust::emit_with_basic_owned_dynamic_field(p, options),
        codegen_rust::emit_with_basic_owned_xml_serialize(p, options),
        codegen_rust::emit_with_basic_owned_sequence_aggregate(p, options),
    ]
}
pub fn old_csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
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
        codegen_csharp::emit_with_basic_owned_recursive_filter(p),
        codegen_csharp::emit_with_basic_owned_lookup_find(p),
        codegen_csharp::emit_with_basic_owned_dynamic_field(p),
        codegen_csharp::emit_with_basic_owned_xml_serialize(p),
        codegen_csharp::emit_with_basic_owned_sequence_aggregate(p),
    ]
}
/// The ordinary and control-free entries refuse owned aggregates before emission.
pub fn ordinary_refusals(p: &codegen::Program, options: &codegen_rust::Options) -> Vec<bool> {
    vec![
        codegen::validate_owned_emission(p).is_err(),
        codegen_rust::emit(p, options).is_err(),
        codegen_rust::emit_with_basic_owned_sources(p, options).is_err(),
        codegen_csharp::emit(p).is_err(),
        codegen_csharp::emit_with_basic_owned_sources(p).is_err(),
    ]
}
