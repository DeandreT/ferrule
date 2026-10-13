use ir::{Instance, Value};
use mapping::{FunctionId, JoinId, NodeId, ScopeIteration, SequenceExpr};

use crate::source_iteration::PositionFrame;

mod source_tree;
pub use source_tree::TraceSourceTree;

const MAX_TRACE_PREVIEW_CHARS: usize = 160;
const MAX_TRACE_ROW_FIELDS: usize = 8;
const MAX_TRACE_ROW_TEXT_BYTES: usize = 512;
const MAX_TRACE_ROW_NAME_CHARS: usize = 80;

/// One active collection position captured when a graph node was evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePosition {
    pub collection: Vec<String>,
    pub index: usize,
    pub grouped: bool,
    pub join: Option<JoinId>,
    pub join_position: Option<(JoinId, usize)>,
    pub document_path: Option<String>,
}

impl From<&PositionFrame> for TracePosition {
    fn from(position: &PositionFrame) -> Self {
        Self {
            collection: position.collection.clone(),
            index: position.index,
            grouped: position.grouped,
            join: position.join,
            join_position: position.join_position,
            document_path: position.document_path.clone(),
        }
    }
}

/// Collision-free identity of a project's primary or named output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceTarget {
    Primary,
    Named(String),
}

/// Stable identity of one target scope within one declared output.
///
/// `structural_path` distinguishes repeated target names and concatenate
/// segments without depending on runtime values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceScope {
    pub target: TraceTarget,
    pub target_path: Vec<String>,
    pub structural_path: Vec<usize>,
    pub source_scope: mapping::SourceScopeLocation,
}

impl TraceScope {
    pub(crate) fn primary() -> Self {
        Self {
            source_scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
            target: TraceTarget::Primary,
            target_path: Vec::new(),
            structural_path: Vec::new(),
        }
    }

    pub(crate) fn named(target: impl Into<String>, index: usize) -> Self {
        let name = target.into();
        Self {
            source_scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Named {
                index,
                name: name.clone(),
            }),
            target: TraceTarget::Named(name),
            target_path: Vec::new(),
            structural_path: Vec::new(),
        }
    }

    pub(crate) fn child(&self, target_field: impl Into<String>, index: usize) -> Self {
        let mut target_path = self.target_path.clone();
        target_path.push(target_field.into());
        let mut structural_path = self.structural_path.clone();
        structural_path.push(index);
        Self {
            source_scope: self
                .source_scope
                .descendant(mapping::SourceScopeStep::Child(index)),
            target: self.target.clone(),
            target_path,
            structural_path,
        }
    }

    pub(crate) fn dynamic_child(&self, index: usize, trace_index: usize) -> Self {
        let mut scope = self.child("<dynamic>", trace_index);
        scope.source_scope = self
            .source_scope
            .descendant(mapping::SourceScopeStep::DynamicChild(index));
        scope
    }

    pub(crate) fn segment(&self, index: usize) -> Self {
        let mut structural_path = self.structural_path.clone();
        structural_path.push(index);
        Self {
            source_scope: self
                .source_scope
                .descendant(mapping::SourceScopeStep::Segment(index)),
            target: self.target.clone(),
            target_path: self.target_path.clone(),
            structural_path,
        }
    }
}

/// Runtime source of the items entering a target scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceIteration {
    Once,
    Source {
        path: Vec<String>,
    },
    /// Exact declared owner and endpoint-relative path; never a legacy wrapper path.
    OwnedSource {
        source: mapping::OwnedSourceRef,
    },
    DynamicDocuments {
        source: Vec<String>,
    },
    Generated {
        kind: &'static str,
    },
    Join {
        join: JoinId,
    },
    Concatenate {
        segments: usize,
    },
}

pub(crate) fn scope_iteration(
    iteration: &ScopeIteration,
) -> Result<TraceIteration, mapping::OwnedSourceAccess> {
    Ok(match iteration {
        ScopeIteration::OwnedSource { .. } => return Err(mapping::OwnedSourceAccess),
        ScopeIteration::None => TraceIteration::Once,
        ScopeIteration::Source(path) => TraceIteration::Source { path: path.clone() },
        ScopeIteration::DynamicDocuments { source, .. } => TraceIteration::DynamicDocuments {
            source: source.clone(),
        },
        ScopeIteration::Sequence(sequence) => TraceIteration::Generated {
            kind: sequence_kind(sequence),
        },
        ScopeIteration::InnerJoin { id, .. } => TraceIteration::Join { join: *id },
        ScopeIteration::Concatenate(segments) => TraceIteration::Concatenate {
            segments: segments.iter().count(),
        },
    })
}

/// Metadata for an already admitted Basic owned scope; the legacy helper stays closed.
pub(crate) fn basic_scope_iteration(
    iteration: &ScopeIteration,
) -> Result<TraceIteration, mapping::OwnedSourceAccess> {
    match iteration {
        ScopeIteration::OwnedSource {
            source,
            output_path: None,
        } => Ok(TraceIteration::OwnedSource {
            source: source.clone(),
        }),
        ScopeIteration::OwnedSource { .. } => Err(mapping::OwnedSourceAccess),
        _ => scope_iteration(iteration),
    }
}

