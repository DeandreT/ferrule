//! Backend-neutral code-generation foundations.
//!
//! [`lower`] converts the deliberately small supported mapping subset into a
//! deterministic [`Program`]. Backend emitters can then return an
//! [`ArtifactSet`] without owning filesystem policy.

mod artifact;
mod csv_output;
mod diagnostic;
mod embedded_schema;
mod join;
mod json5_boundary;
mod lower;
mod model;
mod validate;
mod x12_boundary;

pub use artifact::{
    ArtifactPath, ArtifactPathError, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError,
    GeneratedFile,
};
pub use csv_output::{CsvOutputError, CsvOutputPolicy, validate_csv_output};
pub use diagnostic::{Diagnostic, LowerError, ScopeFeature, UnsupportedNodeKind};
pub use embedded_schema::{
    EmbeddedSchemaError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_EMBEDDED_XML_SCHEMA_BYTES,
    serialize_embedded_schema,
};
pub use join::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource,
    JoinSourceCardinality,
};
pub use json5_boundary::{
    Json5BoundaryPolicyError, Json5BoundaryProfile, Json5BoundarySide, prepare_json5_boundary,
    validate_json5_boundary, validate_json5_format_options,
};
pub use lower::lower;
pub use model::{
    AggregateFunction, AggregateValue, Binding, DelimitedTextField, DelimitedTextFieldError,
    DynamicDocumentIteration, DynamicSourceProgram, DynamicTargetBinding, DynamicTargetChild,
    Expression, ExpressionNode, FailureIteration, FailureRule, FailureSelection,
    FlexTextFieldProfile, GeneratedFilterMapCapture, GeneratedFilterMapV1, GeneratedRange,
    GeneratedSequence, GroupingPlan, IterationOutput, IterationPlan, IterationSource,
    NamedSourceProgram, NamedTargetProgram, NamedXmlInputPolicy, NamedXmlOutputPolicy, Program,
    RuntimeValue, SUPPORTED_SCALAR_CALLS, ScalarFunction, ScalarTargetDomain, ScopeSequence,
    SequenceWindow, SortFilterOrder, SortKey, SortPlan, SourceIteration, TargetConstruction,
    TargetScope, UserFunctionParameter, UserFunctionProgram, XmlBoundaryProgram, XmlInputPolicy,
    XmlInputProfile, XmlMixedContentElement, XmlMixedContentReplacement, XmlOutputMode,
    XmlOutputPolicy,
};
pub use validate::{
    GroupingExpressionRole, JoinKeySide, ProgramValidationError, RecursiveSequencePathRole,
    SequenceExpressionRole, SequenceOwner, validate_program,
};
pub use x12_boundary::{
    MAX_EMBEDDED_X12_DESCRIPTOR_BYTES, MAX_X12_SCHEMA_DEPTH, MAX_X12_SCHEMA_NODES,
    X12BoundaryOptions, X12BoundaryPolicy, X12BoundaryPolicyError, X12BoundaryProfile,
    X12BoundarySide, prepare_x12_boundary, validate_x12_boundary, validate_x12_json_format_options,
};

#[cfg(test)]
mod tests;
