//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod capabilities;
mod compatibility;
mod csv_document_boundary;
mod csv_input_boundary;
mod csv_json_boundary;
mod csv_output;
mod csv_x12_boundary;
mod diagnostic;
mod document_program;
mod embedded_schema;
mod emission_plan;
mod identity;
mod join;
mod json5_boundary;
mod lower;
mod model;
mod owned_admission;
mod presets;
mod primary_document_boundary;
mod static_document_boundary;
mod validate;
mod x12_boundary;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use capabilities::{
    FailureSources, GroupingModes, OwnedCapabilities, OwnedConstructions, OwnedPreset, OwnedSearch,
    OwnedXml, SequenceConsumers, SourceControls,
};
pub use compatibility::{
    CompatibilityManifest, CompatibilityRefusal, GENERATOR_NAME, GENERATOR_VERSION,
    GenerationProfile, GeneratorRecord, LibraryIdentity, MANIFEST_FORMAT, MANIFEST_FORMAT_VERSION,
    MANIFEST_PATH, ManifestError, ManifestSelection, RuntimeBinding, RuntimeIdentity,
    RuntimeRecord,
};
pub use csv_document_boundary::{
    CsvDocumentBoundaryError, CsvDocumentBoundaryPolicy, validate_csv_document_boundary,
};
pub use csv_input_boundary::{
    CsvInputBoundaryError, CsvInputField, CsvInputPolicy, validate_primary_csv_acquisition,
};
pub use csv_json_boundary::{
    CsvJsonBoundaryError, CsvJsonBoundaryOwner, CsvJsonBoundaryPolicy, CsvJsonBoundaryProfile,
    NamedJsonTargetDescriptor, prepare_csv_json_boundary, prepare_csv_json_project_boundary,
};
pub use csv_output::{CsvOutputError, CsvOutputPolicy, validate_csv_output};
pub use csv_x12_boundary::{
    CsvX12BoundaryError, CsvX12BoundaryPolicy, CsvX12BoundaryProfile, prepare_csv_x12_boundary,
    prepare_csv_x12_project_boundary,
};
pub use diagnostic::{Diagnostic, LowerError, ScopeFeature, UnsupportedNodeKind};
pub use document_program::{
    DocumentLowerError, DocumentProgram, FixedWidthDocumentPolicy, PrimaryFilenamePlan,
    lower_document,
};
pub use embedded_schema::{
    EmbeddedSchemaError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_EMBEDDED_XML_SCHEMA_BYTES,
    serialize_embedded_schema,
};
pub use emission_plan::{
    BoundaryEmissionPlan, CompanionSelection, XmlDocumentCardinality, XmlDocumentInputs,
    XmlEmissionPlan, XmlOutputMode, prepare_boundary_emission,
};
pub use identity::{IdentityError, IdentityKind, IdentityViolation};
pub use join::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource,
    JoinSourceCardinality, OwnedInnerJoin,
};
pub use json5_boundary::{
    Json5BoundaryPolicyError, Json5BoundaryProfile, Json5BoundarySide, prepare_json5_boundary,
    validate_json5_boundary, validate_json5_format_options,
};
pub use lower::{lower, lower_with_owned_capabilities};
pub use model::{
    AggregateFunction, AggregateValue, Binding, DelimitedTextField, DelimitedTextFieldError,
    DynamicDocumentIteration, DynamicSourceDriver, DynamicSourceProgram, DynamicTargetBinding,
    DynamicTargetChild, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, FlexTextFieldProfile, GeneratedFilterMapCapture, GeneratedFilterMapV1,
    GeneratedRange, GeneratedSequence, GroupingPlan, IterationOutput, IterationPlan,
    IterationSource, NamedSourceProgram, NamedTargetProgram, NamedXmlInputPolicy,
    NamedXmlOutputPolicy, OptionalPresence, OwnedSourceIteration, Program, RuntimeValue,
    SUPPORTED_SCALAR_CALLS, ScalarFunction, ScalarTargetDomain, ScopeSequence, SequenceWindow,
    SortFilterOrder, SortKey, SortPlan, SourceIteration, TargetConstruction, TargetScope,
    UserFunctionParameter, UserFunctionProgram, XmlBoundaryProgram, XmlInputPolicy,
    XmlInputProfile, XmlMixedContentElement, XmlMixedContentReplacement, XmlOutputPolicy,
};
pub use owned_admission::{
    NamedInputAdmission, NamedInputAdmissionPlan, NamedInputAvailability, NamedInputCatalogue,
    NamedTargetInputCatalogue, legacy_named_input_admission_plan,
    legacy_named_input_admission_plan_with_owned_capabilities,
};
pub use presets::*;
pub use primary_document_boundary::{
    PrimaryDocumentBoundaryError, validate_primary_edi_documents,
    validate_primary_fixed_width_documents, validate_primary_json_documents,
    validate_primary_xml_documents,
};
pub use static_document_boundary::{
    DocumentBoundaryDescriptor, DocumentBoundaryFormat, DocumentBoundaryOptions,
    EdifactBoundaryOptions, NamedDocumentBoundaryDescriptor, NamedDocumentBoundaryOptions,
    StaticDocumentBoundaryError, StaticDocumentBoundaryOwner, StaticDocumentBoundaryPolicy,
    StaticDocumentBoundaryProfile, prepare_static_document_boundary,
    prepare_static_document_project_boundaries,
};
pub use validate::{
    GroupingExpressionRole, JoinKeySide, OwnedDynamicLoading, OwnedSourceCapabilityOrigin,
    OwnedSourceCapabilityReason, OwnedSourceDiagnostic, OwnedSourceDiagnosticLocation,
    OwnedSourceDiagnosticOwner, OwnedSourceDiagnosticRequest, OwnedSourceNamespace,
    OwnedSourceSchemaShape, OwnedSourceStructureReason, ProgramValidationError,
    RecursiveSequencePathRole, SequenceExpressionRole, SequenceOwner, XmlDocumentSetInputRefusal,
    is_basic_owned_primary_row_failure, owned_dynamic_loading, owned_expression_emission_error,
    owned_sequence_emission_error, validate_basic_owned_emission_with_capabilities,
    validate_owned_emission, validate_program, validate_program_with_owned_capabilities,
};
pub use x12_boundary::{
    MAX_EMBEDDED_X12_DESCRIPTOR_BYTES, MAX_X12_SCHEMA_DEPTH, MAX_X12_SCHEMA_NODES,
    X12BoundaryOptions, X12BoundaryPolicy, X12BoundaryPolicyError, X12BoundaryProfile,
    X12BoundarySide, X12EnvelopeProfile, prepare_x12_boundary, validate_x12_boundary,
    validate_x12_json_format_options,
};

#[cfg(test)]
mod tests;