fn sequence_kind(sequence: &SequenceExpr) -> &'static str {
    match sequence {
        SequenceExpr::Tokenize { .. } => "tokenize",
        SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        SequenceExpr::TokenizeRegex { .. } => "tokenize-regex",
        SequenceExpr::Generate { .. } => "integer-range",
        SequenceExpr::FilterMapV1(_) => "filter-map-v1",
        SequenceExpr::RecursiveCollect { .. } => "recursive-collect",
    }
}

/// Where an item predicate was evaluated in the scope control pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFilterPhase {
    BeforeSort,
    AfterSort,
    Selection,
    GroupStarting,
    GroupEnding,
    PostGroupMember,
}

/// A bounded scalar preview used by control-flow trace events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceValue {
    pub value_type: &'static str,
    pub preview: String,
    pub truncated: bool,
}

impl TraceValue {
    pub(crate) fn new(value: &Value) -> Self {
        match value {
            Value::String(value) => {
                let (preview, truncated) = truncate_chars(value, MAX_TRACE_PREVIEW_CHARS);
                Self {
                    value_type: "string",
                    preview,
                    truncated,
                }
            }
            value => {
                let rendered = match value {
                    Value::Null => "null".to_string(),
                    Value::JsonNull(_) => "json-null".to_string(),
                    Value::XmlNil(_) => "xml-nil".to_string(),
                    Value::Bool(value) => value.to_string(),
                    Value::Int(value) => value.to_string(),
                    Value::Float(value) => value.to_string(),
                    Value::String(_) => String::new(),
                };
                Self {
                    value_type: value.type_name(),
                    preview: rendered,
                    truncated: false,
                }
            }
        }
    }
}

/// One immediate field of a bounded source-row snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSourceField {
    pub name: String,
    pub name_truncated: bool,
    pub kind: TraceOutputKind,
    pub value: Option<TraceValue>,
}

/// Source data as it entered one scope candidate, before its controls ran.
/// Immediate fields retain the legacy preview. Structural rows also carry a
/// separately bounded tree for inspection of nested groups and collections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSourceRow {
    pub kind: TraceOutputKind,
    pub value: Option<TraceValue>,
    pub fields: Vec<TraceSourceField>,
    pub omitted_fields: usize,
    pub structure: Option<TraceSourceTree>,
}

impl TraceSourceRow {
    pub(crate) fn new(instance: &Instance) -> Self {
        let mut row = Self {
            kind: TraceOutputKind::of(instance),
            value: None,
            fields: Vec::new(),
            omitted_fields: 0,
            structure: None,
        };
        match instance {
            Instance::Scalar(value) => {
                row.value = Some(bounded_row_value(value, MAX_TRACE_ROW_TEXT_BYTES));
            }
            Instance::Group(fields) => {
                let visible = fields.len().min(MAX_TRACE_ROW_FIELDS);
                let mut remaining = MAX_TRACE_ROW_TEXT_BYTES;
                for (index, (name, instance)) in fields.iter().take(visible).enumerate() {
                    // Divide the remaining budget among the remaining fields;
                    // a long early value cannot hide every later field.
                    let share = remaining / (visible - index);
                    let (name, char_truncated) = truncate_chars(name, MAX_TRACE_ROW_NAME_CHARS);
                    let (name, byte_truncated) = truncate_bytes(&name, share / 2);
                    remaining -= name.len();
                    let value = match instance {
                        Instance::Scalar(value) => {
                            let preview = bounded_row_value(value, share - name.len());
                            remaining -= preview.preview.len();
                            Some(preview)
                        }
                        _ => None,
                    };
                    row.fields.push(TraceSourceField {
                        name,
                        name_truncated: char_truncated || byte_truncated,
                        kind: TraceOutputKind::of(instance),
                        value,
                    });
                }
                row.omitted_fields = fields.len() - visible;
            }
            Instance::Repeated(_) | Instance::MappedSequence(_) | Instance::DocumentSet(_) => {}
        }
        let structural = match instance {
            Instance::Group(fields) => fields
                .iter()
                .take(MAX_TRACE_ROW_FIELDS)
                .any(|(_, value)| !matches!(value, Instance::Scalar(_))),
            Instance::Scalar(_) => false,
            _ => true,
        };
        if structural {
            row.structure = Some(TraceSourceTree::new(instance));
        }
        row
    }
}

/// One evaluated sort key without retaining its potentially large full value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSortKey {
    pub node: NodeId,
    pub descending: bool,
    pub value: TraceValue,
}

/// One concrete grouping strategy selected by a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceGrouping {
    By { node: NodeId },
    AdjacentBy { node: NodeId },
    StartingWith { node: NodeId },
    EndingWith { node: NodeId },
    IntoBlocks { node: NodeId, size: usize },
}

/// One evaluated sequence window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceWindow {
    SkipFirst(usize),
    First(usize),
    From(usize),
    FromTo { first: usize, last: usize },
    Last(usize),
}

/// Shape of one produced target item or finalized scope value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOutputKind {
    Scalar,
    Group,
    Repeated,
    MappedSequence,
    DocumentSet,
}

impl TraceOutputKind {
    pub(crate) fn of(instance: &ir::Instance) -> Self {
        match instance {
            ir::Instance::Scalar(_) => Self::Scalar,
            ir::Instance::Group(_) => Self::Group,
            ir::Instance::Repeated(_) => Self::Repeated,
            ir::Instance::MappedSequence(_) => Self::MappedSequence,
            ir::Instance::DocumentSet(_) => Self::DocumentSet,
        }
    }
}

