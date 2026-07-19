//! The canvas node model: the mapping graph's nodes plus compact source and
//! target endpoint blocks. Endpoint pins keep their complete schema identity,
//! while blocks group nearby fields into nodes that remain readable.

use ir::{SchemaKind, SchemaNode};
use mapping::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanvasNode {
    SourceBlock(usize),
    TargetBlock(usize),
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

/// Maximum number of schema leaves rendered by one endpoint node.
pub const ENDPOINT_BLOCK_PIN_LIMIT: usize = 12;

/// A compact source endpoint. `frame` is the exact repeating context shared
/// by all leaves, while `pin_labels` are relative display labels only.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceBlock {
    pub title: String,
    pub frame: Option<Vec<String>>,
    pub leaves: Vec<SourceLeaf>,
    pub pin_labels: Vec<String>,
}

/// A compact target endpoint. `chain` is the exact owning scope chain shared
/// by all leaves, while `pin_labels` are relative display labels only.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetBlock {
    pub title: String,
    pub chain: Vec<String>,
    pub leaves: Vec<TargetLeaf>,
    pub pin_labels: Vec<String>,
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

/// Groups source leaves by their exact repeating frame, then chunks large
/// frames so a single endpoint cannot dominate the canvas. Group and leaf
/// order follows schema declaration order and is therefore deterministic.
pub fn source_blocks(schema: &SchemaNode) -> Vec<SourceBlock> {
    let mut groups: Vec<(Option<Vec<String>>, Vec<SourceLeaf>)> = Vec::new();
    for leaf in source_leaves(schema) {
        if let Some((_, leaves)) = groups.iter_mut().find(|(frame, _)| frame == &leaf.frame) {
            leaves.push(leaf);
        } else {
            groups.push((leaf.frame.clone(), vec![leaf]));
        }
    }

    if groups.is_empty() {
        groups.push((schema.repeating.then(Vec::new), Vec::new()));
    }

    let mut blocks = Vec::new();
    for (frame, leaves) in groups {
        let chunk_count = leaves.len().max(1).div_ceil(ENDPOINT_BLOCK_PIN_LIMIT);
        if leaves.is_empty() {
            blocks.push(source_block(schema, frame, Vec::new(), 0, chunk_count));
            continue;
        }
        for (chunk_index, chunk) in leaves.chunks(ENDPOINT_BLOCK_PIN_LIMIT).enumerate() {
            blocks.push(source_block(
                schema,
                frame.clone(),
                chunk.to_vec(),
                chunk_index,
                chunk_count,
            ));
        }
    }
    blocks
}

