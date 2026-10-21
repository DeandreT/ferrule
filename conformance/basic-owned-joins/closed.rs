//! Refusal controls: unsupported owned joins, wrong or inactive owned join
//! reads, older generation profiles and directly constructed Programs.
//! Expected errors are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
    ProgramValidationError,
};
use mapping::{SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use projects::{LOOKUP, path};

/// The first Primary join scope, `Row`.
fn join_scope() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary).descendant(SourceScopeStep::Child(0))
}

/// Every earlier profile refuses the Primary owned join scope.
pub fn join_closed() -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::Iteration {
                scope: join_scope(),
            },
            request: Some(Request::Iteration {
                owner: Owner::Primary,
                path: path(&["Rows"]),
            }),
            reason: Reason::Join,
            origin: Origin::Scope {
                scope: join_scope(),
            },
        }),
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

/// Projects refused by the owned join profile.
pub fn projects() -> Vec<(&'static str, Project, Refusal)> {
    let j01 = || projects::project("J01-same-path-rows");
    let mut result = Vec::new();
    // A read naming the other owner's same collection text is not a member.
    let mut p = j01();
    p.graph
        .nodes
        .insert(1, projects::join_field(1, LOOKUP, &["Orders"], &["code"]));
    result.push((
        "R01-wrong-owner-collection",
        p,
        Refusal::Validation(vec![validation(
            "scope `Row`",
            "owned join node 1 collection named `Lookup` `Orders` does not belong to join 1",
        )]),
    ));
    // A read of another join's tuple is inactive here.
    let mut p = j01();
    p.graph
        .nodes
        .insert(1, projects::join_field(2, "Primary", &["Rows"], &["code"]));
    result.push((
        "R02-inactive-join",
        p,
        Refusal::Validation(vec![validation(
            "scope `Row`",
            "owned join node 1 references inactive join 2",
        )]),
    ));
    // Legacy join fields never read an owned tuple.
    let mut p = j01();
    p.graph.nodes.insert(
        1,
        Node::JoinField {
            join: mapping::JoinId::new(1),
            collection: path(&["Rows"]),
            path: path(&["code"]),
        },
    );
    result.push((
        "R03-legacy-join-field",
        p,
        Refusal::Validation(vec![validation(
            "scope `Row`",
            "join field node 1 references owned join 1; read its members with owned join fields",
        )]),
    ));
    // A field below a member must be one of its scalars.
    let mut p = j01();
    p.graph.nodes.insert(
        1,
        projects::join_field(1, "Primary", &["Rows"], &["missing"]),
    );
    result.push((
        "R04-missing-member-field",
        p,
        Refusal::Validation(vec![validation(
            "scope `Row`",
            "owned join field `missing` is not a nonrepeating scalar under owned collection `Rows`",
        )]),
    ));
    // Ordinary owned reads stay closed in a tuple context.
    let mut p = j01();
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: projects::source("Primary", &["Header", "code"]),
            frame: None,
        }),
    );
    result.push((
        "R05-owned-read-in-tuple",
        p,
        Refusal::Capability(Reason::Join),
    ));
    // An iterating descendant of a tuple stays closed.
    let mut p = j01();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::group("Lines", vec![]).repeating()])
                .repeating(),
        ],
    );
    p.root.children[0].bindings.clear();
    p.root.children[0].children = vec![Scope {
        target_field: "Lines".into(),
        iteration: ScopeIteration::OwnedSource {
            source: projects::source("Primary", &["Rows"]),
            output_path: None,
        },
        ..Scope::default()
    }];
    result.push(("R06-nested-iteration", p, Refusal::Capability(Reason::Join)));
    // Grouping an owned join is refused by validation.
    let mut p = j01();
    p.root.children[0].group_by = Some(1);
    result.push((
        "R07-grouped-join",
        p,
        Refusal::Validation(vec![validation(
            "scope `Row`",
            "inner join iteration cannot be combined with grouping controls",
        )]),
    ));
    // An owned join read outside every join scope is inactive.
    let mut p = j01();
    p.root.bindings = vec![Binding {
        target_field: "Codes".into(),
        node: 1,
    }];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("tuple", ScalarType::Int)])
                .repeating(),
            SchemaNode::scalar("Codes", ScalarType::String),
        ],
    );
    p.root.children[0].bindings = vec![Binding {
        target_field: "tuple".into(),
        node: 5,
    }];
    result.push((
        "R08-read-outside-join",
        p,
        Refusal::Validation(vec![validation(
            "root scope",
            "owned join node 1 references inactive join 1",
        )]),
    ));
    result
}

