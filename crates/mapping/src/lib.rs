//! The mapping graph IR: nodes and connections that describe how a source
//! instance is transformed into a target instance, plus the project file
//! (source schema + target schema + graph + scope tree) that gets
//! saved/loaded.

mod adjacency;
mod edi;
mod external_source;
mod fixed_width;
mod flextext;
mod http;
mod idoc;
mod iteration;
mod model;
mod path_hierarchy;
mod pdf;
mod protobuf;
mod reachable;
mod recursive;
mod scope_serde;
mod swift;
mod tabular;
mod wsdl;
mod xbrl;
mod xlsx_output;

pub use adjacency::AdjacencyTreePlan;
pub use edi::{
    EdiAutocomplete, EdiBoundaryKind, EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind,
    EdifactAutocomplete, X12Autocomplete, X12Separators,
};
pub use external_source::{
    ExternalHttpHeader, ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions,
    ExternalSourceOptionsError, ExternalSourceOrigin,
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
pub use iteration::{
    JoinConditions, JoinId, JoinKey, JoinPlan, JoinPlanError, JoinSource, JoinSourceCardinality,
    ScopeIteration, ScopeSequence,
};
pub use model::{
    AggregateOp, Binding, DynamicBinding, DynamicChild, DynamicSourcePath, FailureIteration,
    FailureRule, FailureSelection, FormatOptions, Graph, IterationOutput, NamedSource, NamedTarget,
    Node, NodeId, Project, RuntimeValue, Scope, ScopeConstruction, SequenceExpr, SequenceWindow,
    SortFilterOrder, SortKey, XlsxColumn, XlsxCompositeLayout, XlsxFixedCell, XlsxFixedRecord,
    XlsxGridLayout, XlsxRow, XlsxTableRegion, XlsxWorksheetSetLayout, XmlMixedContentElement,
    XmlMixedContentReplacement,
};
pub use path_hierarchy::PathHierarchyPlan;
pub use pdf::{
    PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCommand, PdfCoordinate, PdfEdgeFind,
    PdfEdgeRows, PdfGroup, PdfLayout, PdfLayoutError, PdfMerge, PdfMergeComposition,
    PdfMergeSource, PdfMetricMatch, PdfPageSelection, PdfPages, PdfReference, PdfRegion,
    PdfTextCase, PdfTextGroup, PdfTextGroupOutput, PdfTextGroups, PdfTextMatch, PdfTextProperties,
    PdfTextRows, PdfVerticalBoundaryFind,
};
pub use protobuf::ProtobufOptions;
pub use recursive::RecursiveFilterPlan;
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

pub(crate) use model::{is_constructed_scope, is_repeated_output};

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;
