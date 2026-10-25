use ir::ScalarType;

use super::*;
use crate::{FilterMapCapture, FilterMapV1, FunctionId, NodeId, OwnedRecursiveCollect};

fn compose(source: SequenceExpr, item: NodeId, captures: &[NodeId]) -> SequenceExpr {
    SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(source),
        item,
        predicate: FunctionId::new(1),
        mapper: FunctionId::new(2),
        output_type: ScalarType::String,
        captures: captures
            .iter()
            .map(|&node| FilterMapCapture {
                node,
                ty: ScalarType::Int,
            })
            .collect(),
    })
}

fn producers() -> Vec<SequenceExpr> {
    vec![
        SequenceExpr::Tokenize {
            input: 1,
            delimiter: 2,
            item: 3,
        },
        SequenceExpr::TokenizeByLength {
            input: 1,
            length: 2,
            item: 3,
        },
        SequenceExpr::TokenizeRegex {
            input: 1,
            pattern: 2,
            flags: Some(4),
            item: 3,
        },
        SequenceExpr::TokenizeRegex {
            input: 1,
            pattern: 2,
            flags: None,
            item: 3,
        },
        SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        },
        SequenceExpr::Generate {
            from: None,
            to: 2,
            item: 3,
        },
        SequenceExpr::RecursiveCollect {
            collection: vec!["Tree".into()],
            children: vec!["Children".into()],
            descent_value: vec!["Name".into()],
            values: vec!["Leaves".into()],
            value: vec!["Value".into()],
            prefix: 1,
            separator: 2,
            item: 3,
        },
        SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
            collection: crate::OwnedSourceRef {
                owner: crate::SourceOwner::Primary,
                path: vec!["Tree".into()],
            },
            children: vec!["Children".into()],
            descent_value: vec!["Name".into()],
            values: vec!["Leaves".into()],
            value: vec!["Value".into()],
            prefix: 1,
            separator: 2,
            item: 3,
        }),
    ]
}

/// Every producer alone, composed once, and composed twice, with and without
/// captures; ids repeat across levels so shared nodes are covered too.
fn sequences() -> Vec<SequenceExpr> {
    let mut all = Vec::new();
    for producer in producers() {
        let once = compose(producer.clone(), 5, &[6, 7]);
        all.push(compose(once.clone(), 8, &[9]));
        all.push(compose(once.clone(), 3, &[]));
        all.push(compose(producer.clone(), 5, &[]));
        all.push(once);
        all.push(producer);
    }
    all
}

fn level_inputs(sequence: &SequenceExpr) -> Vec<SequenceLevelInput> {
    let mut found = Vec::new();
    sequence.visit_level_inputs(&mut |input| found.push(input));
    found
}

/// A level evaluates the inputs it and every deeper level declare, and owns
/// its own identities: exactly what `inputs` and `owned_items` flatten.
#[test]
fn each_level_reads_its_flattened_inputs_and_owns_its_items() {
    for sequence in sequences() {
        let declared = level_inputs(&sequence);
        for (level, composed) in sequence.levels().into_iter().enumerate() {
            let inputs: Vec<_> = declared
                .iter()
                .filter(|input| input.level >= level && input.input.role == NodeInputRole::Parent)
                .map(|input| input.input.node)
                .collect();
            assert_eq!(inputs, composed.inputs(), "{sequence:?} level {level}");
            let identities: Vec<_> = declared
                .iter()
                .filter(|input| {
                    input.level == level && input.input.role == NodeInputRole::ItemIdentity
                })
                .map(|input| input.input.node)
                .collect();
            assert_eq!(
                identities,
                composed.owned_items(),
                "{sequence:?} level {level}"
            );
        }
        // Inputs keep their flattened index.
        let indexes: Vec<_> = declared
            .iter()
            .filter_map(|input| match input.input.slot {
                NodeInputSlot::SequenceInput(index) => Some(index),
                _ => None,
            })
            .collect();
        assert_eq!(indexes, (0..sequence.inputs().len()).collect::<Vec<_>>());
    }
}

#[test]
fn a_twice_composed_sequence_declares_inputs_at_their_own_level() {
    let producer = SequenceExpr::Tokenize {
        input: 1,
        delimiter: 2,
        item: 3,
    };
    let sequence = compose(compose(producer, 5, &[6]), 8, &[9]);
    let declared: Vec<_> = level_inputs(&sequence)
        .into_iter()
        .map(|input| (input.level, input.input.node, input.input.slot))
        .collect();
    assert_eq!(
        declared,
        [
            (2, 1, NodeInputSlot::SequenceInput(0)),
            (2, 2, NodeInputSlot::SequenceInput(1)),
            (1, 6, NodeInputSlot::SequenceInput(2)),
            (0, 9, NodeInputSlot::SequenceInput(3)),
            (0, 5, NodeInputSlot::SequenceReservedItem),
            (0, 8, NodeInputSlot::SequenceItem),
            (1, 3, NodeInputSlot::SequenceReservedItem),
            (1, 5, NodeInputSlot::SequenceItem),
            (2, 3, NodeInputSlot::SequenceItem),
        ]
    );
    assert_eq!(sequence.levels().len(), 3);
}