/// How one successfully written target field was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceTargetFieldBinding {
    StaticBinding { value: NodeId },
    DynamicBinding { key: NodeId, value: NodeId },
    StaticChild,
    DynamicChild { key: NodeId },
}

/// A successful, observable step in interpreter evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    NodeValue {
        node: NodeId,
        positions: Vec<TracePosition>,
        value: TraceValue,
    },
    /// One graph-node output delivered to a specific input pin of a consumer.
    /// Untaken conditional inputs and inputs that fail before producing a value
    /// have no corresponding event.
    NodeInputValue {
        consumer: NodeId,
        input: NodeId,
        input_index: usize,
        positions: Vec<TracePosition>,
        value: TraceValue,
    },
    /// A successful node inside a reusable function body. Function-local node
    /// IDs may overlap project graph IDs and must remain qualified.
    FunctionNodeValue {
        function: FunctionId,
        node: NodeId,
        positions: Vec<TracePosition>,
        value: TraceValue,
    },
    /// A successful value delivered to a visible input of a function-body
    /// node. `input_index` is zero-based, as for `NodeInputValue`.
    FunctionNodeInputValue {
        function: FunctionId,
        consumer: NodeId,
        input: NodeId,
        input_index: usize,
        positions: Vec<TracePosition>,
        value: TraceValue,
    },
    ScopeStarted {
        scope: TraceScope,
        iteration: TraceIteration,
        positions: Vec<TracePosition>,
    },
    IterationCandidate {
        scope: TraceScope,
        ordinal: usize,
        positions: Vec<TracePosition>,
        source_row: Option<TraceSourceRow>,
    },
    FilterDecision {
        scope: TraceScope,
        node: NodeId,
        phase: TraceFilterPhase,
        positions: Vec<TracePosition>,
        passed: bool,
    },
    SortCandidate {
        scope: TraceScope,
        positions: Vec<TracePosition>,
        keys: Vec<TraceSortKey>,
    },
    SortPosition {
        scope: TraceScope,
        positions: Vec<TracePosition>,
        output_index: usize,
    },
    GroupProduced {
        scope: TraceScope,
        grouping: TraceGrouping,
        group_index: usize,
        member_count: usize,
        key: Option<TraceValue>,
        retained: bool,
        positions: Vec<TracePosition>,
    },
    WindowApplied {
        scope: TraceScope,
        window_index: usize,
        window: TraceWindow,
        before: usize,
        after: usize,
    },
    TargetFieldWritten {
        scope: TraceScope,
        field: String,
        binding: TraceTargetFieldBinding,
        positions: Vec<TracePosition>,
        kind: TraceOutputKind,
        value: Option<TraceValue>,
    },
    TargetProduced {
        scope: TraceScope,
        positions: Vec<TracePosition>,
        output_path: Option<String>,
        kind: TraceOutputKind,
    },
    ScopeFinished {
        scope: TraceScope,
        candidates: usize,
        produced: usize,
        kind: TraceOutputKind,
    },
}

/// Receives deterministic interpreter events in evaluation order.
///
/// The callback is synchronous. Implementations that retain events should use
/// interior mutability because execution only needs a shared sink reference.
pub trait TraceSink {
    fn record(&self, event: TraceEvent);

    /// Opts into borrowed ownership context for main-graph value/input events.
    /// Returning false avoids per-event projection; it may change during a run.
    fn wants_owned_context(&self) -> bool {
        false
    }

    /// Selects event kinds that should receive borrowed ownership context.
    /// The default preserves existing value/input opt-ins; other kinds require
    /// an explicit request. Returning false avoids per-event projection.
    fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
        matches!(
            event,
            TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
        ) && self.wants_owned_context()
    }

    /// Receives the original event exactly once with synchronous borrowed context.
    /// Projection and snapshot failures are observer metadata, not execution errors.
    /// A sink that retains context must use its bounded snapshot API.
    fn record_with_owned_context(
        &self,
        event: TraceEvent,
        context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
    ) {
        let _ = context;
        self.record(event);
    }

    /// Observes one filesystem named-source loader entry, before cache lookup
    /// or acquisition. Names and paths are borrowed, exact request identities.
    /// Cache hits and failed attempts are included; this is not an outcome.
    /// Sinks that retain requests are responsible for their own bounds.
    fn dynamic_source_load_requested(&self, source_name: &str, requested_path: &str) {
        let _ = (source_name, requested_path);
    }

    /// Whether constructing a bounded source-row snapshot can be useful to
    /// this sink. Streaming sinks keep the default; bounded collectors may
    /// decline after their retained prefix is full.
    fn wants_source_row(&self) -> bool {
        true
    }
}

pub(crate) fn record(sink: Option<&dyn TraceSink>, event: impl FnOnce() -> TraceEvent) {
    if let Some(sink) = sink {
        sink.record(event());
    }
}

pub(crate) fn trace_positions(positions: &[PositionFrame]) -> Vec<TracePosition> {
    positions.iter().map(TracePosition::from).collect()
}

