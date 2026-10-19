//! Q01 literal old-wrapper matrix. UNRUN; retain returns before comparing.
use super::*;
#[derive(Debug)]
pub struct Products<'a>{
 pub lowering:Vec<Result<codegen::Program,codegen::LowerError>>,
 pub validators:Vec<Result<(),codegen::ProgramValidationError>>,
 pub catalogues:Vec<Result<codegen::NamedInputAdmissionPlan<'a>,codegen::ProgramValidationError>>,
 pub rust:Vec<Result<codegen::ArtifactSet,codegen_rust::EmitError>>,
 pub csharp:Vec<Result<codegen::ArtifactSet,codegen_csharp::EmitError>>,
}
pub fn products<'a>(project:&Project,p:&'a codegen::Program,options:&codegen_rust::Options)->Products<'a>{
 Products{
 lowering:closed::old_lower(project),
 validators:vec![
  codegen::validate_program(p),codegen::validate_basic_owned_emission(p),
  codegen::validate_program_with_basic_owned_filters(p),codegen::validate_basic_owned_filter_emission(p),
  codegen::validate_program_with_basic_owned_sort(p),codegen::validate_basic_owned_sort_emission(p),
  codegen::validate_program_with_basic_owned_windows(p),codegen::validate_basic_owned_window_emission(p),
  codegen::validate_program_with_basic_owned_row_failures(p),codegen::validate_basic_owned_row_failure_emission(p),
  codegen::validate_program_with_basic_owned_group_by(p),codegen::validate_basic_owned_group_by_emission(p),
  codegen::validate_program_with_basic_owned_sequence_item_at(p),codegen::validate_basic_owned_sequence_item_at_emission(p),
  codegen::validate_program_with_basic_owned_group_adjacent_by(p),codegen::validate_basic_owned_group_adjacent_by_emission(p)
 ],
 catalogues:vec![
  codegen::legacy_named_input_admission_plan(p),codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p),
  codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p),codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p),
  codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p),codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p),
  codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p),codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p)
 ],
 rust:vec![
  codegen_rust::emit(p,options),codegen_rust::emit_with_basic_owned_sources(p,options),
  codegen_rust::emit_with_basic_owned_filters(p,options),codegen_rust::emit_with_basic_owned_sort(p,options),
  codegen_rust::emit_with_basic_owned_windows(p,options),codegen_rust::emit_with_basic_owned_row_failures(p,options),
  codegen_rust::emit_with_basic_owned_group_by(p,options),codegen_rust::emit_with_basic_owned_sequence_item_at(p,options),
  codegen_rust::emit_with_basic_owned_group_adjacent_by(p,options)
 ],
 csharp:vec![
  codegen_csharp::emit(p),codegen_csharp::emit_with_basic_owned_sources(p),
  codegen_csharp::emit_with_basic_owned_filters(p),codegen_csharp::emit_with_basic_owned_sort(p),
  codegen_csharp::emit_with_basic_owned_windows(p),codegen_csharp::emit_with_basic_owned_row_failures(p),
  codegen_csharp::emit_with_basic_owned_group_by(p),codegen_csharp::emit_with_basic_owned_sequence_item_at(p),
  codegen_csharp::emit_with_basic_owned_group_adjacent_by(p)
 ]}
}
