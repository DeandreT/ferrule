//! Refusals: closed shapes under the IntoBlocks profile and every earlier
//! profile's existing typed gate for owned IntoBlocks.
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

/// Projects the IntoBlocks profile refuses, with their exact diagnostics.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut refusals = Vec::new();
    for (id, mode) in [("conflicting-by", 0), ("conflicting-ending", 1)] {
        let mut p = projects::base();
        if mode == 0 {
            p.root.children[0].group_by = Some(2);
        } else {
            p.root.children[0].group_ending_with = Some(901);
        }
        refusals.push((
            id,
            p,
            vec![validation("scope grouping modes are mutually exclusive")],
        ));
    }
    let mut missing = projects::base();
    missing.root.children[0].group_into_blocks = Some(99);
    refusals.push((
        "missing-size",
        missing,
        vec![validation("group block size references missing node 99")],
    ));
    let mut source_free = projects::base();
    source_free.root.children[0].iteration = ScopeIteration::None;
    refusals.push((
        "source-free",
        source_free,
        vec![validation("group block size has no iterated source")],
    ));
    refusals
}

/// Direct Programs the IntoBlocks validators and emitters refuse: grouping
/// stays closed for mapped-sequence output.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    let mut mapped = base.clone();
    let iteration = mapped.root.children[0].iteration.clone().unwrap();
    mapped.root.children[0].iteration = Some(codegen::IterationPlan::new_grouped(
        iteration.input().clone(),
        None,
        None,
        iteration.grouping(),
        Vec::new(),
        codegen::IterationOutput::MappedSequence,
    ));
    // A mapped sequence targets one nonrepeating group.
    mapped.root.children[0].repeating = false;
    let ir::SchemaKind::Group { children, .. } = &mut mapped.target.kind else {
        unreachable!("authored group")
    };
    children[0].repeating = false;
    vec![("direct-mapped-sequence", mapped, grouping_error())]
}

/// Every earlier generation profile and its products for one IntoBlocks Program.
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
            codegen::lower_with_basic_owned_group_ending_with(project),
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
            codegen::validate_program_with_basic_owned_group_ending_with(p),
            codegen::validate_basic_owned_group_ending_with_emission(p),
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
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p),
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
            codegen_rust::emit_with_basic_owned_group_ending_with(p, options),
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
            codegen_csharp::emit_with_basic_owned_group_ending_with(p),
        ],
    }
}