pub(crate) fn bounded_text(value: &str) -> String {
    truncate_chars(value, MAX_TRACE_PREVIEW_CHARS).0
}

pub(crate) fn record_node_value(
    program: crate::eval_expr::EvalProgram<'_>,
    node: NodeId,
    positions: &[PositionFrame],
    value: &Value,
) {
    if program.trace_sink.is_none() {
        return;
    }
    record_owned_event(program, positions, || TraceEvent::NodeValue {
        node,
        positions: positions.iter().map(TracePosition::from).collect(),
        value: TraceValue::new(value),
    });
}

pub(crate) fn record_node_input_value(
    program: crate::eval_expr::EvalProgram<'_>,
    consumer: NodeId,
    input: NodeId,
    input_index: usize,
    positions: &[PositionFrame],
    value: &Value,
) {
    if program.trace_sink.is_none() {
        return;
    }
    record_owned_event(program, positions, || TraceEvent::NodeInputValue {
        consumer,
        input,
        input_index,
        positions: positions.iter().map(TracePosition::from).collect(),
        value: TraceValue::new(value),
    });
}

pub(crate) fn record_function_node_value(
    sink: Option<&dyn TraceSink>,
    function: FunctionId,
    node: NodeId,
    positions: &[PositionFrame],
    value: &Value,
) {
    let Some(sink) = sink else {
        return;
    };
    sink.record(TraceEvent::FunctionNodeValue {
        function,
        node,
        positions: positions.iter().map(TracePosition::from).collect(),
        value: TraceValue::new(value),
    });
}

pub(crate) fn record_function_node_input_value(
    sink: Option<&dyn TraceSink>,
    function: FunctionId,
    consumer: NodeId,
    input: NodeId,
    input_index: usize,
    positions: &[PositionFrame],
    value: &Value,
) {
    let Some(sink) = sink else {
        return;
    };
    sink.record(TraceEvent::FunctionNodeInputValue {
        function,
        consumer,
        input,
        input_index,
        positions: positions.iter().map(TracePosition::from).collect(),
        value: TraceValue::new(value),
    });
}

fn truncate_chars(value: &str, limit: usize) -> (String, bool) {
    let mut chars = value.chars();
    let preview = chars.by_ref().take(limit).collect::<String>();
    let truncated = chars.next().is_some();
    (preview, truncated)
}

fn bounded_row_value(value: &Value, byte_limit: usize) -> TraceValue {
    let mut preview = TraceValue::new(value);
    let (text, truncated) = truncate_bytes(&preview.preview, byte_limit);
    preview.preview = text;
    preview.truncated |= truncated;
    preview
}

fn truncate_bytes(value: &str, limit: usize) -> (String, bool) {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), end < value.len())
}

/// Evaluation position space supplied by the caller, never inferred from an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOwnedPhase {
    Structural,
    Selection,
    Output,
}

/// Provenance domain supplied by the caller; only Basic is supported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOwnedDomain {
    Basic,
    Grouped,
    Joined,
    Documents,
    Generated,
    Dynamic,
    Function,
}

/// Exact source declaration identity. A named owner is not a wrapper field path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOwnedOwnerRef<'a> {
    Primary,
    Named {
        declaration_index: usize,
        name: &'a str,
    },
}

/// One original input, borrowed from the current evaluator's root registry.
#[derive(Clone, Copy)]
pub struct TraceOwnedRootRef<'a> {
    pub owner: TraceOwnedOwnerRef<'a>,
    /// None means unavailable; a supplied scalar null is still Some.
    pub input: Option<&'a Instance>,
}

/// One active occurrence, in outer-to-inner ancestry order.
#[derive(Clone, Copy)]
pub struct TraceOwnedFrameRef<'a> {
    pub owner: TraceOwnedOwnerRef<'a>,
    pub collection: &'a [String],
    pub item: &'a Instance,
    pub position_slot: usize,
}

/// One canonical source origin retained by the current evaluator.
#[derive(Clone, Copy)]
pub struct TraceOwnedOriginRef<'a> {
    pub owner: TraceOwnedOwnerRef<'a>,
    pub path: &'a [String],
    pub item: &'a Instance,
}

/// Raw legacy position metadata, borrowed independently of canonical owned paths.
#[derive(Clone, Copy)]
pub struct TraceOwnedPositionRef<'a> {
    pub collection: &'a [String],
    pub index: usize,
    pub grouped: bool,
    pub join: Option<JoinId>,
    pub join_position: Option<(JoinId, usize)>,
    pub document_path: Option<&'a str>,
}

impl<'a> From<&'a PositionFrame> for TraceOwnedPositionRef<'a> {
    fn from(position: &'a PositionFrame) -> Self {
        Self {
            collection: &position.collection,
            index: position.index,
            grouped: position.grouped,
            join: position.join,
            join_position: position.join_position,
            document_path: position.document_path.as_deref(),
        }
    }
}

impl<'a> From<&'a TracePosition> for TraceOwnedPositionRef<'a> {
    fn from(position: &'a TracePosition) -> Self {
        Self {
            collection: &position.collection,
            index: position.index,
            grouped: position.grouped,
            join: position.join,
            join_position: position.join_position,
            document_path: position.document_path.as_deref(),
        }
    }
}

