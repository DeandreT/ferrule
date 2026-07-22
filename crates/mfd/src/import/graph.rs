use std::collections::{BTreeMap, BTreeSet};

use ir::Value;
use mapping::{Graph, Node, NodeId};

use super::function::FnComponent;
use super::schema::{SchemaComponent, parse_u32};
use super::udf::{Call as UdfCall, Registry as UdfRegistry};

pub(super) struct GraphBuilder<'a> {
    pub(super) graph: Graph,
    pub(super) next_id: NodeId,
    pub(super) fn_nodes: BTreeMap<usize, NodeId>,
    pub(super) sequence_items: BTreeMap<usize, NodeId>,
    pub(super) sequence_scope_components: BTreeSet<usize>,
    pub(super) sequence_predicate_components: BTreeSet<usize>,
    pub(super) warned_sequence_uses: BTreeSet<usize>,
    pub(super) warned_scalar_filters: BTreeSet<usize>,
    pub(super) warned_join_controls: BTreeSet<mapping::JoinId>,
    pub(super) rejected_join_paths: BTreeSet<Vec<String>>,
    pub(super) source_fields: BTreeMap<(Option<Vec<String>>, Vec<String>), NodeId>,
    pub(super) json_serializer_nodes: BTreeMap<u32, NodeId>,
    pub(super) xml_serializer_nodes: BTreeMap<u32, NodeId>,
    pub(super) external_scalar_nodes: BTreeMap<u32, NodeId>,
    pub(super) external_xslt_nodes: BTreeMap<u32, NodeId>,
    pub(super) json_parser_nodes: BTreeMap<u32, NodeId>,
    pub(super) flextext_parser_nodes: BTreeMap<u32, NodeId>,
    pub(super) source_node_function_nodes: BTreeMap<u32, NodeId>,
    pub(super) claimed_dynamic_ports: BTreeSet<u32>,
    pub(super) query_scope_sources: BTreeSet<usize>,
    pub(super) warned_unscoped_queries: BTreeSet<usize>,
    pub(super) xml_type_conditions: BTreeMap<u32, String>,
    pub(super) edge_from: &'a BTreeMap<u32, u32>,
    pub(super) sources: &'a [&'a SchemaComponent],
    pub(super) source_names: &'a [String],
    pub(super) intermediates: &'a [&'a SchemaComponent],
    pub(super) json_serializers: &'a [super::json_serializer::Recipe],
    pub(super) xml_serializers: &'a [super::xml_serializer::Recipe],
    pub(super) external_scalar_recipes: &'a [super::external_scalar::Recipe],
    pub(super) external_xslt_aggregates: &'a [super::external_xslt::Recipe],
    pub(super) json_parsers: &'a [super::json_parser::Recipe],
    pub(super) flextext_parsers: &'a [super::flextext_parser::Recipe],
    pub(super) source_node_functions: &'a super::source_node_function::Rules,
    pub(super) fn_components: &'a [FnComponent],
    pub(super) fn_by_output: BTreeMap<u32, usize>,
    pub(super) udf_nodes: BTreeMap<u32, NodeId>,
    pub(super) udf_by_output: BTreeMap<u32, (usize, u32)>,
    pub(super) udf_calls: &'a [UdfCall],
    pub(super) udf_registry: &'a UdfRegistry,
    pub(super) joins: super::join::Registry,
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

    pub(super) fn group_member_value(&mut self, node: NodeId) -> NodeId {
        // Grouped DTD ports are sparse aliases on generic element members.
        // Preserve other expression shapes instead of guessing their context.
        let source = match self.graph.nodes.get(&node) {
            Some(Node::SourceField { .. }) => node,
            Some(Node::CollectionFind { value, .. }) => *value,
            _ => return node,
        };
        let Some(Node::SourceField { path, .. }) = self.graph.nodes.get(&source).cloned() else {
            return node;
        };
        let local = self.source_field(None, path);
        let present = self.alloc(Node::Call {
            function: "exists".into(),
            args: vec![local],
        });
        self.alloc(Node::CollectionFind {
            collection: Vec::new(),
            predicate: present,
            value: local,
        })
    }

    pub(super) fn database_xml_column_node(
        &mut self,
        feed: u32,
        column: &super::schema::database_xml::Column,
    ) -> Option<NodeId> {
        let source = self.sequence_source_path(feed)?;
        let (frame, path) = self.source_location_at(&source)?;
        Some(self.alloc(Node::XmlSerialize {
            path,
            frame,
            schema: column.schema.clone(),
            declaration: false,
            indent: false,
            namespace: column.namespace.clone(),
        }))
    }
}

