//! Refusals: closed modes under the Ending profile and every earlier profile's
//! existing typed gate for owned EndingWith.
use super::*;

pub fn route() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0))
}
pub fn grouping_error() -> codegen::ProgramValidationError {
    let scope = route();
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: projects::path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::Grouping,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope },
        }),
    }
}
pub fn diagnostic(error: codegen::ProgramValidationError) -> codegen::Diagnostic {
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("owned error")
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}
fn validation(message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: "scope `Groups`".into(),
        message: message.into(),
    }
}

/// Projects the Ending profile refuses, with their exact diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut refusals = Vec::new();
    let mut blocks = projects::base();
    blocks.root.children[0].group_ending_with = None;
    blocks.root.children[0].group_into_blocks = Some(912);
    refusals.push(("blocks", blocks, vec![diagnostic(grouping_error())]));
    for (id, mode) in [("conflicting-by", 0), ("conflicting-starting", 1)] {
        let mut p = projects::base();
        if mode == 0 {
            p.root.children[0].group_by = Some(2);
        } else {
            p.root.children[0].group_starting_with = Some(1);
        }
        refusals.push((
            id,
            p,
            vec![validation("scope grouping modes are mutually exclusive")],
        ));
    }
    let mut missing = projects::base();
    missing.root.children[0].group_ending_with = Some(99);
    refusals.push((
        "missing-predicate",
        missing,
        vec![validation(
            "group-ending-with predicate references missing node 99",
        )],
    ));
    let mut source_free = projects::base();
    source_free.root.children[0].iteration = ScopeIteration::None;
    refusals.push((
        "source-free",
        source_free,
        vec![validation(
            "group-ending-with predicate has no iterated source",
        )],
    ));
    refusals
}

/// Direct Programs the Ending validators and emitters refuse.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    let mut blocks = base.clone();
    blocks.expressions.push(codegen::ExpressionNode {
        id: 912,
        expression: codegen::Expression::Const {
            value: Value::Int(2),
        },
    });
    let iteration = blocks.root.children[0].iteration.clone().unwrap();
    blocks.root.children[0].iteration =
        Some(iteration.with_grouping(codegen::GroupingPlan::IntoBlocks { size: 912 }));
    vec![("direct-blocks", blocks, grouping_error())]
}

/// Every earlier generation profile and its products for one Ending Program.
pub struct OldProducts<'a> {
    pub lowering: Vec<Result<codegen::Program, codegen::LowerError>>,
    pub validators: Vec<Result<(), codegen::ProgramValidationError>>,
    pub catalogues:
        Vec<Result<codegen::NamedInputAdmissionPlan<'a>, codegen::ProgramValidationError>>,
    pub rust: Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>>,
    pub csharp: Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>>,
}
pub fn old_products<'a>(
    project: &Project,
    p: &'a codegen::Program,
    options: &codegen_rust::Options,
) -> OldProducts<'a> {
    OldProducts {
        lowering: vec![
            codegen::lower(project),
            codegen::lower_with_basic_owned_filters(project),
            codegen::lower_with_basic_owned_sort(project),
            codegen::lower_with_basic_owned_windows(project),
            codegen::lower_with_basic_owned_row_failures(project),
            codegen::lower_with_basic_owned_group_by(project),
            codegen::lower_with_basic_owned_sequence_item_at(project),
            codegen::lower_with_basic_owned_group_adjacent_by(project),
            codegen::lower_with_basic_owned_group_starting_with(project),
        ],
        validators: vec![
            codegen::validate_program(p),
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
        ],
        catalogues: vec![
            codegen::legacy_named_input_admission_plan(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p),
        ],
        rust: vec![
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
        ],
        csharp: vec![
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
        ],
    }
}