/// Borrowed Basic ownership metadata, suitable for a synchronous observer.
///
/// The evaluator supplies these bindings from its existing registry and active
/// frames. This API performs no source lookup, iteration, or expression access.
/// Input references cannot outlive their original storage. Snapshotting copies
/// only bounded metadata and scalar previews, never source instance trees.
/// Descriptor producers must supply authentic registry/occurrence references;
/// validation checks declaration and slot metadata, not pointer ancestry by
/// traversing inputs. No supplied descriptor is treated as an execution capability.
/// This foundation does not enable owned tracing or debugging in execution.
pub struct TraceOwnedWitness<'a> {
    pub project: &'a mapping::Project,
    pub domain: TraceOwnedDomain,
    pub phase: TraceOwnedPhase,
    pub roots: &'a [TraceOwnedRootRef<'a>],
    pub frames: &'a [TraceOwnedFrameRef<'a>],
    pub origins: &'a [TraceOwnedOriginRef<'a>],
    pub positions: &'a [TraceOwnedPositionRef<'a>],
    pub owned_ancestry: bool,
}

/// The location of malformed or unsupported supplied witness metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceOwnedLocation {
    Context,
    Root(usize),
    Frame(usize),
    Origin(usize),
    Position(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceOwnedWitnessError {
    #[error("owned trace witness exceeds {resource} limit {limit}")]
    ResourceLimit {
        resource: &'static str,
        limit: usize,
    },
    #[error("unknown source declaration at {at:?}")]
    UnknownDeclaration { at: TraceOwnedLocation },
    #[error("source declaration name mismatch at {at:?}")]
    DeclarationNameMismatch { at: TraceOwnedLocation },
    #[error("ambiguous source declaration at {at:?}")]
    AmbiguousDeclaration { at: TraceOwnedLocation },
    #[error("duplicate root binding at {at:?}")]
    DuplicateRoot { at: TraceOwnedLocation },
    #[error("no root binding for owned witness at {at:?}")]
    MissingRoot { at: TraceOwnedLocation },
    #[error("unavailable source for owned witness at {at:?}")]
    UnavailableRoot { at: TraceOwnedLocation },
    #[error("invalid owned frame position slot {slot} at {at:?}")]
    InvalidPositionSlot { at: TraceOwnedLocation, slot: usize },
    #[error("owned trace witness does not support {domain} at {at:?}")]
    UnsupportedDomain {
        at: TraceOwnedLocation,
        domain: &'static str,
    },
}

/// A bounded path display; omitted segments remain explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedPath {
    pub segments: Vec<String>,
    pub truncated_segments: Vec<bool>,
    pub omitted_segments: usize,
}

/// Exact endpoint identity retained even when its display name is truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceOwnedOwner {
    Primary,
    Named {
        declaration_index: usize,
        name: String,
        name_truncated: bool,
    },
}

