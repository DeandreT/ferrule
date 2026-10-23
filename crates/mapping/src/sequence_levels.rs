//! The composition levels of a generated sequence and the inputs each
//! declares.
//!
//! A filter/map composes another sequence, which may itself compose one. The
//! sequence is level 0 and each composed source one level deeper. A level
//! declares its own producer arguments or captures and its item identity; a
//! filter/map also owns its source's item as a reserved identity. Analyses
//! that give each level its own context read a level's inputs from here
//! rather than from the flattened [`SequenceExpr::inputs`].

use crate::{NodeInput, NodeInputRole, NodeInputSlot, SequenceExpr};

/// One input or item identity of a generated sequence, with the composition
/// level that declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceLevelInput {
    pub level: usize,
    pub input: NodeInput,
}

impl SequenceExpr {
    /// This sequence and each source it composes, outermost first; a
    /// sequence's index is its composition level.
    pub fn levels(&self) -> Vec<&SequenceExpr> {
        let mut levels = vec![self];
        let mut current = self;
        while let Self::FilterMapV1(composition) = current {
            current = &composition.source;
            levels.push(current);
        }
        levels
    }

    /// Visits every level's inputs in [`SequenceExpr::inputs`] order, each a
    /// parent-context [`NodeInputSlot::SequenceInput`] at its flattened index,
    /// then each level's item identities, outermost level first: a
    /// filter/map's reserved source item, then the level's own item.
    pub fn visit_level_inputs(&self, visit: &mut impl FnMut(SequenceLevelInput)) {
        let mut index = 0;
        visit_declared_inputs(self, 0, &mut index, visit);
        for (level, sequence) in self.levels().into_iter().enumerate() {
            let mut identity = |node, slot| {
                visit(SequenceLevelInput {
                    level,
                    input: NodeInput {
                        node,
                        slot,
                        role: NodeInputRole::ItemIdentity,
                    },
                });
            };
            if let Self::FilterMapV1(composition) = sequence {
                identity(
                    composition.source.item(),
                    NodeInputSlot::SequenceReservedItem,
                );
            }
            identity(sequence.item(), NodeInputSlot::SequenceItem);
        }
    }
}

/// A composed source's inputs precede the captures of the filter/map that
/// composes it, as in [`SequenceExpr::inputs`].
fn visit_declared_inputs(
    sequence: &SequenceExpr,
    level: usize,
    index: &mut usize,
    visit: &mut impl FnMut(SequenceLevelInput),
) {
    match sequence {
        SequenceExpr::FilterMapV1(composition) => {
            visit_declared_inputs(&composition.source, level + 1, index, visit);
            for capture in &composition.captures {
                declare(capture.node, level, index, visit);
            }
        }
        _ => {
            for node in sequence.inputs() {
                declare(node, level, index, visit);
            }
        }
    }
}

fn declare(
    node: crate::NodeId,
    level: usize,
    index: &mut usize,
    visit: &mut impl FnMut(SequenceLevelInput),
) {
    visit(SequenceLevelInput {
        level,
        input: NodeInput {
            node,
            slot: NodeInputSlot::SequenceInput(*index),
            role: NodeInputRole::Parent,
        },
    });
    *index += 1;
}

#[cfg(test)]
#[path = "tests/sequence_levels.rs"]
mod tests;
