use mapping::{Node, NodeId, SequenceExpr};

pub(super) const READ_ONLY_INPUTS_MESSAGE: &str = "Filter/map, owned recursive-collect, owned dynamic-field and owned XML mixed-content inputs cannot be edited yet.";

/// Filter/map compositions, owned producers and owned dynamic reads are
/// preserved read-only.
pub(super) fn inputs_are_read_only(node: &Node) -> bool {
    matches!(
        node,
        Node::OwnedDynamicSourceField(_) | Node::OwnedXmlMixedContent(_)
    ) || matches!(
        node,
        Node::SequenceExists { sequence, .. }
            | Node::SequenceItemAt { sequence, .. }
            | Node::SequenceAggregate { sequence, .. }
            if matches!(
                sequence,
                SequenceExpr::FilterMapV1(_) | SequenceExpr::OwnedRecursiveCollect(_)
            )
    )
}

pub(super) fn set_input(sequence: &mut SequenceExpr, index: usize, node: NodeId) {
    match sequence {
        SequenceExpr::FilterMapV1(value) => {
            let source_count = value.source.inputs().len();
            if index < source_count {
                set_input(&mut value.source, index, node);
            } else if let Some(capture) = value.captures.get_mut(index - source_count) {
                capture.node = node;
            }
        }
        SequenceExpr::Tokenize {
            input, delimiter, ..
        } => match index {
            0 => *input = node,
            1 => *delimiter = node,
            _ => {}
        },
        SequenceExpr::TokenizeByLength { input, length, .. } => match index {
            0 => *input = node,
            1 => *length = node,
            _ => {}
        },
        SequenceExpr::TokenizeRegex {
            input,
            pattern,
            flags,
            ..
        } => match index {
            0 => *input = node,
            1 => *pattern = node,
            2 => *flags = Some(node),
            _ => {}
        },
        SequenceExpr::Generate {
            from: Some(from),
            to,
            ..
        } => match index {
            0 => *from = node,
            1 => *to = node,
            _ => {}
        },
        SequenceExpr::Generate { from: None, to, .. } => {
            if index == 0 {
                *to = node;
            }
        }
        SequenceExpr::RecursiveCollect {
            prefix, separator, ..
        } => match index {
            0 => *prefix = node,
            1 => *separator = node,
            _ => {}
        },
        // Saved owned producers stay read-only; authoring is not available.
        SequenceExpr::OwnedRecursiveCollect(_) => {}
    }
}

pub(super) fn label(sequence: &SequenceExpr) -> &'static str {
    match sequence {
        SequenceExpr::FilterMapV1(_) => "filter/map",
        SequenceExpr::Tokenize { .. } => "tokenize",
        SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        SequenceExpr::TokenizeRegex { .. } => "tokenize-regexp",
        SequenceExpr::Generate { .. } => "generate-sequence",
        SequenceExpr::RecursiveCollect { .. } => "recursive-collect",
        SequenceExpr::OwnedRecursiveCollect(_) => "owned-recursive-collect",
    }
}

pub(super) fn pin_label(sequence: &SequenceExpr, index: usize) -> &'static str {
    if index == sequence.inputs().len() {
        return "predicate";
    }
    match sequence {
        SequenceExpr::FilterMapV1(value) => {
            let source_count = value.source.inputs().len();
            if index < source_count {
                pin_label(&value.source, index)
            } else {
                "capture"
            }
        }
        SequenceExpr::Tokenize { .. } => ["input", "delimiter"]
            .get(index)
            .copied()
            .unwrap_or("input"),
        SequenceExpr::TokenizeByLength { .. } => {
            ["input", "length"].get(index).copied().unwrap_or("input")
        }
        SequenceExpr::TokenizeRegex { flags, .. } => {
            if flags.is_some() {
                ["input", "pattern", "flags"]
                    .get(index)
                    .copied()
                    .unwrap_or("input")
            } else {
                ["input", "pattern"].get(index).copied().unwrap_or("input")
            }
        }
        SequenceExpr::Generate { from: Some(_), .. } => {
            ["from", "to"].get(index).copied().unwrap_or("input")
        }
        SequenceExpr::Generate { from: None, .. } => "to",
        SequenceExpr::RecursiveCollect { .. } | SequenceExpr::OwnedRecursiveCollect(_) => {
            ["prefix", "separator"]
                .get(index)
                .copied()
                .unwrap_or("input")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_owned_walk_inputs_are_preserved_read_only() {
        let walk = SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
            collection: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Dir".into()],
            },
            children: vec!["Dir".into()],
            descent_value: vec!["Note".into()],
            values: vec!["file".into()],
            value: vec!["Note".into()],
            prefix: 1,
            separator: 2,
            item: 3,
        });
        let node = Node::SequenceItemAt {
            sequence: walk.clone(),
            index: 4,
        };
        assert!(inputs_are_read_only(&node));
        assert_eq!(label(&walk), "owned-recursive-collect");
        assert_eq!(pin_label(&walk, 1), "separator");
        let mut edited = walk.clone();
        set_input(&mut edited, 0, 9);
        assert_eq!(edited, walk);
        assert!(!inputs_are_read_only(&Node::SequenceItemAt {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 1,
                item: 3,
            },
            index: 4,
        }));
    }

    #[test]
    fn saved_owned_dynamic_reads_are_preserved_read_only() {
        let read = Node::OwnedDynamicSourceField(mapping::OwnedDynamicSourceField {
            object: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Props".into()],
            },
            frame: None,
            key: 4,
        });
        assert!(inputs_are_read_only(&read));
        assert!(!inputs_are_read_only(&Node::DynamicSourceField {
            object: vec!["Props".into()],
            frame: None,
            key: 4,
        }));
    }
}
