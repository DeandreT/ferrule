//! Public entry points of the named owned generation presets. Each row names
//! one [`OwnedPreset`] and its lowering, program validation, emission
//! validation and named-input planning functions; every function applies that
//! preset's [`OwnedPreset::capabilities`] to the shared capability gates.

use crate::{
    LowerError, NamedInputAdmissionPlan, OwnedPreset, Program, ProgramValidationError,
    legacy_named_input_admission_plan_with_owned_capabilities, lower_with_owned_capabilities,
    validate_basic_owned_emission_with_capabilities,
};
use mapping::Project;

/// Validates the common, control-free Basic source context supported by both
/// emitters ([`OwnedPreset::ControlFree`]).
///
/// General Program structure and stored ownership contexts are checked first.
/// This additive gate does not replace the ordinary closed emission gate.
pub fn validate_basic_owned_emission(program: &Program) -> Result<(), ProgramValidationError> {
    validate_basic_owned_emission_with_capabilities(
        program,
        OwnedPreset::ControlFree.capabilities(),
    )
}

macro_rules! owned_presets {
    ($($preset:ident => $lower:ident, $validate:ident, $emission:ident, $plan:ident;)*) => {$(
        #[doc = concat!(
            "Lowers a Project through unchanged graph lowering and validates it for the [`OwnedPreset::",
            stringify!($preset),
            "`] owned generation preset."
        )]
        pub fn $lower(project: &Project) -> Result<Program, LowerError> {
            lower_with_owned_capabilities(project, OwnedPreset::$preset.capabilities())
        }

        #[doc = concat!(
            "Validates complete Program structure and Basic owned emission for the [`OwnedPreset::",
            stringify!($preset),
            "`] preset."
        )]
        pub fn $validate(program: &Program) -> Result<(), ProgramValidationError> {
            validate_basic_owned_emission_with_capabilities(program, OwnedPreset::$preset.capabilities())
        }

        #[doc = concat!(
            "Validates Basic owned emission for the [`OwnedPreset::",
            stringify!($preset),
            "`] preset; identical to [`",
            stringify!($validate),
            "`]."
        )]
        pub fn $emission(program: &Program) -> Result<(), ProgramValidationError> {
            validate_basic_owned_emission_with_capabilities(program, OwnedPreset::$preset.capabilities())
        }

        #[doc = concat!(
            "Validates the [`OwnedPreset::",
            stringify!($preset),
            "`] preset, then plans potential legacy eager named inputs with original declaration ordinals."
        )]
        pub fn $plan(program: &Program) -> Result<NamedInputAdmissionPlan<'_>, ProgramValidationError> {
            legacy_named_input_admission_plan_with_owned_capabilities(
                program,
                OwnedPreset::$preset.capabilities(),
            )
        }
    )*};
}

