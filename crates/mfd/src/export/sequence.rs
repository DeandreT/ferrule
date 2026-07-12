use mapping::{NodeId, Scope, SequenceExpr};

pub(super) struct SequenceExistsPins {
    pub(super) predicate: NodeId,
    pub(super) sequence_output: u32,
    pub(super) filter_predicate: u32,
}

pub(super) fn collect_scope_sequences<'a>(scope: &'a Scope, sequences: &mut Vec<&'a SequenceExpr>) {
    if let Some(sequence) = scope.sequence() {
        sequences.push(sequence);
    }
    for child in &scope.children {
        collect_scope_sequences(child, sequences);
    }
}
