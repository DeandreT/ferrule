//! The mapping graph IR: nodes and connections that describe how a source
//! instance is transformed into a target instance, plus the project file
//! (source schema + target schema + graph + scope tree) that gets
//! saved/loaded.

mod adjacency;
mod adjacency_owned;
mod csv_repair;
mod document_project;
pub mod document_project_file;
mod document_route;
mod dynamic_field_owned;
mod edi;
mod external_source;
mod file_codec;
mod filter_map;
mod fixed_width;
mod flextext;
mod http;
mod idoc;
mod idoc_native_config;
mod idoc_native_text;
mod iteration;
mod json_tuple_arrays;
mod model;
mod node_inputs;
mod owned_join;
mod path_hierarchy;
mod path_hierarchy_owned;
mod pdf;
pub mod pdf_layout_file;
mod pdf_repair;
mod pipeline;
pub mod pipeline_file;
pub mod project_file;
mod protobuf;
mod reachable;
mod recursive;
mod recursive_collect_owned;
mod runtime_dependency;
mod scope_roots;
mod scope_serde;
mod sequence_levels;
mod source_owner;
mod source_scope;
mod sqlite_read;
mod stored_schemas;
mod swift;
mod tabular;
mod wsdl;
mod xbrl;
mod xlsx_output;
mod xml_mixed_content_owned;
mod xml_scalar_output;
mod xml_serialize_owned;
mod xml_wildcard_exclusions;
pub use json_tuple_arrays::JSON_TUPLE_ARRAYS_V1_CAPABILITY;
pub use xml_scalar_output::{
    XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY, XmlScalarOutputAdmissionError,
};
pub use xml_wildcard_exclusions::XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY;

pub use adjacency::AdjacencyTreePlan;
pub use adjacency_owned::OwnedAdjacencyTreePlan;
pub use csv_repair::{CsvRuntimeDependency, CsvTextRepairCause, CsvTextRepairDependency};

