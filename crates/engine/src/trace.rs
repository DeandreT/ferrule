use ir::Value;
use mapping::{JoinId, NodeId, ScopeIteration, SequenceExpr};

use crate::source_iteration::PositionFrame;

const MAX_TRACE_PREVIEW_CHARS: usize = 160;

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
}

impl TraceScope {
    pub(crate) fn primary() -> Self {
        Self {
            target: TraceTarget::Primary,
            target_path: Vec::new(),
            structural_path: Vec::new(),
        }
    }

    pub(crate) fn named(target: impl Into<String>) -> Self {
        Self {
            target: TraceTarget::Named(target.into()),
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
            target: self.target.clone(),
            target_path,
            structural_path,
        }
    }

    pub(crate) fn segment(&self, index: usize) -> Self {
        let mut structural_path = self.structural_path.clone();
        structural_path.push(index);
        Self {
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
    Source { path: Vec<String> },
    DynamicDocuments { source: Vec<String> },
    Generated { kind: &'static str },
    Join { join: JoinId },
    Concatenate { segments: usize },
}

pub(crate) fn scope_iteration(iteration: &ScopeIteration) -> TraceIteration {
    match iteration {
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
    }
}

fn sequence_kind(sequence: &SequenceExpr) -> &'static str {
    match sequence {
        SequenceExpr::Tokenize { .. } => "tokenize",
        SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        SequenceExpr::TokenizeRegex { .. } => "tokenize-regex",
        SequenceExpr::Generate { .. } => "integer-range",
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

/// A successful, observable step in interpreter evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    NodeValue {
        node: NodeId,
        positions: Vec<TracePosition>,
        value: Value,
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
    sink: Option<&dyn TraceSink>,
    node: NodeId,
    positions: &[PositionFrame],
    value: &Value,
) {
    let Some(sink) = sink else {
        return;
    };
    sink.record(TraceEvent::NodeValue {
        node,
        positions: positions.iter().map(TracePosition::from).collect(),
        value: value.clone(),
    });
}

fn truncate_chars(value: &str, limit: usize) -> (String, bool) {
    let mut chars = value.chars();
    let preview = chars.by_ref().take(limit).collect::<String>();
    let truncated = chars.next().is_some();
    (preview, truncated)
}
