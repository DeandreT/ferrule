//! The canvas node model: the mapping graph's nodes plus two special
//! endpoints -- a Source node whose output pins are the source schema's
//! scalar leaves and a Target node whose input pins are the target's --
//! so mappings are wired leaf-to-leaf like a visual mapper, with
//! `SourceField` nodes and `Binding`s maintained behind the wires.

use ir::{SchemaKind, SchemaNode};
use mapping::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

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
