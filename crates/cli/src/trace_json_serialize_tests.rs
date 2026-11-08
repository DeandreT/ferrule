//! Byte-exact JSON Lines for every trace event variant and payload shape.

use super::*;
use engine::{
    TraceFilterPhase, TraceGrouping, TraceIteration, TraceOutputKind, TracePosition, TraceScope,
    TraceSortKey, TraceSourceField, TraceSourceRow, TraceSourceTree, TraceTarget,
    TraceTargetFieldBinding, TraceValue, TraceWindow,
};

fn sample_events() -> Vec<TraceEvent> {
    let value = |preview: &str| TraceValue {
        value_type: "string",
        preview: preview.into(),
        truncated: true,
    };
    let joined = TracePosition {
        collection: vec!["Rows".into(), "\"q\"\\é".into()],
        index: 3,
        grouped: true,
        join: Some(mapping::JoinId::new(7)),
        join_position: Some((mapping::JoinId::new(9), 2)),
        document_path: Some("input/3.json".into()),
    };
    let plain = TracePosition {
        collection: Vec::new(),
        index: 0,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    };
    let positions = vec![joined, plain];
    let named = TraceScope {
        source_scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
        target: TraceTarget::Named("a\n\u{1}".into()),
        target_path: vec!["Rows".into()],
        structural_path: vec![2, 0],
    };
    let primary = TraceScope {
        target: TraceTarget::Primary,
        target_path: Vec::new(),
        structural_path: Vec::new(),
        ..named.clone()
    };
    let leaf = TraceSourceTree {
        name: None,
        name_truncated: false,
        kind: TraceOutputKind::Scalar,
        value: Some(value("v")),
        children: Vec::new(),
        omitted_children: 0,
        depth_limited: true,
    };
    let tree = TraceSourceTree {
        name: Some("Items".into()),
        name_truncated: true,
        kind: TraceOutputKind::MappedSequence,
        value: None,
        children: vec![leaf],
        omitted_children: 4,
        depth_limited: false,
    };
    let row = TraceSourceRow {
        kind: TraceOutputKind::DocumentSet,
        value: Some(value("row")),
        fields: vec![TraceSourceField {
            name: "F".into(),
            name_truncated: true,
            kind: TraceOutputKind::Repeated,
            value: None,
        }],
        omitted_fields: 1,
        structure: Some(tree),
    };
    let flat_row = TraceSourceRow {
        structure: None,
        ..row.clone()
    };
    let owned = |owner| TraceIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner,
            path: vec!["".into(), "a/b".into()],
        },
    };
    let iterations = [
        TraceIteration::Once,
        TraceIteration::Source {
            path: vec!["Rows".into()],
        },
        owned(mapping::SourceOwner::Primary),
        owned(mapping::SourceOwner::Named {
            name: String::new(),
        }),
        owned(mapping::SourceOwner::Named { name: "n".into() }),
        TraceIteration::DynamicDocuments { source: Vec::new() },
        TraceIteration::Generated {
            kind: "tokenize-regex",
        },
        TraceIteration::Join {
            join: mapping::JoinId::new(u64::MAX),
        },
        TraceIteration::Concatenate { segments: 3 },
        TraceIteration::OptionalPresence {
            condition: u32::MAX,
        },
    ];
    let mut events = vec![
        TraceEvent::NodeValue {
            node: 1,
            positions: positions.clone(),
            value: TraceValue {
                value_type: "float",
                preview: "inf".into(),
                truncated: false,
            },
        },
        TraceEvent::NodeInputValue {
            consumer: 8,
            input: 1,
            input_index: usize::MAX,
            positions: Vec::new(),
            value: value(""),
        },
        TraceEvent::FunctionNodeValue {
            function: mapping::FunctionId::new(12),
            node: 3,
            positions: positions.clone(),
            value: value("x"),
        },
        TraceEvent::FunctionNodeInputValue {
            function: mapping::FunctionId::new(12),
            consumer: 4,
            input: 3,
            input_index: 1,
            positions: Vec::new(),
            value: value("y"),
        },
    ];
    events.extend(
        iterations
            .into_iter()
            .map(|iteration| TraceEvent::ScopeStarted {
                scope: named.clone(),
                iteration,
                positions: positions.clone(),
            }),
    );
    for source_row in [None, Some(flat_row), Some(row)] {
        events.push(TraceEvent::IterationCandidate {
            scope: primary.clone(),
            ordinal: 1,
            positions: positions.clone(),
            source_row,
        });
    }
    for phase in [
        TraceFilterPhase::BeforeSort,
        TraceFilterPhase::AfterSort,
        TraceFilterPhase::Selection,
        TraceFilterPhase::GroupStarting,
        TraceFilterPhase::GroupEnding,
        TraceFilterPhase::PostGroupMember,
    ] {
        events.push(TraceEvent::FilterDecision {
            scope: named.clone(),
            node: 2,
            phase,
            positions: Vec::new(),
            passed: false,
        });
    }
    events.push(TraceEvent::SortCandidate {
        scope: named.clone(),
        positions: positions.clone(),
        keys: vec![
            TraceSortKey {
                node: 3,
                descending: true,
                value: value("k"),
            },
            TraceSortKey {
                node: 4,
                descending: false,
                value: value("l"),
            },
        ],
    });
    events.push(TraceEvent::SortPosition {
        scope: named.clone(),
        positions: Vec::new(),
        output_index: 1,
    });
    for (grouping, key) in [
        (TraceGrouping::By { node: 1 }, None),
        (TraceGrouping::AdjacentBy { node: 2 }, Some(value("g"))),
        (TraceGrouping::StartingWith { node: 3 }, None),
        (TraceGrouping::EndingWith { node: 4 }, None),
        (TraceGrouping::IntoBlocks { node: 5, size: 2 }, None),
    ] {
        events.push(TraceEvent::GroupProduced {
            scope: named.clone(),
            grouping,
            group_index: 1,
            member_count: 2,
            key,
            retained: true,
            positions: positions.clone(),
        });
    }
    for window in [
        TraceWindow::SkipFirst(1),
        TraceWindow::First(2),
        TraceWindow::From(3),
        TraceWindow::FromTo { first: 4, last: 5 },
        TraceWindow::Last(6),
    ] {
        events.push(TraceEvent::WindowApplied {
            scope: named.clone(),
            window_index: 0,
            window,
            before: 9,
            after: 1,
        });
    }
    for (binding, kind, field_value) in [
        (
            TraceTargetFieldBinding::StaticBinding { value: 5 },
            TraceOutputKind::Scalar,
            Some(value("s")),
        ),
        (
            TraceTargetFieldBinding::DynamicBinding { key: 5, value: 6 },
            TraceOutputKind::Group,
            None,
        ),
        (
            TraceTargetFieldBinding::StaticChild,
            TraceOutputKind::Repeated,
            None,
        ),
        (
            TraceTargetFieldBinding::DynamicChild { key: 7 },
            TraceOutputKind::MappedSequence,
            None,
        ),
    ] {
        events.push(TraceEvent::TargetFieldWritten {
            scope: named.clone(),
            field: "status".into(),
            binding,
            positions: Vec::new(),
            kind,
            value: field_value,
        });
    }
    for output_path in [None, Some("audit.json".to_string())] {
        events.push(TraceEvent::TargetProduced {
            scope: primary.clone(),
            positions: positions.clone(),
            output_path,
            kind: TraceOutputKind::DocumentSet,
        });
    }
    events.push(TraceEvent::ScopeFinished {
        scope: primary,
        candidates: 4,
        produced: 1,
        kind: TraceOutputKind::Group,
    });
    events
}

#[test]
fn every_event_shape_serializes_to_the_recorded_lines() {
    let mut lines = String::new();
    for (sequence, event) in sample_events().iter().enumerate() {
        let line = serde_json::to_string(&trace_line(sequence as u64, event))
            .expect("trace events serialize");
        lines.push_str(&line);
        lines.push('\n');
    }
    assert_eq!(
        lines,
        include_str!("../tests/fixtures/trace/serialized_events.jsonl")
    );
}
