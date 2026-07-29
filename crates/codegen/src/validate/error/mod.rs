use mapping::{FunctionId, FunctionParameterId, NodeId};

use crate::IterationOutput;

use super::{
    GroupingExpressionRole, JoinKeySide, RecursiveSequencePathRole, SequenceExpressionRole,
    SequenceOwner,
};

mod display;

/// A malformed backend-neutral program that an emitter must not publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramValidationError {
    InvalidSchemaMetadata {
        boundary: String,
        path: Vec<String>,
    },
    EmptyExtraSourceName {
        index: usize,
    },
    DuplicateExtraSourceName {
        name: String,
        first: usize,
        duplicate: usize,
    },
    DuplicateExpression {
        node: NodeId,
    },
    MissingDependency {
        node: NodeId,
        dependency: NodeId,
    },
    ExpressionCycle {
        cycle: Vec<NodeId>,
    },
    DuplicateUserFunction {
        function: FunctionId,
        first: usize,
        duplicate: usize,
    },
    UserFunction {
        function: FunctionId,
        error: Box<ProgramValidationError>,
    },
    MissingUserFunctionOutput {
        function: FunctionId,
        output: NodeId,
    },
    DuplicateUserFunctionParameter {
        function: FunctionId,
        parameter: FunctionParameterId,
    },
    FunctionParameterInMain {
        node: NodeId,
        parameter: FunctionParameterId,
    },
    UnknownFunctionParameter {
        function: FunctionId,
        node: NodeId,
        parameter: FunctionParameterId,
    },
    UnsupportedUserFunctionExpression {
        function: FunctionId,
        node: NodeId,
    },
    MissingUserFunction {
        owner: Option<FunctionId>,
        node: NodeId,
        function: FunctionId,
    },
    UserFunctionArity {
        owner: Option<FunctionId>,
        node: NodeId,
        function: FunctionId,
        expected: usize,
        actual: usize,
    },
    UserFunctionCycle {
        cycle: Vec<FunctionId>,
    },
    UserFunctionDepth {
        function: FunctionId,
        limit: usize,
    },
    InvalidAggregateCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidAggregateValuePath {
        node: NodeId,
        collection: Vec<String>,
        value: Vec<String>,
    },
    InvalidCollectionFindCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidLookupCollection {
        node: NodeId,
        collection: Vec<String>,
    },
    InvalidLookupKeyPath {
        node: NodeId,
        collection: Vec<String>,
        key: Vec<String>,
    },
    InvalidLookupValuePath {
        node: NodeId,
        collection: Vec<String>,
        value: Vec<String>,
    },
    InvalidXmlSerializeSource {
        node: NodeId,
        path: Vec<String>,
        schema: String,
    },
    RepeatingXmlSerializeSchema {
        node: NodeId,
        schema: String,
    },
    EmptyXmlSerializeNamespace {
        node: NodeId,
    },
    UnsupportedXmlSerializeSchema {
        node: NodeId,
        schema: String,
        feature: &'static str,
    },
    InvalidXmlMixedContentSource {
        node: NodeId,
        path: Vec<String>,
    },
    EmptyXmlMixedContentElement {
        node: NodeId,
        replacement: usize,
    },
    DuplicateXmlMixedContentElement {
        node: NodeId,
        element: String,
    },
    InvalidXmlMixedContentCollection {
        node: NodeId,
        replacement: usize,
        collection: Vec<String>,
    },
    DuplicateJoinOwner {
        join: crate::JoinId,
    },
    JoinRequiresRootContext {
        target_path: Vec<String>,
        join: crate::JoinId,
    },
    JoinAggregateRequiresRootContext {
        node: NodeId,
        join: crate::JoinId,
    },
    InvalidJoinSource {
        join: crate::JoinId,
        collection: Vec<String>,
        cardinality: crate::JoinSourceCardinality,
    },
    InvalidJoinKey {
        join: crate::JoinId,
        side: JoinKeySide,
        collection: Vec<String>,
        path: Vec<String>,
    },
    InactiveJoinExpression {
        node: NodeId,
        join: crate::JoinId,
    },
    InvalidJoinFieldCollection {
        node: NodeId,
        join: crate::JoinId,
        collection: Vec<String>,
    },
    InvalidJoinFieldPath {
        node: NodeId,
        join: crate::JoinId,
        collection: Vec<String>,
        path: Vec<String>,
    },
    InvalidSourceIteration {
        target_path: Vec<String>,
        source_path: Vec<String>,
    },
    MissingDynamicSourcePathExpression {
        source: String,
        expression: NodeId,
    },
    InvalidDynamicSourceDriver {
        source: String,
        driver: Vec<String>,
    },
    DynamicDocumentsRequireRoot {
        target_path: Vec<String>,
    },
    MissingDynamicTargetPathExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    MissingGroupingExpression {
        target_path: Vec<String>,
        role: GroupingExpressionRole,
        expression: NodeId,
    },
    JoinGroupingUnsupported {
        target_path: Vec<String>,
        join: crate::JoinId,
    },
    InvalidFailureSourceIteration {
        rule: usize,
        source_path: Vec<String>,
    },
    MissingFailurePredicate {
        rule: usize,
        expression: NodeId,
    },
    MissingFailureMessage {
        rule: usize,
        expression: NodeId,
    },
    MissingTargetScope {
        target_path: Vec<String>,
    },
    TargetCardinalityMismatch {
        target_path: Vec<String>,
        scope_repeating: bool,
        target_repeating: bool,
    },
    ScopeSequenceRequiresGroupTarget {
        target_path: Vec<String>,
    },
    InvalidScopeSequenceWrapper {
        target_path: Vec<String>,
    },
    InvalidScopeSequenceSegment {
        target_path: Vec<String>,
        segment: usize,
    },
    MissingSequenceExpression {
        owner: SequenceOwner,
        role: SequenceExpressionRole,
        expression: NodeId,
    },
    InvalidSequenceItem {
        owner: SequenceOwner,
        expression: NodeId,
    },
    InvalidRecursiveSequencePath {
        owner: SequenceOwner,
        role: RecursiveSequencePathRole,
        path: Vec<String>,
    },
    DuplicateSequenceItem {
        owner: SequenceOwner,
        first_owner: SequenceOwner,
        expression: NodeId,
    },
    SequenceItemOutOfContext {
        owner: SequenceOwner,
        expression: NodeId,
        item: NodeId,
    },
    MissingBindingExpression {
        target_path: Vec<String>,
        target_field: String,
        expression: NodeId,
    },
    MissingScalarExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    ScalarConstructionRequiresScalarTarget {
        target_path: Vec<String>,
    },
    GroupConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    XmlMixedContentConstructionRequiresGroupSource {
        target_path: Vec<String>,
    },
    XmlMixedContentConstructionRequiresMixedTarget {
        target_path: Vec<String>,
    },
    EmptyXmlMixedContentConstruction {
        target_path: Vec<String>,
    },
    InvalidXmlMixedContentConstructionElement {
        target_path: Vec<String>,
        element: usize,
    },
    InvalidXmlMixedContentConstructionTarget {
        target_path: Vec<String>,
        element: usize,
        target_field: String,
    },
    InvalidRecursiveFilterConstruction {
        target_path: Vec<String>,
    },
    RecursiveFilterConstructionRequiresGroupSource {
        target_path: Vec<String>,
    },
    RecursiveFilterConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    RecursiveFilterConstructionRequiresMatchingGroups {
        target_path: Vec<String>,
    },
    InvalidRecursiveFilterChildren {
        target_path: Vec<String>,
        field: String,
    },
    InvalidRecursiveFilterItems {
        target_path: Vec<String>,
        field: String,
    },
    MissingRecursiveFilterPredicate {
        target_path: Vec<String>,
        expression: NodeId,
    },
    RecursiveFilterConstructionHasContent {
        target_path: Vec<String>,
    },
    RecursiveFilterConstructionHasControls {
        target_path: Vec<String>,
    },
    RecursiveFilterConstructionHasInvalidIteration {
        target_path: Vec<String>,
    },
    InvalidPathHierarchyConstruction {
        target_path: Vec<String>,
    },
    InvalidPathHierarchyCollection {
        target_path: Vec<String>,
        collection: Vec<String>,
    },
    PathHierarchyConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    InvalidPathHierarchyName {
        target_path: Vec<String>,
        field: String,
    },
    InvalidPathHierarchyFiles {
        target_path: Vec<String>,
        field: String,
        name: String,
    },
    InvalidPathHierarchyDirectories {
        target_path: Vec<String>,
        field: String,
    },
    PathHierarchyConstructionHasContent {
        target_path: Vec<String>,
    },
    PathHierarchyConstructionHasIteration {
        target_path: Vec<String>,
    },
    InvalidAdjacencyTreeConstruction {
        target_path: Vec<String>,
    },
    InvalidAdjacencyTreeCollection {
        target_path: Vec<String>,
        collection: Vec<String>,
    },
    InvalidAdjacencyTreeField {
        target_path: Vec<String>,
        role: &'static str,
        path: Vec<String>,
    },
    AdjacencyTreeConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    InvalidAdjacencyTreeTargetKey {
        target_path: Vec<String>,
        field: String,
    },
    InvalidAdjacencyTreeTargetChildren {
        target_path: Vec<String>,
        field: String,
    },
    MissingAdjacencyTreeRoot {
        target_path: Vec<String>,
        expression: NodeId,
    },
    AdjacencyTreeConstructionHasContent {
        target_path: Vec<String>,
    },
    AdjacencyTreeConstructionHasIteration {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresGroupSource {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresGroupTarget {
        target_path: Vec<String>,
    },
    CopyConstructionRequiresMatchingGroups {
        target_path: Vec<String>,
    },
    CopyConstructionHasContent {
        target_path: Vec<String>,
    },
    CopyConstructionHasGrouping {
        target_path: Vec<String>,
    },
    ScalarConstructionHasContent {
        target_path: Vec<String>,
    },
    MissingFilterExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    MissingPostGroupFilterExpression {
        target_path: Vec<String>,
        expression: NodeId,
    },
    MissingSortExpression {
        target_path: Vec<String>,
        key: usize,
        expression: NodeId,
    },
    MissingWindowExpression {
        target_path: Vec<String>,
        window: usize,
        bound: usize,
        expression: NodeId,
    },
    InvalidIterationOutput {
        target_path: Vec<String>,
        output: IterationOutput,
    },
    InvalidDynamicTarget {
        target_path: Vec<String>,
        reason: &'static str,
    },
    MissingDynamicPropertyExpression {
        target_path: Vec<String>,
        property: usize,
        role: &'static str,
        expression: NodeId,
    },
    DynamicChild {
        target_path: Vec<String>,
        child: usize,
        error: Box<ProgramValidationError>,
    },
    InvalidDuplicateBinding {
        target_path: Vec<String>,
        target_field: String,
        first_binding: usize,
        duplicate_binding: usize,
    },
    DuplicateChildTarget {
        target_path: Vec<String>,
        target_field: String,
        first_child: usize,
        duplicate_child: usize,
    },
    BindingChildCollision {
        target_path: Vec<String>,
        target_field: String,
        binding: usize,
        child: usize,
    },
    InvalidBindingTarget {
        target_path: Vec<String>,
        target_field: String,
        binding: usize,
    },
    InvalidScalarTargetDomain {
        target_path: Vec<String>,
    },
    NamedTarget {
        target: String,
        error: Box<ProgramValidationError>,
    },
}