fn source_block(
    schema: &SchemaNode,
    frame: Option<Vec<String>>,
    leaves: Vec<SourceLeaf>,
    chunk_index: usize,
    chunk_count: usize,
) -> SourceBlock {
    let context = frame
        .as_ref()
        .filter(|frame| !frame.is_empty())
        .and_then(|frame| frame.last().cloned())
        .unwrap_or_else(|| schema.name.clone());
    let title = chunked_title(format!("Source: {context}"), chunk_index, chunk_count);
    let pin_labels = leaves
        .iter()
        .map(|leaf| {
            if leaf.path.is_empty() {
                "<item>".to_string()
            } else {
                leaf.path.join("/")
            }
        })
        .collect();
    SourceBlock {
        title,
        frame,
        leaves,
        pin_labels,
    }
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

/// Groups target leaves by their exact owning scope chain, then chunks large
/// groups so target endpoints remain readable without losing binding identity.
pub fn target_blocks(schema: &SchemaNode) -> Vec<TargetBlock> {
    let mut groups: Vec<(Vec<String>, Vec<TargetLeaf>)> = Vec::new();
    for leaf in target_leaves(schema) {
        if let Some((_, leaves)) = groups.iter_mut().find(|(chain, _)| chain == &leaf.chain) {
            leaves.push(leaf);
        } else {
            groups.push((leaf.chain.clone(), vec![leaf]));
        }
    }

    if groups.is_empty() {
        groups.push((Vec::new(), Vec::new()));
    }

    let mut blocks = Vec::new();
    for (chain, leaves) in groups {
        let chunk_count = leaves.len().max(1).div_ceil(ENDPOINT_BLOCK_PIN_LIMIT);
        if leaves.is_empty() {
            blocks.push(target_block(schema, chain, Vec::new(), 0, chunk_count));
            continue;
        }
        for (chunk_index, chunk) in leaves.chunks(ENDPOINT_BLOCK_PIN_LIMIT).enumerate() {
            blocks.push(target_block(
                schema,
                chain.clone(),
                chunk.to_vec(),
                chunk_index,
                chunk_count,
            ));
        }
    }
    blocks
}

fn target_block(
    schema: &SchemaNode,
    chain: Vec<String>,
    leaves: Vec<TargetLeaf>,
    chunk_index: usize,
    chunk_count: usize,
) -> TargetBlock {
    let context = chain.last().cloned().unwrap_or_else(|| schema.name.clone());
    let title = chunked_title(format!("Target: {context}"), chunk_index, chunk_count);
    let pin_labels = leaves.iter().map(|leaf| leaf.field.clone()).collect();
    TargetBlock {
        title,
        chain,
        leaves,
        pin_labels,
    }
}

fn chunked_title(base: String, chunk_index: usize, chunk_count: usize) -> String {
    if chunk_count <= 1 {
        base
    } else {
        format!("{base} ({}/{chunk_count})", chunk_index + 1)
    }
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
    fn source_blocks_group_repeating_contexts_and_keep_full_leaf_identity() {
        let schema = SchemaNode::group(
            "Company",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Office",
                    vec![
                        SchemaNode::scalar("City", ScalarType::String),
                        SchemaNode::group(
                            "Person",
                            vec![SchemaNode::scalar("First", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        );

        let blocks = source_blocks(&schema);

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].title, "Source: Company");
        assert_eq!(blocks[0].frame, None);
        assert_eq!(blocks[0].pin_labels, ["Name"]);
        assert_eq!(blocks[1].title, "Source: Office");
        assert_eq!(blocks[1].frame, Some(vec!["Office".into()]));
        assert_eq!(blocks[1].pin_labels, ["City"]);
        assert_eq!(blocks[1].leaves[0].label, "Office/City");
        assert_eq!(blocks[1].leaves[0].path, ["City"]);
        assert_eq!(blocks[2].title, "Source: Person");
        assert_eq!(blocks[2].pin_labels, ["First"]);
        assert_eq!(
            blocks[2].leaves[0].frame,
            Some(vec!["Office".into(), "Person".into()])
        );
    }

    #[test]
    fn source_blocks_chunk_large_frames_deterministically() {
        let fields = (0..25)
            .map(|index| SchemaNode::scalar(format!("Field{index:02}"), ScalarType::String))
            .collect();
        let schema = SchemaNode::group("row", fields);

        let blocks = source_blocks(&schema);

        assert_eq!(
            blocks
                .iter()
                .map(|block| block.leaves.len())
                .collect::<Vec<_>>(),
            [12, 12, 1]
        );
        assert_eq!(blocks[0].title, "Source: row (1/3)");
        assert_eq!(blocks[2].title, "Source: row (3/3)");
        assert_eq!(
            blocks[0].pin_labels.first().map(String::as_str),
            Some("Field00")
        );
        assert_eq!(blocks[2].pin_labels, ["Field24"]);
        assert_eq!(blocks, source_blocks(&schema));
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

    #[test]
    fn target_blocks_group_scope_owners_and_use_local_pin_labels() {
        let schema = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Status", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Number", ScalarType::Int),
                        SchemaNode::scalar("Total", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        );

        let blocks = target_blocks(&schema);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].title, "Target: Output");
        assert!(blocks[0].chain.is_empty());
        assert_eq!(blocks[0].pin_labels, ["Status"]);
        assert_eq!(blocks[1].title, "Target: Order");
        assert_eq!(blocks[1].chain, ["Order"]);
        assert_eq!(blocks[1].pin_labels, ["Number", "Total"]);
        assert_eq!(blocks[1].leaves[0].label, "Order/Number");
        assert_eq!(blocks[1].leaves[0].field, "Number");
    }

    #[test]
    fn empty_schemas_keep_one_empty_endpoint_block() {
        let schema = SchemaNode::group("empty", Vec::new());

        assert_eq!(source_blocks(&schema).len(), 1);
        assert!(source_blocks(&schema)[0].leaves.is_empty());
        assert_eq!(target_blocks(&schema).len(), 1);
        assert!(target_blocks(&schema)[0].leaves.is_empty());
    }
}
