use std::collections::{BTreeMap, BTreeSet};

use mapping::{Graph, NodeId, SequenceExpr};

use super::{ScalarExpr, instantiate};

#[derive(Clone)]
pub(in crate::import) enum ScalarSequenceExpr {
    Tokenize {
        input: Box<ScalarExpr>,
        delimiter: Box<ScalarExpr>,
    },
    TokenizeByLength {
        input: Box<ScalarExpr>,
        length: Box<ScalarExpr>,
    },
    TokenizeRegex {
        input: Box<ScalarExpr>,
        pattern: Box<ScalarExpr>,
        flags: Option<Box<ScalarExpr>>,
    },
    Generate {
        from: Option<Box<ScalarExpr>>,
        to: Box<ScalarExpr>,
    },
}

impl ScalarSequenceExpr {
    pub(super) fn collect_parameters(&self, parameters: &mut BTreeSet<u32>) {
        match self {
            Self::Tokenize { input, delimiter } => {
                input.collect_parameters(parameters);
                delimiter.collect_parameters(parameters);
            }
            Self::TokenizeByLength { input, length } => {
                input.collect_parameters(parameters);
                length.collect_parameters(parameters);
            }
            Self::TokenizeRegex {
                input,
                pattern,
                flags,
            } => {
                input.collect_parameters(parameters);
                pattern.collect_parameters(parameters);
                if let Some(flags) = flags {
                    flags.collect_parameters(parameters);
                }
            }
            Self::Generate { from, to } => {
                if let Some(from) = from {
                    from.collect_parameters(parameters);
                }
                to.collect_parameters(parameters);
            }
        }
    }

    pub(super) fn instantiate(
        &self,
        item: NodeId,
        parameters: &BTreeMap<u32, NodeId>,
        graph: &mut Graph,
        next_id: &mut NodeId,
    ) -> SequenceExpr {
        match self {
            Self::Tokenize { input, delimiter } => SequenceExpr::Tokenize {
                input: instantiate(input, parameters, graph, next_id),
                delimiter: instantiate(delimiter, parameters, graph, next_id),
                item,
            },
            Self::TokenizeByLength { input, length } => SequenceExpr::TokenizeByLength {
                input: instantiate(input, parameters, graph, next_id),
                length: instantiate(length, parameters, graph, next_id),
                item,
            },
            Self::TokenizeRegex {
                input,
                pattern,
                flags,
            } => SequenceExpr::TokenizeRegex {
                input: instantiate(input, parameters, graph, next_id),
                pattern: instantiate(pattern, parameters, graph, next_id),
                flags: flags
                    .as_ref()
                    .map(|flags| instantiate(flags, parameters, graph, next_id)),
                item,
            },
            Self::Generate { from, to } => SequenceExpr::Generate {
                from: from
                    .as_ref()
                    .map(|from| instantiate(from, parameters, graph, next_id)),
                to: instantiate(to, parameters, graph, next_id),
                item,
            },
        }
    }
}
