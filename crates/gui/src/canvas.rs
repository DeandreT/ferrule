//! The canvas node model: the mapping graph's nodes plus two special
//! endpoints -- a Source node whose output pins are the source schema's
//! scalar leaves and a Target node whose input pins are the target's --
//! so mappings are wired leaf-to-leaf like a visual mapper, with
//! `SourceField` nodes and `Binding`s maintained behind the wires.

use ir::{SchemaKind, SchemaNode};
use mapping::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasNode {
    Source,
    Target,
    Graph(NodeId),
    /// A GUI-created null producer that keeps a required graph input valid
    /// until the user connects a value. Keeping it on the canvas prevents
    /// the persisted graph and its visual representation from diverging.
    Placeholder(NodeId),
}

/// One scalar leaf of the source schema. `path` is what a `SourceField`
/// should hold to read this leaf: the segments after the innermost
/// repeating ancestor. `frame` identifies that ancestor by its absolute
/// source path so equal relative paths in different collections stay
/// distinct. A repeating document root uses an empty frame path.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLeaf {
    pub label: String,
    pub frame: Option<Vec<String>>,
    pub path: Vec<String>,
}

/// One scalar leaf of the target schema. `chain` is the group-name chain
/// from the root to the leaf's parent -- the scope whose `target_field`
/// chain matches owns the leaf's binding -- and `field` is the binding's
/// target field name.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetLeaf {
    pub label: String,
    pub chain: Vec<String>,
    pub field: String,
}

pub fn source_leaves(schema: &SchemaNode) -> Vec<SourceLeaf> {
    let mut out = Vec::new();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return out;
    };
    let root_frame_len = schema.repeating.then_some(0);
    for child in children {
        collect_source(
            child,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            root_frame_len,
            &mut out,
        );
    }
    out
}

fn collect_source(
    node: &SchemaNode,
    label: &mut Vec<String>,
    absolute: &mut Vec<String>,
    suffix: &mut Vec<String>,
    frame_len: Option<usize>,
    out: &mut Vec<SourceLeaf>,
) {
    label.push(node.name.clone());
    absolute.push(node.name.clone());
    match &node.kind {
        SchemaKind::Scalar { .. } => {
            suffix.push(node.name.clone());
            out.push(SourceLeaf {
                label: label.join("/"),
                frame: frame_len.map(|len| absolute[..len].to_vec()),
                path: suffix.clone(),
            });
            suffix.pop();
        }
        SchemaKind::Group { children, .. } => {
            // Descending into a repeating group resets the suffix: it is
            // the new innermost repeating ancestor.
            let mut fresh = Vec::new();
            let suffix = if node.repeating {
                &mut fresh
            } else {
                &mut *suffix
            };
            suffix.push(node.name.clone());
            if node.repeating {
                suffix.clear();
            }
            let frame_len = if node.repeating {
                Some(absolute.len())
            } else {
                frame_len
            };
            for child in children {
                collect_source(child, label, absolute, suffix, frame_len, out);
            }
            if !node.repeating {
                suffix.pop();
            }
        }
    }
    absolute.pop();
    label.pop();
}

pub fn target_leaves(schema: &SchemaNode) -> Vec<TargetLeaf> {
    let mut out = Vec::new();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return out;
    };
    for child in children {
        collect_target(child, &mut Vec::new(), &mut out);
    }
    out
}

fn collect_target(node: &SchemaNode, chain: &mut Vec<String>, out: &mut Vec<TargetLeaf>) {
    match &node.kind {
        SchemaKind::Scalar { .. } => {
            let mut label = chain.clone();
            label.push(node.name.clone());
            out.push(TargetLeaf {
                label: label.join("/"),
                chain: chain.clone(),
                field: node.name.clone(),
            });
        }
        SchemaKind::Group { children, .. } => {
            chain.push(node.name.clone());
            for child in children {
                collect_target(child, chain, out);
            }
            chain.pop();
        }
    }
}