/// Programs constructed directly from the lowered J01 Program.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let replace = |id: u32, expression: codegen::Expression| {
        let mut program = base.clone();
        for node in &mut program.expressions {
            if node.id == id {
                node.expression = expression.clone();
            }
        }
        program
    };
    let read = |join: u64, name: &str, collection: &[&str], relative: &[&str]| {
        codegen::Expression::OwnedJoinField(mapping::OwnedJoinField {
            join: mapping::JoinId::new(join),
            collection: projects::source(name, collection),
            path: path(relative),
        })
    };
    // A key path below a member must name one of its scalars.
    let mut missing_key = base.clone();
    if let Some(iteration) = missing_key.root.children[0].iteration.as_mut() {
        let plan = mapping::OwnedJoinPlan::new(
            mapping::OwnedJoinSource::new(projects::source("Primary", &["Rows"])),
            mapping::OwnedJoinSource::new(projects::source(LOOKUP, &["Rows"])),
            mapping::OwnedJoinConditions::new(projects::key(
                "Primary",
                &["Rows"],
                &["id"],
                &["missing"],
            )),
        )
        .expect("authored owned plan");
        *iteration = codegen::IterationPlan::new(
            codegen::OwnedInnerJoin::new(codegen::JoinId::new(1), plan),
            None,
            None,
            Vec::new(),
            codegen::IterationOutput::Repeated,
        );
    }
    vec![
        (
            "D01-inactive-join",
            replace(1, read(7, "Primary", &["Rows"], &["code"])),
            ProgramValidationError::InactiveJoinExpression {
                node: 1,
                join: codegen::JoinId::new(7),
            },
        ),
        (
            "D02-wrong-owner",
            replace(1, read(1, LOOKUP, &["Orders"], &["code"])),
            ProgramValidationError::InvalidOwnedJoinMember {
                node: 1,
                join: codegen::JoinId::new(1),
                collection: projects::source(LOOKUP, &["Orders"]),
            },
        ),
        (
            "D03-missing-field",
            replace(2, read(1, LOOKUP, &["Rows"], &["missing"])),
            ProgramValidationError::InvalidOwnedJoinFieldPath {
                node: 2,
                join: codegen::JoinId::new(1),
                collection: projects::source(LOOKUP, &["Rows"]),
                path: path(&["missing"]),
            },
        ),
        (
            "D04-missing-key-path",
            missing_key,
            ProgramValidationError::OwnedSource {
                diagnostic: Box::new(Diagnostic::Structure {
                    location: Location::Iteration {
                        scope: join_scope(),
                    },
                    request: Request::Field {
                        owner: Owner::Named {
                            name: LOOKUP.into(),
                        },
                        path: path(&["Rows", "missing"]),
                        frame: Some(path(&["Rows"])),
                    },
                    reason: Structure::MissingOwnedPath { segment: 1 },
                }),
            },
        ),
        (
            "D05-tuple-legacy-read",
            replace(
                1,
                codegen::Expression::SourceField {
                    frame: None,
                    path: path(&["Rows", "code"]),
                },
            ),
            ProgramValidationError::OwnedSource {
                diagnostic: Box::new(Diagnostic::Capability {
                    location: Location::Expression {
                        namespace: Namespace::Main,
                        node: 1,
                        scope: Some(join_scope()),
                    },
                    request: None,
                    reason: Reason::Join,
                    origin: Origin::Scope {
                        scope: join_scope(),
                    },
                }),
            },
        ),
    ]
}

/// Every other generation profile, including the sibling dynamic-field companion.
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
        codegen::lower_with_basic_owned_sequence_aggregate(project),
        codegen::lower_with_basic_owned_recursive_filter(project),
        codegen::lower_with_basic_owned_lookup_find(project),
        codegen::lower_with_basic_owned_dynamic_field(project),
        codegen::lower_with_basic_owned_xml_serialize(project),
        codegen::lower_with_basic_owned_collection_aggregates(project),
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
        codegen::validate_program_with_basic_owned_sequence_aggregate(p),
        codegen::validate_basic_owned_sequence_aggregate_emission(p),
        codegen::validate_program_with_basic_owned_recursive_filter(p),
        codegen::validate_basic_owned_recursive_filter_emission(p),
        codegen::validate_program_with_basic_owned_lookup_find(p),
        codegen::validate_basic_owned_lookup_find_emission(p),
        codegen::validate_program_with_basic_owned_dynamic_field(p),
        codegen::validate_basic_owned_dynamic_field_emission(p),
        codegen::validate_program_with_basic_owned_xml_serialize(p),
        codegen::validate_basic_owned_xml_serialize_emission(p),
        codegen::validate_program_with_basic_owned_collection_aggregates(p),
        codegen::validate_basic_owned_collection_aggregates_emission(p),
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
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(p).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_collection_aggregates(p)
            .map(drop),
    ]
    .into()
}
pub fn old_rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
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
        codegen_rust::emit_with_basic_owned_sequence_aggregate(p, options),
        codegen_rust::emit_with_basic_owned_recursive_filter(p, options),
        codegen_rust::emit_with_basic_owned_lookup_find(p, options),
        codegen_rust::emit_with_basic_owned_dynamic_field(p, options),
        codegen_rust::emit_with_basic_owned_xml_serialize(p, options),
        codegen_rust::emit_with_basic_owned_collection_aggregates(p, options),
    ]
}
pub fn old_csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
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
        codegen_csharp::emit_with_basic_owned_sequence_aggregate(p),
        codegen_csharp::emit_with_basic_owned_recursive_filter(p),
        codegen_csharp::emit_with_basic_owned_lookup_find(p),
        codegen_csharp::emit_with_basic_owned_dynamic_field(p),
        codegen_csharp::emit_with_basic_owned_xml_serialize(p),
        codegen_csharp::emit_with_basic_owned_collection_aggregates(p),
    ]
}
/// The ordinary emitters refuse owned joins before emission.
pub fn ordinary_refusals(p: &codegen::Program, options: &codegen_rust::Options) -> Vec<bool> {
    vec![
        codegen_rust::emit(p, options).is_err(),
        codegen_csharp::emit(p).is_err(),
    ]
}