/// A bounded description, without traversing structural instance children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedValue {
    pub kind: TraceOutputKind,
    pub scalar: Option<TraceValue>,
    pub child_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedRoot {
    pub owner: TraceOwnedOwner,
    pub input: Option<TraceOwnedValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedFrame {
    pub owner: TraceOwnedOwner,
    pub collection: TraceOwnedPath,
    pub item: TraceOwnedValue,
    /// Indexes the original witness's positions, including any omitted tail.
    pub position_slot: usize,
    /// The copied index remains available even when its raw position is omitted.
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedOrigin {
    pub owner: TraceOwnedOwner,
    pub path: TraceOwnedPath,
    pub item: TraceOwnedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedPosition {
    pub collection: TraceOwnedPath,
    pub index: usize,
}

/// A retained witness with hard metadata and preview bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceOwnedSnapshot {
    pub phase: TraceOwnedPhase,
    pub owned_ancestry: bool,
    pub roots: Vec<TraceOwnedRoot>,
    pub frames: Vec<TraceOwnedFrame>,
    pub origins: Vec<TraceOwnedOrigin>,
    pub positions: Vec<TraceOwnedPosition>,
    pub omitted_roots: usize,
    pub omitted_frames: usize,
    pub omitted_origins: usize,
    pub omitted_positions: usize,
}

const MAX_TRACE_OWNED_ENTRIES: usize = 4096;
const MAX_TRACE_OWNED_PATH_SEGMENTS: usize = 256;
const MAX_TRACE_OWNED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACE_OWNED_VISIBLE_ENTRIES: usize = 32;
const MAX_TRACE_OWNED_VISIBLE_PATH_SEGMENTS: usize = 16;
const MAX_TRACE_OWNED_NAME_CHARS: usize = 80;

impl TraceOwnedWitness<'_> {
    /// Validates every supplied entry before returning a bounded retained snapshot.
    /// Validation never consumes the evaluator's execution/work budget.
    pub fn snapshot(&self) -> Result<TraceOwnedSnapshot, TraceOwnedWitnessError> {
        self.validate()?;
        let visible = MAX_TRACE_OWNED_VISIBLE_ENTRIES;
        Ok(TraceOwnedSnapshot {
            phase: self.phase,
            owned_ancestry: self.owned_ancestry,
            roots: self
                .roots
                .iter()
                .take(visible)
                .map(|root| TraceOwnedRoot {
                    owner: owned_owner_snapshot(root.owner),
                    input: root.input.map(owned_value_snapshot),
                })
                .collect(),
            frames: self
                .frames
                .iter()
                .take(visible)
                .map(|frame| TraceOwnedFrame {
                    owner: owned_owner_snapshot(frame.owner),
                    collection: owned_path_snapshot(frame.collection),
                    item: owned_value_snapshot(frame.item),
                    position_slot: frame.position_slot,
                    index: self.positions[frame.position_slot].index,
                })
                .collect(),
            origins: self
                .origins
                .iter()
                .take(visible)
                .map(|origin| TraceOwnedOrigin {
                    owner: owned_owner_snapshot(origin.owner),
                    path: owned_path_snapshot(origin.path),
                    item: owned_value_snapshot(origin.item),
                })
                .collect(),
            positions: self
                .positions
                .iter()
                .take(visible)
                .map(|position| TraceOwnedPosition {
                    collection: owned_path_snapshot(position.collection),
                    index: position.index,
                })
                .collect(),
            omitted_roots: self.roots.len().saturating_sub(visible),
            omitted_frames: self.frames.len().saturating_sub(visible),
            omitted_origins: self.origins.len().saturating_sub(visible),
            omitted_positions: self.positions.len().saturating_sub(visible),
        })
    }

    fn validate(&self) -> Result<(), TraceOwnedWitnessError> {
        let domain = match self.domain {
            TraceOwnedDomain::Basic => None,
            TraceOwnedDomain::Grouped => Some("grouped source"),
            TraceOwnedDomain::Joined => Some("joined source"),
            TraceOwnedDomain::Documents => Some("document source"),
            TraceOwnedDomain::Generated => Some("generated source"),
            TraceOwnedDomain::Dynamic => Some("dynamic source"),
            TraceOwnedDomain::Function => Some("function source"),
        };
        if let Some(domain) = domain {
            return Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Context,
                domain,
            });
        }
        let declarations = self.project.extra_sources.len().checked_add(1).ok_or(
            TraceOwnedWitnessError::ResourceLimit {
                resource: "declaration entries",
                limit: MAX_TRACE_OWNED_ENTRIES,
            },
        )?;
        for (resource, count) in [
            ("root entries", self.roots.len()),
            ("frame entries", self.frames.len()),
            ("origin entries", self.origins.len()),
            ("position entries", self.positions.len()),
            ("declaration entries", declarations),
        ] {
            if count > MAX_TRACE_OWNED_ENTRIES {
                return Err(TraceOwnedWitnessError::ResourceLimit {
                    resource,
                    limit: MAX_TRACE_OWNED_ENTRIES,
                });
            }
        }
        let mut text_bytes = 0usize;
        for source in &self.project.extra_sources {
            charge_owned_text(source.name.len(), &mut text_bytes)?;
        }
        for (index, root) in self.roots.iter().enumerate() {
            let at = TraceOwnedLocation::Root(index);
            self.validate_owner(root.owner, at, &mut text_bytes)?;
            if self.roots[..index]
                .iter()
                .any(|other| other.owner == root.owner)
            {
                return Err(TraceOwnedWitnessError::DuplicateRoot { at });
            }
            if let Some(input) = root.input {
                validate_owned_value(input, at)?;
            }
        }
        for (index, frame) in self.frames.iter().enumerate() {
            let at = TraceOwnedLocation::Frame(index);
            self.validate_binding(frame.owner, at, &mut text_bytes)?;
            validate_owned_path(frame.collection, &mut text_bytes)?;
            validate_owned_value(frame.item, at)?;
            if frame.position_slot >= self.positions.len() {
                return Err(TraceOwnedWitnessError::InvalidPositionSlot {
                    at,
                    slot: frame.position_slot,
                });
            }
        }
        for (index, origin) in self.origins.iter().enumerate() {
            let at = TraceOwnedLocation::Origin(index);
            self.validate_binding(origin.owner, at, &mut text_bytes)?;
            validate_owned_path(origin.path, &mut text_bytes)?;
            validate_owned_value(origin.item, at)?;
        }
        for (index, position) in self.positions.iter().enumerate() {
            let at = TraceOwnedLocation::Position(index);
            validate_owned_path(position.collection, &mut text_bytes)?;
            let domain = if position.grouped {
                Some("grouped source")
            } else if position.join.is_some() || position.join_position.is_some() {
                Some("joined source")
            } else if position.document_path.is_some() {
                Some("document source")
            } else {
                None
            };
            if let Some(domain) = domain {
                return Err(TraceOwnedWitnessError::UnsupportedDomain { at, domain });
            }
        }
        Ok(())
    }

    fn validate_owner(
        &self,
        owner: TraceOwnedOwnerRef<'_>,
        at: TraceOwnedLocation,
        text_bytes: &mut usize,
    ) -> Result<(), TraceOwnedWitnessError> {
        if let TraceOwnedOwnerRef::Named {
            declaration_index,
            name,
        } = owner
        {
            charge_owned_text(name.len(), text_bytes)?;
            let source = self
                .project
                .extra_sources
                .get(declaration_index)
                .ok_or(TraceOwnedWitnessError::UnknownDeclaration { at })?;
            if source.name != name || name.is_empty() {
                return Err(TraceOwnedWitnessError::DeclarationNameMismatch { at });
            }
            if self
                .project
                .extra_sources
                .iter()
                .filter(|source| source.name == name)
                .count()
                != 1
            {
                return Err(TraceOwnedWitnessError::AmbiguousDeclaration { at });
            }
            if source.dynamic_path.is_some() {
                return Err(TraceOwnedWitnessError::UnsupportedDomain {
                    at,
                    domain: "dynamic source",
                });
            }
        }
        Ok(())
    }

    fn validate_binding(
        &self,
        owner: TraceOwnedOwnerRef<'_>,
        at: TraceOwnedLocation,
        text_bytes: &mut usize,
    ) -> Result<(), TraceOwnedWitnessError> {
        self.validate_owner(owner, at, text_bytes)?;
        match self.roots.iter().find(|root| root.owner == owner) {
            None => Err(TraceOwnedWitnessError::MissingRoot { at }),
            Some(root) if root.input.is_none() => {
                Err(TraceOwnedWitnessError::UnavailableRoot { at })
            }
            Some(_) => Ok(()),
        }
    }
}

