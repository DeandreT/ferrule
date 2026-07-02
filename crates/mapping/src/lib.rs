//! The mapping graph IR: nodes and connections that describe how a source
//! record is transformed into a target record, plus the project file
//! (source schema + target schema + graph + target field bindings) that
//! gets saved/loaded.

use std::collections::BTreeMap;

use ir::{RecordSchema, Value};
use serde::{Deserialize, Serialize};

pub type NodeId = u32;

/// A single node in the mapping graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    /// Reads a named field out of the source record.
    SourceField { field: String },
    /// A literal value.
    Const { value: Value },
    /// Calls a built-in function (see the `functions` crate) with the
    /// evaluated outputs of the given argument nodes.
    Call { function: String, args: Vec<NodeId> },
}

/// The mapping graph for one project: every node that can be wired into a
/// target field, keyed by id so multiple target fields can share subgraphs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<NodeId, Node>,
}

/// Connects a graph node's output to a named field on the target record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub target_field: String,
    pub node: NodeId,
}

/// A complete mapping project: the source/target shapes, the graph, and
/// which node feeds each target field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    pub source: RecordSchema,
    pub target: RecordSchema,
    pub graph: Graph,
    pub bindings: Vec<Binding>,
}
