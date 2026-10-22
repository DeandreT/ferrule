//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod csv_document_boundary;
mod csv_input_boundary;
mod csv_json_boundary;
mod csv_output;
mod csv_x12_boundary;
mod diagnostic;
mod document_program;
mod document_visitor;
mod embedded_schema;
mod join;
mod json5_boundary;
mod lower;
mod model;
mod owned_admission;
mod static_document_boundary;
mod validate;
mod x12_boundary;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use csv_document_boundary::{
    CsvDocumentBoundaryError, CsvDocumentBoundaryPolicy, validate_csv_document_boundary,
    validate_csv_document_set_boundary,
};
pub use csv_input_boundary::{
    CsvInputBoundaryError, CsvInputField, CsvInputPolicy, validate_primary_csv_acquisition,
};
pub use csv_json_boundary::{
    CsvJsonBoundaryError, CsvJsonBoundaryOwner, CsvJsonBoundaryPolicy, CsvJsonBoundaryProfile,
    NamedJsonTargetDescriptor, prepare_csv_json_boundary, prepare_csv_json_project_boundary,
};
pub use csv_output::{
    CsvOutputError, CsvOutputPolicy, validate_csv_document_set_output, validate_csv_output,
};
pub use csv_x12_boundary::{
    CsvX12BoundaryError, CsvX12BoundaryPolicy, CsvX12BoundaryProfile, prepare_csv_x12_boundary,
    prepare_csv_x12_project_boundary,
};
pub use diagnostic::{Diagnostic, LowerError, ScopeFeature, UnsupportedNodeKind};
pub use document_program::{
    DocumentLowerError, DocumentProgram, PrimaryFilenamePlan, lower_document,
};
pub use document_visitor::{
    DocumentVisitorBoundaryError, DocumentVisitorEmitError, validate_document_clocks,
    validate_document_filename_clock, validate_document_visitor,
};
pub use embedded_schema::{
    EmbeddedSchemaError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_EMBEDDED_XML_SCHEMA_BYTES,
    serialize_embedded_schema,
};
pub use join::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource,
    JoinSourceCardinality, OwnedInnerJoin,
};
pub use json5_boundary::{
    Json5BoundaryPolicyError, Json5BoundaryProfile, Json5BoundarySide, prepare_json5_boundary,
    validate_json5_boundary, validate_json5_format_options,
};
pub use lower::lower_with_basic_owned_adjacency_tree;
pub use lower::lower_with_basic_owned_path_hierarchy;
pub use lower::lower_with_basic_owned_recursive_filter;
pub use lower::lower_with_basic_owned_xml_mixed_construction;
pub use lower::lower_with_basic_owned_xml_mixed_content;
pub use lower::lower_with_basic_owned_xml_serialize;
pub use lower::{
    lower, lower_with_basic_owned_collection_aggregates, lower_with_basic_owned_dynamic_field,
    lower_with_basic_owned_failure_sources, lower_with_basic_owned_filters,
    lower_with_basic_owned_group_adjacent_by, lower_with_basic_owned_group_by,
    lower_with_basic_owned_group_ending_with, lower_with_basic_owned_group_into_blocks,
    lower_with_basic_owned_group_starting_with, lower_with_basic_owned_joins,
    lower_with_basic_owned_lookup_find, lower_with_basic_owned_recursive_collect,
    lower_with_basic_owned_row_failures, lower_with_basic_owned_sequence_item_at,
    lower_with_basic_owned_sort, lower_with_basic_owned_windows,
};
pub use model::{
    AggregateFunction, AggregateValue, Binding, DelimitedTextField, DelimitedTextFieldError,
    DynamicDocumentIteration, DynamicSourceProgram, DynamicTargetBinding, DynamicTargetChild,
    Expression, ExpressionNode, FailureIteration, FailureRule, FailureSelection,
    FlexTextFieldProfile, GeneratedFilterMapCapture, GeneratedFilterMapV1, GeneratedRange,
    GeneratedSequence, GroupingPlan, IterationOutput, IterationPlan, IterationSource,
    NamedSourceProgram, NamedTargetProgram, NamedXmlInputPolicy, NamedXmlOutputPolicy,
    OptionalPresence, OwnedSourceIteration, Program, RuntimeValue, SUPPORTED_SCALAR_CALLS,
    ScalarFunction, ScalarTargetDomain, ScopeSequence, SequenceWindow, SortFilterOrder, SortKey,
    SortPlan, SourceIteration, TargetConstruction, TargetScope, UserFunctionParameter,
    UserFunctionProgram, XmlBoundaryProgram, XmlInputPolicy, XmlInputProfile,
    XmlMixedContentElement, XmlMixedContentReplacement, XmlOutputMode, XmlOutputPolicy,
};
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_recursive_filter;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_xml_serialize;
pub use owned_admission::{
    NamedInputAdmission, NamedInputAdmissionPlan, NamedInputAvailability, NamedInputCatalogue,
    NamedTargetInputCatalogue, legacy_named_input_admission_plan,
    legacy_named_input_admission_plan_with_basic_owned_collection_aggregates,
    legacy_named_input_admission_plan_with_basic_owned_dynamic_field,
    legacy_named_input_admission_plan_with_basic_owned_failure_sources,
    legacy_named_input_admission_plan_with_basic_owned_filters,
    legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by,
    legacy_named_input_admission_plan_with_basic_owned_group_by,
    legacy_named_input_admission_plan_with_basic_owned_group_ending_with,
    legacy_named_input_admission_plan_with_basic_owned_group_into_blocks,
    legacy_named_input_admission_plan_with_basic_owned_group_starting_with,
    legacy_named_input_admission_plan_with_basic_owned_joins,
    legacy_named_input_admission_plan_with_basic_owned_lookup_find,
    legacy_named_input_admission_plan_with_basic_owned_recursive_collect,
    legacy_named_input_admission_plan_with_basic_owned_row_failures,
    legacy_named_input_admission_plan_with_basic_owned_sequence_item_at,
    legacy_named_input_admission_plan_with_basic_owned_sort,
    legacy_named_input_admission_plan_with_basic_owned_windows,
};
pub use static_document_boundary::{
    DocumentBoundaryDescriptor, DocumentBoundaryFormat, DocumentBoundaryOptions,
    NamedDocumentBoundaryDescriptor, NamedDocumentBoundaryOptions, StaticDocumentBoundaryError,
    StaticDocumentBoundaryOwner, StaticDocumentBoundaryPolicy, StaticDocumentBoundaryProfile,
    prepare_static_document_boundary, prepare_static_document_project_boundaries,
};
pub use validate::{
    GroupingExpressionRole, JoinKeySide, OwnedSourceCapabilityOrigin, OwnedSourceCapabilityReason,
    OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation, OwnedSourceDiagnosticOwner,
    OwnedSourceDiagnosticRequest, OwnedSourceNamespace, OwnedSourceSchemaShape,
    OwnedSourceStructureReason, ProgramValidationError, RecursiveSequencePathRole,
    SequenceExpressionRole, SequenceOwner, owned_expression_emission_error,
    owned_sequence_emission_error, validate_owned_emission, validate_program,
    validate_program_with_basic_owned_collection_aggregates,
    validate_program_with_basic_owned_dynamic_field,
    validate_program_with_basic_owned_failure_sources, validate_program_with_basic_owned_filters,
    validate_program_with_basic_owned_group_adjacent_by,
    validate_program_with_basic_owned_group_by,
    validate_program_with_basic_owned_group_ending_with,
    validate_program_with_basic_owned_group_into_blocks,
    validate_program_with_basic_owned_group_starting_with, validate_program_with_basic_owned_joins,
    validate_program_with_basic_owned_lookup_find,
    validate_program_with_basic_owned_recursive_collect,
    validate_program_with_basic_owned_row_failures,
    validate_program_with_basic_owned_sequence_item_at, validate_program_with_basic_owned_sort,
    validate_program_with_basic_owned_windows,
};
pub use validate::{
    validate_basic_owned_adjacency_tree_emission, validate_basic_owned_path_hierarchy_emission,
    validate_program_with_basic_owned_adjacency_tree,
    validate_program_with_basic_owned_path_hierarchy,
};
pub use validate::{
    validate_basic_owned_recursive_filter_emission,
    validate_basic_owned_xml_mixed_construction_emission,
    validate_basic_owned_xml_mixed_content_emission, validate_basic_owned_xml_serialize_emission,
    validate_program_with_basic_owned_recursive_filter,
    validate_program_with_basic_owned_xml_mixed_construction,
    validate_program_with_basic_owned_xml_mixed_content,
    validate_program_with_basic_owned_xml_serialize,
};
pub use x12_boundary::{
    MAX_EMBEDDED_X12_DESCRIPTOR_BYTES, MAX_X12_SCHEMA_DEPTH, MAX_X12_SCHEMA_NODES,
    X12BoundaryOptions, X12BoundaryPolicy, X12BoundaryPolicyError, X12BoundaryProfile,
    X12BoundarySide, X12EnvelopeProfile, prepare_x12_boundary, validate_x12_boundary,
    validate_x12_json_format_options,
};

