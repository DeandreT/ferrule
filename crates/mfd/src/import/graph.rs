use std::collections::{BTreeMap, BTreeSet};

use ir::Value;
use mapping::{Graph, Node, NodeId};

use super::function::FnComponent;
use super::schema::SchemaComponent;

pub(super) struct GraphBuilder<'a> {
    pub(super) graph: Graph,
    pub(super) next_id: NodeId,
    pub(super) fn_nodes: BTreeMap<usize, NodeId>,
    pub(super) sequence_items: BTreeMap<usize, NodeId>,
    pub(super) sequence_scope_components: BTreeSet<usize>,
    pub(super) warned_sequence_uses: BTreeSet<usize>,
    pub(super) source_fields: BTreeMap<(Option<Vec<String>>, Vec<String>), NodeId>,
    pub(super) edge_from: &'a BTreeMap<u32, u32>,
    pub(super) sources: &'a [&'a SchemaComponent],
    pub(super) intermediates: &'a [&'a SchemaComponent],
    pub(super) fn_components: &'a [FnComponent],
    pub(super) fn_by_output: BTreeMap<u32, usize>,
    /// Absolute source paths ending at a repeating node that some scope's
    /// iteration crosses -- i.e. levels that get their own context frame
    /// at run time. SourceField paths are cut after the innermost framed
    /// ancestor; repeating levels no scope iterates stay in the path (the
    /// engine reads their first item).
    pub(super) framed: std::collections::BTreeSet<Vec<String>>,
    pub(super) warnings: Vec<String>,
}

impl GraphBuilder<'_> {
    pub(super) fn alloc(&mut self, node: Node) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.graph.nodes.insert(id, node);
        id
    }

    pub(super) fn const_null(&mut self) -> NodeId {
        self.alloc(Node::Const { value: Value::Null })
    }

    pub(super) fn source_field(&mut self, frame: Option<Vec<String>>, path: Vec<String>) -> NodeId {
        let key = (frame.clone(), path.clone());
        let id = *self.source_fields.entry(key).or_insert_with_key(|_| {
            let id = self.next_id;
            self.next_id += 1;
            id
        });
        self.graph
            .nodes
            .entry(id)
            .or_insert(Node::SourceField { path, frame });
        id
    }

    pub(super) fn sequence_item(&mut self, idx: usize) -> NodeId {
        if let Some(&item) = self.sequence_items.get(&idx) {
            return item;
        }
        let item = self.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        self.sequence_items.insert(idx, item);
        item
    }
}