mod json5_interchange;
pub use document_project::{
    DocumentProject, DocumentProjectFormat, EmptyFileSelection, GlobalFilename,
    PrimaryFileSelection, PrimaryFilenameError, PrimarySourcePlan,
};
pub use document_route::{
    PrimaryDocumentFormat, PrimaryDocumentRoute, PrimaryDocumentShape, PrimaryRouteError,
    PrimaryRouteSide,
};
pub use dynamic_field_owned::OwnedDynamicSourceField;
pub use edi::{
    EdiAutocomplete, EdiBoundaryKind, EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind,
    EdiValueConstraint, EdifactAutocomplete, X12Autocomplete, X12Separators,
};
pub use external_source::{
    ExternalHttpHeader, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions,
    ExternalSourceOptionsError, ExternalSourceOrigin,
};
pub use file_codec::FileCodecError;
pub use filter_map::{
    FilterMapAdmissionError, FilterMapAdmissionKind, FilterMapCapture, FilterMapStage, FilterMapV1,
    MAX_FILTER_MAP_CAPTURES, MAX_FILTER_MAP_FUNCTION_DEPTH, MAX_FILTER_MAP_FUNCTION_PARAMETERS,
};
pub use fixed_width::{FixedFieldWidth, FixedWidthLayout, FixedWidthLayoutError};
pub use flextext::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout, FlexTextLayoutError, MAX_FLEXTEXT_LAYOUT_DEPTH, MAX_FLEXTEXT_LAYOUT_NODES,
    MAX_FLEXTEXT_LAYOUT_STRING_BYTES, MAX_FLEXTEXT_REGEX_COMPILED_BYTES,
    MAX_FLEXTEXT_REGEX_PATTERN_BYTES, ManySplitter, OnceSplitter, StoreTrim, SwitchArm, SwitchMode,
    TrimSide,
};
pub use http::{HttpGetOptions, HttpTimeoutSeconds};
pub use idoc::{
    IdocFieldLayout, IdocLayout, IdocLayoutError, IdocSegmentLayout, MAX_IDOC_FIELDS,
    MAX_IDOC_RECORD_BYTES, MAX_IDOC_SEGMENTS,
};
pub use idoc_native_config::{
    IdocNativeCode, IdocNativeConfig, IdocNativeConfigError, IdocNativeField, IdocNativeFieldType,
    IdocNativeGroup, IdocNativeNode, IdocNativeSegment, IdocNativeStatus, MAX_IDOC_NATIVE_BYTES,
    MAX_IDOC_NATIVE_CODES, MAX_IDOC_NATIVE_DEPTH, MAX_IDOC_NATIVE_NODES,
    MAX_IDOC_NATIVE_TEXT_BYTES,
};
pub use idoc_native_text::{
    IDOC_NATIVE_VALIDATION_CASES, IdocNativeSeparators, IdocNativeTextError,
    IdocNativeTextSettings, IdocNativeValidationAction, IdocNativeValidationCase,
    IdocNativeValidationKind,
};
pub use iteration::{
    JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource, JoinSourceCardinality,
    ScopeIteration, ScopeSequence,
};
pub use json5_interchange::{
    Json5InterchangeDependency, Json5StringComponentKind, Json5StringInterchangeDependency,
};
pub use model::{
    AggregateOp, Binding, DynamicBinding, DynamicChild, DynamicSourcePath, EdiConfigDependency,
    FailureIteration, FailureRule, FailureSelection, FormatOptions, FunctionId, FunctionParameter,
    FunctionParameterId, Graph, IterationOutput, MAX_RUNTIME_PARAMETER_NAME_BYTES, NamedSource,
    NamedTarget, Node, NodeId, OwnedAggregate, OwnedAggregateValue, OwnedCollectionFind,
    OwnedJoinAggregate, OwnedJoinField, OwnedJoinSourcePosition, OwnedLookup, OwnedPosition,
    OwnedRowDocumentPath, OwnedSourceDocumentPath, OwnedSourceField, Project, RuntimeValue, Scope,
    ScopeConstruction, SequenceExpr, SequenceWindow, SortFilterOrder, SortKey, UserFunction,
    XlsxColumn, XlsxCompositeLayout, XlsxFixedCell, XlsxFixedRecord, XlsxGridLayout, XlsxRow,
    XlsxTableRegion, XlsxWorksheetSetLayout, XmlMixedContentElement, XmlMixedContentReplacement,
};
pub use node_inputs::{NodeInput, NodeInputRole, NodeInputSlot};
pub use owned_join::{
    OwnedJoinConditions, OwnedJoinKey, OwnedJoinPlan, OwnedJoinPlanError, OwnedJoinSource,
    describe_owned_collection,
};
pub use path_hierarchy::PathHierarchyPlan;
pub use path_hierarchy_owned::OwnedPathHierarchyPlan;
pub use pdf::{
    PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCaptureAlgorithm, PdfCommand, PdfCoordinate,
    PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfLayoutError, PdfMerge, PdfMergeComposition,
    PdfMergeSource, PdfMetricMatch, PdfPageSelection, PdfPages, PdfReference, PdfRegion,
    PdfTextCase, PdfTextGroup, PdfTextGroupOutput, PdfTextGroups, PdfTextMatch, PdfTextProperties,
    PdfTextRows, PdfVerticalBoundaryFind, PdfWhitespaceMode, PdfWordSeparation,
};
pub use pdf_repair::{PdfRepairDependency, PdfRuntimeDependency};
pub use pipeline::{Pipeline, PipelineInput, PipelineNamedInput, PipelineStage};
pub use protobuf::{ProtobufOptions, ProtobufSchemaFile};
pub use recursive::RecursiveFilterPlan;
pub use recursive_collect_owned::OwnedRecursiveCollect;
pub use runtime_dependency::{RuntimeBoundary, RuntimeDependency};
pub use scope_roots::{NestedScopeKind, ScopeRoot, ScopeRootRole, ScopeRootSlot};
pub use sequence_levels::SequenceLevelInput;
pub use source_owner::{
    OwnedSourceAccess, OwnedSourceRef, SourceOwner, SourceSelector, SourceSelectorMut,
    SourceSelectorRef,
};
pub use source_scope::{
    SourceScopeLocation, SourceScopeStep, SourceScopeTarget, first_owned_source_scope,
    first_project_owned_source_scope,
};
pub use sqlite_read::{
    MAX_SQLITE_READ_COLUMNS, MAX_SQLITE_READ_IDENTIFIER_BYTES, MAX_SQLITE_READ_ORDER_TERMS,
    MAX_SQLITE_READ_PARAMETER_NAME_BYTES, MAX_SQLITE_READ_PARAMETERS, MAX_SQLITE_READ_PREDICATES,
    QueryProfileError, QueryProfileReason, SQLITE_READ_V1_CAPABILITY, SqliteReadComparison,
    SqliteReadComparisonOp, SqliteReadDirection, SqliteReadInput, SqliteReadNullTest,
    SqliteReadNulls, SqliteReadOrder, SqliteReadOwner, SqliteReadParameter, SqliteReadPredicate,
    SqliteReadProfile, SqliteReadProjection, SqliteReadV1, ValidatedOrder, ValidatedParameter,
    ValidatedPredicate, ValidatedPredicateTest, ValidatedProjection, ValidatedSqliteRead,
    validate_sqlite_read_profile,
};
pub use swift::{
    SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftMtLayoutError,
    SwiftValueExpr,
};
pub use tabular::TabularBoundaryKind;
pub use wsdl::{WsdlMessageOptions, WsdlMessageOptionsError, WsdlMessageRole};
pub use xbrl::{
    XBRL_UNIT_FIELD_PREFIX, XbrlBoundaryMode, XbrlBoundaryOptions, XbrlBoundaryOptionsError,
    XbrlFactBinding, XbrlFactType, XbrlNamespaceBinding,
};
pub use xlsx_output::{
    XlsxCellKind, XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange, XlsxRangeStart,
};
pub use xml_mixed_content_owned::{
    OwnedXmlMixedContent, OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement,
};
pub use xml_serialize_owned::OwnedXmlSerialize;

pub(crate) use model::{is_constructed_scope, is_repeated_output};

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/filter_map.rs"]
mod filter_map_tests;

#[cfg(test)]
#[path = "tests/owned_collection.rs"]
mod owned_collection_tests;

#[cfg(test)]
#[path = "tests/owned_failure_iteration.rs"]
mod owned_failure_iteration_tests;

#[cfg(test)]
#[path = "tests/owned_collection_aggregates.rs"]
mod owned_collection_aggregates_tests;
#[cfg(test)]
#[path = "tests/owned_join_aggregate.rs"]
mod owned_join_aggregate_tests;
#[cfg(test)]
#[path = "tests/owned_join_tuple.rs"]
mod owned_join_tuple_tests;
#[cfg(test)]
#[path = "tests/owned_lookup_find.rs"]
mod owned_lookup_find_tests;
#[cfg(test)]
#[path = "tests/owned_recursive_collect.rs"]
mod owned_recursive_collect_tests;

#[cfg(test)]
#[path = "tests/owned_recursive_filter.rs"]
mod owned_recursive_filter_tests;

#[cfg(test)]
#[path = "tests/owned_dynamic_field.rs"]
mod owned_dynamic_field_tests;

#[cfg(test)]
#[path = "tests/optional_presence.rs"]
mod optional_presence_tests;

#[cfg(test)]
#[path = "tests/owned_dynamic_driver.rs"]
mod owned_dynamic_driver_tests;