pub use validate::{
    is_basic_owned_primary_row_failure, validate_basic_owned_dynamic_field_emission,
    validate_basic_owned_emission, validate_basic_owned_failure_source_emission,
    validate_basic_owned_filter_emission, validate_basic_owned_group_adjacent_by_emission,
    validate_basic_owned_group_by_emission, validate_basic_owned_group_ending_with_emission,
    validate_basic_owned_group_into_blocks_emission,
    validate_basic_owned_group_starting_with_emission, validate_basic_owned_joins_emission,
    validate_basic_owned_lookup_find_emission, validate_basic_owned_recursive_collect_emission,
    validate_basic_owned_row_failure_emission, validate_basic_owned_sequence_item_at_emission,
    validate_basic_owned_sort_emission, validate_basic_owned_window_emission,
};

// Explicit owned SequenceExists generation profile.
pub use lower::lower_with_basic_owned_sequence_exists;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_sequence_exists;
pub use validate::validate_basic_owned_collection_aggregates_emission;
pub use validate::{
    validate_basic_owned_sequence_exists_emission,
    validate_program_with_basic_owned_sequence_exists,
};

// Explicit owned SequenceAggregate generation profile.
pub use lower::lower_with_basic_owned_sequence_aggregate;
pub use owned_admission::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate;
pub use validate::{
    validate_basic_owned_sequence_aggregate_emission,
    validate_program_with_basic_owned_sequence_aggregate,
};

#[cfg(test)]
mod tests;
