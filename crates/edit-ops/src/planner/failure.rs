use super::*;
use crate::{FailureIteration, FailureRule, FailureSelection, GeneratedSequence, SourceCollection};

impl Planner {
    pub(super) fn failure_rule(
        &mut self,
        rule: &FailureRule,
        previous: Option<&mapping::FailureRule>,
    ) -> Result<mapping::FailureRule, EditError> {
        let iteration = match &rule.iteration {
            FailureIteration::Source { collection } => {
                if previous.is_some_and(|rule| {
                    matches!(rule.iteration, mapping::FailureIteration::Sequence { .. })
                }) {
                    return invalid(
                        "change a generated rule by removing it and adding its replacement",
                    );
                }
                match collection {
                    SourceCollection::Legacy { path } => mapping::FailureIteration::Source {
                        collection: path.clone(),
                    },
                    source => mapping::FailureIteration::OwnedSource {
                        source: self.owned_collection(source)?,
                    },
                }
            }
            FailureIteration::Generated { sequence } => {
                let previous = previous.and_then(|rule| match &rule.iteration {
                    mapping::FailureIteration::Sequence { sequence } => Some(sequence),
                    _ => None,
                });
                mapping::FailureIteration::Sequence {
                    sequence: self.sequence(sequence, previous)?,
                }
            }
        };
        let selection = match &rule.selection {
            FailureSelection::All {} => mapping::FailureSelection::All,
            FailureSelection::WhenTrue { expression } => mapping::FailureSelection::WhenTrue {
                predicate: self.expression(expression)?,
            },
            FailureSelection::WhenFalse { expression } => mapping::FailureSelection::WhenFalse {
                predicate: self.expression(expression)?,
            },
        };
        let message = rule
            .message
            .as_ref()
            .map(|value| self.expression(value))
            .transpose()?;
        Ok(mapping::FailureRule {
            iteration,
            selection,
            message,
        })
    }

    fn sequence(
        &mut self,
        sequence: &GeneratedSequence,
        previous: Option<&mapping::SequenceExpr>,
    ) -> Result<mapping::SequenceExpr, EditError> {
        let item = match sequence {
            GeneratedSequence::Generate { item, .. }
            | GeneratedSequence::Tokenize { item, .. }
            | GeneratedSequence::TokenizeByLength { item, .. }
            | GeneratedSequence::TokenizeRegex { item, .. } => *item,
        };
        if let Some(previous) = previous {
            if previous.item() != item.0
                || !matches!(
                    previous,
                    mapping::SequenceExpr::Generate { .. }
                        | mapping::SequenceExpr::Tokenize { .. }
                        | mapping::SequenceExpr::TokenizeByLength { .. }
                        | mapping::SequenceExpr::TokenizeRegex { .. }
                )
            {
                return invalid("generated rule item identity or sequence is read-only");
            }
            if !matches!(self.node(item.0)?, Node::SourceField { path, frame: None } if path.is_empty())
            {
                return invalid("generated rule item is not its private scalar source");
            }
        } else {
            self.fresh(item)?;
            let mut after = self.project.clone();
            after.graph.nodes.insert(
                item.0,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            self.graph_change(
                after,
                GraphAction::InsertNode {
                    node: item.0,
                    position: CanvasPosition { x: 0.0, y: 0.0 },
                },
            )?;
        }
        let result = match sequence {
            GeneratedSequence::Generate { from, to, .. } => mapping::SequenceExpr::Generate {
                from: from
                    .as_ref()
                    .map(|value| self.expression(value))
                    .transpose()?,
                to: self.expression(to)?,
                item: item.0,
            },
            GeneratedSequence::Tokenize {
                input, delimiter, ..
            } => mapping::SequenceExpr::Tokenize {
                input: self.expression(input)?,
                delimiter: self.expression(delimiter)?,
                item: item.0,
            },
            GeneratedSequence::TokenizeByLength { input, length, .. } => {
                mapping::SequenceExpr::TokenizeByLength {
                    input: self.expression(input)?,
                    length: self.expression(length)?,
                    item: item.0,
                }
            }
            GeneratedSequence::TokenizeRegex {
                input,
                pattern,
                flags,
                ..
            } => mapping::SequenceExpr::TokenizeRegex {
                input: self.expression(input)?,
                pattern: self.expression(pattern)?,
                flags: flags
                    .as_ref()
                    .map(|value| self.expression(value))
                    .transpose()?,
                item: item.0,
            },
        };
        for input in result.inputs() {
            if depends_on(&self.project, input, item.0) {
                return invalid("sequence bounds cannot depend on its private item");
            }
        }
        Ok(result)
    }
}