fn charge_owned_text(bytes: usize, total: &mut usize) -> Result<(), TraceOwnedWitnessError> {
    *total = total
        .checked_add(bytes)
        .filter(|total| *total <= MAX_TRACE_OWNED_TEXT_BYTES)
        .ok_or(TraceOwnedWitnessError::ResourceLimit {
            resource: "metadata bytes",
            limit: MAX_TRACE_OWNED_TEXT_BYTES,
        })?;
    Ok(())
}

fn validate_owned_path(path: &[String], total: &mut usize) -> Result<(), TraceOwnedWitnessError> {
    if path.len() > MAX_TRACE_OWNED_PATH_SEGMENTS {
        return Err(TraceOwnedWitnessError::ResourceLimit {
            resource: "path segments",
            limit: MAX_TRACE_OWNED_PATH_SEGMENTS,
        });
    }
    for segment in path {
        charge_owned_text(segment.len(), total)?;
    }
    Ok(())
}

fn validate_owned_value(
    input: &Instance,
    at: TraceOwnedLocation,
) -> Result<(), TraceOwnedWitnessError> {
    let domain = match input {
        Instance::MappedSequence(_) => Some("generated source"),
        Instance::DocumentSet(_) => Some("document source"),
        _ => None,
    };
    match domain {
        Some(domain) => Err(TraceOwnedWitnessError::UnsupportedDomain { at, domain }),
        None => Ok(()),
    }
}

fn owned_owner_snapshot(owner: TraceOwnedOwnerRef<'_>) -> TraceOwnedOwner {
    match owner {
        TraceOwnedOwnerRef::Primary => TraceOwnedOwner::Primary,
        TraceOwnedOwnerRef::Named {
            declaration_index,
            name,
        } => {
            let (name, name_truncated) = truncate_chars(name, MAX_TRACE_OWNED_NAME_CHARS);
            TraceOwnedOwner::Named {
                declaration_index,
                name,
                name_truncated,
            }
        }
    }
}

fn owned_path_snapshot(path: &[String]) -> TraceOwnedPath {
    let visible = path.len().min(MAX_TRACE_OWNED_VISIBLE_PATH_SEGMENTS);
    let (segments, truncated_segments) = path
        .iter()
        .take(visible)
        .map(|segment| truncate_chars(segment, MAX_TRACE_OWNED_NAME_CHARS))
        .unzip();
    TraceOwnedPath {
        segments,
        truncated_segments,
        omitted_segments: path.len() - visible,
    }
}

fn owned_value_snapshot(input: &Instance) -> TraceOwnedValue {
    TraceOwnedValue {
        kind: TraceOutputKind::of(input),
        scalar: match input {
            Instance::Scalar(value) => Some(TraceValue::new(value)),
            _ => None,
        },
        child_count: match input {
            Instance::Group(fields) => Some(fields.len()),
            Instance::Repeated(items) => Some(items.len()),
            _ => None,
        },
    }
}

/// Failure to project the evaluator's registry for observation only.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceOwnedContextError {
    #[error("owned trace project does not match the current registry")]
    MismatchedRegistry,
    #[error("owned trace registry declaration {declaration_index}: {source}")]
    Registry {
        declaration_index: usize,
        source: crate::OwnedSourceError,
    },
    #[error(transparent)]
    Witness(#[from] TraceOwnedWitnessError),
}

/// Borrowing this interface lets a narrowed item program reuse a run-level cache.
/// The cache itself contains immutable input references and never escapes the run.
pub(crate) trait TraceOwnedRegistryView {
    fn project(&self) -> &mapping::Project;
    fn roots(&self) -> Result<&[TraceOwnedRootRef<'_>], &TraceOwnedContextError>;
    fn owner(
        &self,
        key: crate::owned_context::OwnerKey,
        at: TraceOwnedLocation,
    ) -> Result<TraceOwnedOwnerRef<'_>, TraceOwnedContextError>;
}

pub(crate) struct TraceOwnedRegistry<'a> {
    project: &'a mapping::Project,
    registry: &'a crate::owned_context::SourceRoots<'a>,
    pub(crate) cache:
        std::cell::OnceCell<Result<Vec<TraceOwnedRootRef<'a>>, TraceOwnedContextError>>,
}