owned_presets! {
    Filters => lower_with_basic_owned_filters, validate_program_with_basic_owned_filters,
        validate_basic_owned_filter_emission, legacy_named_input_admission_plan_with_basic_owned_filters;
    Sort => lower_with_basic_owned_sort, validate_program_with_basic_owned_sort,
        validate_basic_owned_sort_emission, legacy_named_input_admission_plan_with_basic_owned_sort;
    Windows => lower_with_basic_owned_windows, validate_program_with_basic_owned_windows,
        validate_basic_owned_window_emission, legacy_named_input_admission_plan_with_basic_owned_windows;
    RowFailures => lower_with_basic_owned_row_failures, validate_program_with_basic_owned_row_failures,
        validate_basic_owned_row_failure_emission,
        legacy_named_input_admission_plan_with_basic_owned_row_failures;
    GroupBy => lower_with_basic_owned_group_by, validate_program_with_basic_owned_group_by,
        validate_basic_owned_group_by_emission, legacy_named_input_admission_plan_with_basic_owned_group_by;
    SequenceItemAt => lower_with_basic_owned_sequence_item_at,
        validate_program_with_basic_owned_sequence_item_at,
        validate_basic_owned_sequence_item_at_emission,
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at;
    GroupAdjacentBy => lower_with_basic_owned_group_adjacent_by,
        validate_program_with_basic_owned_group_adjacent_by,
        validate_basic_owned_group_adjacent_by_emission,
        legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by;
    GroupStartingWith => lower_with_basic_owned_group_starting_with,
        validate_program_with_basic_owned_group_starting_with,
        validate_basic_owned_group_starting_with_emission,
        legacy_named_input_admission_plan_with_basic_owned_group_starting_with;
    GroupEndingWith => lower_with_basic_owned_group_ending_with,
        validate_program_with_basic_owned_group_ending_with,
        validate_basic_owned_group_ending_with_emission,
        legacy_named_input_admission_plan_with_basic_owned_group_ending_with;
    GroupIntoBlocks => lower_with_basic_owned_group_into_blocks,
        validate_program_with_basic_owned_group_into_blocks,
        validate_basic_owned_group_into_blocks_emission,
        legacy_named_input_admission_plan_with_basic_owned_group_into_blocks;
    FailureSources => lower_with_basic_owned_failure_sources,
        validate_program_with_basic_owned_failure_sources,
        validate_basic_owned_failure_source_emission,
        legacy_named_input_admission_plan_with_basic_owned_failure_sources;
    PathHierarchy => lower_with_basic_owned_path_hierarchy,
        validate_program_with_basic_owned_path_hierarchy,
        validate_basic_owned_path_hierarchy_emission,
        legacy_named_input_admission_plan_with_basic_owned_path_hierarchy;
    AdjacencyTree => lower_with_basic_owned_adjacency_tree,
        validate_program_with_basic_owned_adjacency_tree,
        validate_basic_owned_adjacency_tree_emission,
        legacy_named_input_admission_plan_with_basic_owned_adjacency_tree;
    RecursiveCollect => lower_with_basic_owned_recursive_collect,
        validate_program_with_basic_owned_recursive_collect,
        validate_basic_owned_recursive_collect_emission,
        legacy_named_input_admission_plan_with_basic_owned_recursive_collect;
    RecursiveFilter => lower_with_basic_owned_recursive_filter,
        validate_program_with_basic_owned_recursive_filter,
        validate_basic_owned_recursive_filter_emission,
        legacy_named_input_admission_plan_with_basic_owned_recursive_filter;
    LookupFind => lower_with_basic_owned_lookup_find, validate_program_with_basic_owned_lookup_find,
        validate_basic_owned_lookup_find_emission,
        legacy_named_input_admission_plan_with_basic_owned_lookup_find;
    DynamicField => lower_with_basic_owned_dynamic_field,
        validate_program_with_basic_owned_dynamic_field,
        validate_basic_owned_dynamic_field_emission,
        legacy_named_input_admission_plan_with_basic_owned_dynamic_field;
    DynamicLoading => lower_with_basic_owned_dynamic_loading,
        validate_program_with_basic_owned_dynamic_loading,
        validate_basic_owned_dynamic_loading_emission,
        legacy_named_input_admission_plan_with_basic_owned_dynamic_loading;
    CollectionAggregates => lower_with_basic_owned_collection_aggregates,
        validate_program_with_basic_owned_collection_aggregates,
        validate_basic_owned_collection_aggregates_emission,
        legacy_named_input_admission_plan_with_basic_owned_collection_aggregates;
    Joins => lower_with_basic_owned_joins, validate_program_with_basic_owned_joins,
        validate_basic_owned_joins_emission, legacy_named_input_admission_plan_with_basic_owned_joins;
    SequenceExists => lower_with_basic_owned_sequence_exists,
        validate_program_with_basic_owned_sequence_exists,
        validate_basic_owned_sequence_exists_emission,
        legacy_named_input_admission_plan_with_basic_owned_sequence_exists;
    SequenceAggregate => lower_with_basic_owned_sequence_aggregate,
        validate_program_with_basic_owned_sequence_aggregate,
        validate_basic_owned_sequence_aggregate_emission,
        legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate;
    XmlSerialize => lower_with_basic_owned_xml_serialize,
        validate_program_with_basic_owned_xml_serialize,
        validate_basic_owned_xml_serialize_emission,
        legacy_named_input_admission_plan_with_basic_owned_xml_serialize;
    XmlMixedContent => lower_with_basic_owned_xml_mixed_content,
        validate_program_with_basic_owned_xml_mixed_content,
        validate_basic_owned_xml_mixed_content_emission,
        legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content;
    XmlMixedConstruction => lower_with_basic_owned_xml_mixed_construction,
        validate_program_with_basic_owned_xml_mixed_construction,
        validate_basic_owned_xml_mixed_construction_emission,
        legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction;
    DocumentPaths => lower_with_basic_owned_document_paths,
        validate_program_with_basic_owned_document_paths,
        validate_basic_owned_document_path_emission,
        legacy_named_input_admission_plan_with_basic_owned_document_paths;
}
