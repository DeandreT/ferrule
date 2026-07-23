use ir::Value;
use mapping::{JoinId, NodeId};

use crate::source_iteration::PositionFrame;

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

/// A successful, observable step in interpreter evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    NodeValue {
        node: NodeId,
        positions: Vec<TracePosition>,
        value: Value,
    },
}

/// Receives deterministic interpreter events in evaluation order.
///
/// The callback is synchronous. Implementations that retain events should use
/// interior mutability because execution only needs a shared sink reference.
pub trait TraceSink {
    fn record(&self, event: TraceEvent);
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
