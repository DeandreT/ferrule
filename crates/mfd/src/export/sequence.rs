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
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            collect_scope_sequences(segment, sequences);
        }
    }
    for child in &scope.children {
        collect_scope_sequences(child, sequences);
    }
}