impl<'a> TraceOwnedRegistry<'a> {
    pub(crate) fn new(
        project: &'a mapping::Project,
        registry: &'a crate::owned_context::SourceRoots<'a>,
    ) -> Self {
        Self {
            project,
            registry,
            cache: std::cell::OnceCell::new(),
        }
    }

    fn project_roots(&self) -> Result<Vec<TraceOwnedRootRef<'a>>, TraceOwnedContextError> {
        use crate::owned_context::OwnerKey;
        if !std::ptr::eq(
            self.project.extra_sources.as_slice(),
            self.registry.declarations(),
        ) {
            return Err(TraceOwnedContextError::MismatchedRegistry);
        }
        let mut roots = Vec::with_capacity(self.registry.declarations().len() + 1);
        roots.push(TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Primary,
            input: Some(self.registry.primary),
        });
        for (declaration_index, source) in self.registry.declarations().iter().enumerate() {
            // Only the already-supplied static registry is inspected. No loader runs.
            if source.dynamic_path.is_some() {
                continue;
            }
            let input = match self.registry.root(OwnerKey::Named(declaration_index)) {
                Ok(input) => Some(input),
                Err(crate::OwnedSourceError::SourceUnavailable { .. }) => None,
                Err(source) => {
                    return Err(TraceOwnedContextError::Registry {
                        declaration_index,
                        source,
                    });
                }
            };
            roots.push(TraceOwnedRootRef {
                owner: TraceOwnedOwnerRef::Named {
                    declaration_index,
                    name: &source.name,
                },
                input,
            });
        }
        Ok(roots)
    }
}

impl TraceOwnedRegistryView for TraceOwnedRegistry<'_> {
    fn project(&self) -> &mapping::Project {
        self.project
    }

    fn roots(&self) -> Result<&[TraceOwnedRootRef<'_>], &TraceOwnedContextError> {
        match self.cache.get_or_init(|| self.project_roots()) {
            Ok(roots) => Ok(roots),
            Err(error) => Err(error),
        }
    }

    fn owner(
        &self,
        key: crate::owned_context::OwnerKey,
        at: TraceOwnedLocation,
    ) -> Result<TraceOwnedOwnerRef<'_>, TraceOwnedContextError> {
        match key {
            crate::owned_context::OwnerKey::Primary => Ok(TraceOwnedOwnerRef::Primary),
            crate::owned_context::OwnerKey::Named(declaration_index) => {
                let source = self
                    .registry
                    .declarations()
                    .get(declaration_index)
                    .ok_or(TraceOwnedWitnessError::UnknownDeclaration { at })?;
                Ok(TraceOwnedOwnerRef::Named {
                    declaration_index,
                    name: &source.name,
                })
            }
        }
    }
}

pub(crate) fn record_owned_event(
    program: crate::eval_expr::EvalProgram<'_>,
    positions: &[PositionFrame],
    event: impl FnOnce() -> TraceEvent,
) {
    let Some(sink) = program.trace_sink else {
        return;
    };
    let event = event();
    if program.owned_context.is_none() || program.owned_trace_registry.is_none() {
        sink.record(event);
        return;
    }
    if !sink.wants_owned_context_for(&event) {
        sink.record(event);
        return;
    }
    with_owned_witness(program, positions, |context| {
        sink.record_with_owned_context(event, context)
    });
}

/// Projects the existing Basic borrowed view only after a consumer opts in.
/// Callers have already checked that both admitted context and registry exist.
/// The callback cannot retain its borrowed view; bounded snapshots are explicit.
pub(crate) fn with_owned_witness<R>(
    program: crate::eval_expr::EvalProgram<'_>,
    positions: &[PositionFrame],
    observe: impl FnOnce(Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>) -> R,
) -> R {
    let owned = program
        .owned_context
        .expect("admitted owned observer context");
    let registry = program
        .owned_trace_registry
        .expect("admitted owned observer registry");
    let roots = match registry.roots() {
        Ok(roots) => roots,
        Err(error) => {
            return observe(Err(error));
        }
    };
    let frames = owned
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            Ok(TraceOwnedFrameRef {
                owner: registry.owner(frame.owner, TraceOwnedLocation::Frame(index))?,
                collection: &frame.collection,
                item: frame.item,
                position_slot: frame.position_slot,
            })
        })
        .collect::<Result<Vec<_>, TraceOwnedContextError>>();
    let frames = match frames {
        Ok(frames) => frames,
        Err(error) => {
            return observe(Err(&error));
        }
    };
    let origins = owned
        .origins
        .iter()
        .enumerate()
        .map(|(index, origin)| {
            Ok(TraceOwnedOriginRef {
                owner: registry.owner(origin.owner, TraceOwnedLocation::Origin(index))?,
                path: &origin.path,
                item: origin.item,
            })
        })
        .collect::<Result<Vec<_>, TraceOwnedContextError>>();
    let origins = match origins {
        Ok(origins) => origins,
        Err(error) => {
            return observe(Err(&error));
        }
    };
    let positions = positions
        .iter()
        .map(TraceOwnedPositionRef::from)
        .collect::<Vec<_>>();
    observe(Ok(TraceOwnedWitness {
        project: registry.project(),
        domain: TraceOwnedDomain::Basic,
        phase: program.owned_trace_phase,
        roots,
        frames: &frames,
        origins: &origins,
        positions: &positions,
        owned_ancestry: owned.owned_ancestry,
    }))
}
