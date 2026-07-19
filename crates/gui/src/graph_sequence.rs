use mapping::{NodeId, SequenceExpr};

pub(super) fn input_at(sequence: &SequenceExpr, index: usize) -> Option<NodeId> {
    sequence.inputs().get(index).copied()
}

pub(super) fn set_input(sequence: &mut SequenceExpr, index: usize, node: NodeId) {
    match sequence {
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
    }
}

pub(super) fn label(sequence: &SequenceExpr) -> &'static str {
    match sequence {
        SequenceExpr::Tokenize { .. } => "tokenize",
        SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        SequenceExpr::TokenizeRegex { .. } => "tokenize-regexp",
        SequenceExpr::Generate { .. } => "generate-sequence",
        SequenceExpr::RecursiveCollect { .. } => "recursive-collect",
    }
}

pub(super) fn pin_label(sequence: &SequenceExpr, index: usize) -> &'static str {
    if index == sequence.inputs().len() {
        return "predicate";
    }
    match sequence {
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
        SequenceExpr::RecursiveCollect { .. } => ["prefix", "separator"]
            .get(index)
            .copied()
            .unwrap_or("input"),
    }
}