/// Layered dataflow layout: each shown node's column is one past its
/// deepest shown input (hidden `SourceField`s count as depth 0, next to
/// the Source endpoint), and rows within a column follow the first target
/// pin the node ultimately feeds, so wires flow left to right with
/// minimal crossing. Returns `(column, row)` per shown node, 0-based.
pub fn layered_layout(
    graph: &mapping::Graph,
    hidden: &std::collections::BTreeSet<NodeId>,
    binding_order: &[(NodeId, usize)],
) -> std::collections::BTreeMap<NodeId, (usize, usize)> {
    use std::collections::BTreeMap;

    fn inputs(node: &mapping::Node) -> Vec<NodeId> {
        match node {
            mapping::Node::SourceField { .. }
            | mapping::Node::Position { .. }
            | mapping::Node::Const { .. }
            | mapping::Node::RuntimeValue { .. } => vec![],
            mapping::Node::Call { args, .. } => args.clone(),
            mapping::Node::If {
                condition,
                then,
                else_,
            } => vec![*condition, *then, *else_],
            mapping::Node::ValueMap { input, .. }
            | mapping::Node::Lookup { matches: input, .. } => {
                vec![*input]
            }
            mapping::Node::Aggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).copied().collect(),
        }
    }

    // Column = longest path from a depth-0 feed.
    fn depth_of(
        id: NodeId,
        graph: &mapping::Graph,
        hidden: &std::collections::BTreeSet<NodeId>,
        memo: &mut BTreeMap<NodeId, usize>,
        visiting: &mut std::collections::BTreeSet<NodeId>,
    ) -> usize {
        if let Some(&d) = memo.get(&id) {
            return d;
        }
        if hidden.contains(&id) || !visiting.insert(id) {
            return 0; // hidden feeds sit with the Source endpoint; cycles cap out
        }
        let d = graph
            .nodes
            .get(&id)
            .map(|node| {
                inputs(node)
                    .iter()
                    .filter(|arg| graph.nodes.contains_key(arg))
                    .map(|&arg| depth_of(arg, graph, hidden, memo, visiting) + 1)
                    .max()
                    .unwrap_or(1)
            })
            .unwrap_or(1)
            .max(1);
        visiting.remove(&id);
        memo.insert(id, d);
        d
    }

    // The first (lowest) target pin each node ultimately feeds, propagated
    // upstream from the bindings.
    let mut min_leaf: BTreeMap<NodeId, usize> = BTreeMap::new();
    for &(node, leaf) in binding_order {
        min_leaf
            .entry(node)
            .and_modify(|l| *l = (*l).min(leaf))
            .or_insert(leaf);
    }
    for _ in 0..graph.nodes.len() {
        let mut changed = false;
        for (&id, node) in &graph.nodes {
            let Some(&leaf) = min_leaf.get(&id) else {
                continue;
            };
            for arg in inputs(node) {
                let entry = min_leaf.entry(arg).or_insert(leaf);
                if *entry > leaf {
                    *entry = leaf;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut memo = BTreeMap::new();
    let mut visiting = std::collections::BTreeSet::new();
    let shown: Vec<NodeId> = graph
        .nodes
        .keys()
        .copied()
        .filter(|id| !hidden.contains(id))
        .collect();
    let mut by_column: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
    for &id in &shown {
        let col = depth_of(id, graph, hidden, &mut memo, &mut visiting) - 1;
        by_column.entry(col).or_default().push(id);
    }
    let mut out = BTreeMap::new();
    for (col, mut ids) in by_column {
        ids.sort_by_key(|id| (min_leaf.get(id).copied().unwrap_or(usize::MAX), *id));
        for (row, id) in ids.into_iter().enumerate() {
            out.insert(id, (col, row));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    #[test]
    fn layered_layout_orders_by_depth_and_first_fed_pin() {
        use mapping::{Graph, Node};
        // hidden 0 -> call 1 -> call 3 (diamond with const 2 also into 3);
        // call 4 binds to leaf 0, call 3 binds to leaf 1.
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["a".into()],
                frame: None,
            },
        );
        graph.nodes.insert(
            1,
            Node::Call {
                function: "upper".into(),
                args: vec![0],
            },
        );
        graph.nodes.insert(
            2,
            Node::Const {
                value: ir::Value::Int(1),
            },
        );
        graph.nodes.insert(
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![1, 2],
            },
        );
        graph.nodes.insert(
            4,
            Node::Call {
                function: "lower".into(),
                args: vec![0],
            },
        );
        let hidden = std::collections::BTreeSet::from([0]);
        let layout = layered_layout(&graph, &hidden, &[(4, 0), (3, 1)]);

        assert_eq!(layout[&1], (0, 1)); // depth 1; feeds leaf 1 -> below node 4
        assert_eq!(layout[&2], (0, 2)); // no direct source input, depth 1
        assert_eq!(layout[&4], (0, 0)); // feeds leaf 0 -> first row
        assert_eq!(layout[&3], (1, 0)); // one past its deepest input
        assert!(!layout.contains_key(&0), "hidden nodes are not placed");
    }

    #[test]
    fn source_leaf_paths_are_relative_to_the_innermost_repeating_ancestor() {
        // Orders { Date, Order(rep) { Cust_Name, Items { Item(rep) { Price } } } }
        let schema = SchemaNode::group(
            "Orders",
            vec![
                SchemaNode::scalar("Date", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Cust_Name", ScalarType::String),
                        SchemaNode::group(
                            "Items",
                            vec![
                                SchemaNode::group(
                                    "Item",
                                    vec![SchemaNode::scalar("Price", ScalarType::Float)],
                                )
                                .repeating(),
                            ],
                        ),
                    ],
                )
                .repeating(),
            ],
        );
        let leaves = source_leaves(&schema);
        assert_eq!(
            leaves,
            vec![
                SourceLeaf {
                    label: "Date".into(),
                    frame: None,
                    path: vec!["Date".into()],
                },
                SourceLeaf {
                    label: "Order/Cust_Name".into(),
                    frame: Some(vec!["Order".into()]),
                    path: vec!["Cust_Name".into()],
                },
                SourceLeaf {
                    label: "Order/Items/Item/Price".into(),
                    frame: Some(vec!["Order".into(), "Items".into(), "Item".into()]),
                    path: vec!["Price".into()],
                },
            ]
        );
    }

    #[test]
    fn sibling_repeating_leaves_keep_distinct_frames() {
        let schema = SchemaNode::group(
            "root",
            vec![
                SchemaNode::group("A", vec![SchemaNode::scalar("Id", ScalarType::String)])
                    .repeating(),
                SchemaNode::group("B", vec![SchemaNode::scalar("Id", ScalarType::String)])
                    .repeating(),
            ],
        );

        let leaves = source_leaves(&schema);
        assert_eq!(leaves[0].path, leaves[1].path);
        assert_eq!(leaves[0].frame, Some(vec!["A".into()]));
        assert_eq!(leaves[1].frame, Some(vec!["B".into()]));
    }

    #[test]
    fn repeating_root_uses_an_empty_absolute_frame() {
        let schema = SchemaNode::group("row", vec![SchemaNode::scalar("Id", ScalarType::String)])
            .repeating();

        assert_eq!(source_leaves(&schema)[0].frame, Some(Vec::new()));
    }

    #[test]
    fn target_leaves_carry_their_scope_chain() {
        // row { a, Order { b } }
        let schema = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("a", ScalarType::String),
                SchemaNode::group("Order", vec![SchemaNode::scalar("b", ScalarType::Int)])
                    .repeating(),
            ],
        );
        let leaves = target_leaves(&schema);
        assert_eq!(leaves[0].chain, Vec::<String>::new());
        assert_eq!(leaves[0].field, "a");
        assert_eq!(leaves[1].chain, vec!["Order"]);
        assert_eq!(leaves[1].field, "b");
        assert_eq!(leaves[1].label, "Order/b");
    }
}