pub(super) fn read_edges(
    structure: &roxmltree::Node<'_, '_>,
    legacy_parent: Option<&roxmltree::Node<'_, '_>>,
) -> BTreeMap<u32, u32> {
    let mut edge_from = BTreeMap::new();
    if let Some(graph) = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("graph"))
    {
        for vertex in graph
            .descendants()
            .filter(|node| node.has_tag_name("vertex"))
        {
            let Some(from) = parse_u32(vertex.attribute("vertexkey")) else {
                continue;
            };
            for edge in vertex
                .descendants()
                .filter(|node| node.has_tag_name("edge"))
            {
                if let Some(to) = parse_u32(edge.attribute("vertexkey")) {
                    edge_from.insert(to, from);
                }
            }
        }
    }
    let connections = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("connections"))
        .or_else(|| {
            legacy_parent.and_then(|parent| {
                parent
                    .children()
                    .find(|node| node.is_element() && node.has_tag_name("connections"))
            })
        });
    if let Some(connections) = connections {
        for edge in connections
            .children()
            .filter(|node| node.is_element() && node.has_tag_name("edge"))
        {
            if let (Some(from), Some(to)) = (
                parse_u32(edge.attribute("from")),
                parse_u32(edge.attribute("to")),
            ) {
                edge_from.insert(to, from);
            }
        }
    }
    edge_from
}

pub(super) fn read_copy_all_targets(
    structure: &roxmltree::Node<'_, '_>,
    legacy_parent: Option<&roxmltree::Node<'_, '_>>,
) -> BTreeSet<u32> {
    let modern = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("graph"))
        .map(|graph| {
            let copy_edges = graph
                .children()
                .find(|node| node.is_element() && node.has_tag_name("edges"))
                .into_iter()
                .flat_map(|edges| edges.children().filter(|node| node.has_tag_name("edge")))
                .filter(|edge| {
                    edge.descendants().any(|node| {
                        node.has_tag_name("dataconnection") && node.attribute("type") == Some("2")
                    })
                })
                .filter_map(|edge| super::schema::parse_u32(edge.attribute("edgekey")))
                .collect::<BTreeSet<_>>();
            graph
                .descendants()
                .filter(|node| node.has_tag_name("vertex"))
                .flat_map(|vertex| {
                    vertex
                        .descendants()
                        .filter(|node| node.has_tag_name("edge"))
                })
                .filter(|edge| {
                    super::schema::parse_u32(edge.attribute("edgekey"))
                        .is_some_and(|key| copy_edges.contains(&key))
                })
                .filter_map(|edge| super::schema::parse_u32(edge.attribute("vertexkey")))
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let legacy = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("connections"))
        .or_else(|| {
            legacy_parent.and_then(|parent| {
                parent
                    .children()
                    .find(|node| node.is_element() && node.has_tag_name("connections"))
            })
        })
        .into_iter()
        .flat_map(|connections| {
            connections
                .children()
                .filter(|node| node.is_element() && node.has_tag_name("edge"))
        })
        .filter(|edge| {
            edge.children().any(|node| {
                node.is_element()
                    && node.has_tag_name("data")
                    && node.attribute("type") == Some("2")
            })
        })
        .filter_map(|edge| super::schema::parse_u32(edge.attribute("to")));
    modern.into_iter().chain(legacy).collect()
}
