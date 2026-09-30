//! Opt-in control point for a host-driven live debugger.

use ir::{Instance, Value};
use mapping::{FunctionId, NodeId};

use crate::EngineError;
use crate::source_iteration::PositionFrame;
use crate::trace::{
    TraceOutputKind, TracePosition, TraceScope, TraceTargetFieldBinding, TraceValue,
    trace_positions,
};

const MAX_DEBUG_DRAFT_FIELDS: usize = 8;
const MAX_DEBUG_SOURCE_FRAMES: usize = 4;
const MAX_DEBUG_SOURCE_FIELDS: usize = 8;
const MAX_DEBUG_FIELD_NAME_CHARS: usize = 160;
const MAX_DEBUG_POSITIONS: usize = 16;
const MAX_DEBUG_POSITION_SEGMENTS: usize = 16;

/// A bounded preview of an instance. Collections and groups expose only their
/// immediate length, never a copy of their contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugInstancePreview {
    pub kind: TraceOutputKind,
    pub value: Option<TraceValue>,
    pub length: Option<usize>,
}

impl DebugInstancePreview {
    fn new(instance: &Instance) -> Self {
        let (value, length) = match instance {
            Instance::Scalar(value) => (Some(TraceValue::new(value)), None),
            Instance::Group(fields) => (None, Some(fields.len())),
            Instance::Repeated(items) => (singleton_scalar(items), Some(items.len())),
            Instance::MappedSequence(items) => (None, Some(items.len())),
            Instance::DocumentSet(documents) => (None, Some(documents.len())),
        };
        Self {
            kind: TraceOutputKind::of(instance),
            value,
            length,
        }
    }
}

fn singleton_scalar(items: &[Instance]) -> Option<TraceValue> {
    match items {
        [Instance::Scalar(value)] => Some(TraceValue::new(value)),
        _ => None,
    }
}

/// One bounded named field preview in a target draft or source frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugDraftField {
    pub name: String,
    pub name_truncated: bool,
    pub preview: DebugInstancePreview,
}

/// A bounded, ordered view of fields already built in the current scope.
/// This is not a complete or serialized target document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugScopeDraft {
    pub fields: Vec<DebugDraftField>,
    pub omitted_fields: usize,
}

impl DebugScopeDraft {
    fn new(fields: &[(String, Instance)]) -> Self {
        Self {
            fields: bounded_fields(fields, MAX_DEBUG_DRAFT_FIELDS),
            omitted_fields: fields.len().saturating_sub(MAX_DEBUG_DRAFT_FIELDS),
        }
    }
}

/// One active source frame, captured without descending into nested values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSourceFrame {
    pub preview: DebugInstancePreview,
    pub fields: Vec<DebugDraftField>,
    pub omitted_fields: usize,
}

impl DebugSourceFrame {
    fn new(instance: &Instance) -> Self {
        let mut preview = DebugInstancePreview::new(instance);
        let mut visible_fields = 0usize;
        let mut fields = Vec::new();
        if let Instance::Group(source_fields) = instance {
            for (name, value) in source_fields {
                if name.starts_with('\u{1f}') {
                    continue;
                }
                visible_fields += 1;
                if fields.len() < MAX_DEBUG_SOURCE_FIELDS {
                    fields.push(preview_field(name, value));
                }
            }
            preview.length = Some(visible_fields);
        }
        Self {
            preview,
            fields,
            omitted_fields: visible_fields.saturating_sub(MAX_DEBUG_SOURCE_FIELDS),
        }
    }
}

/// The innermost active source frames at a pending target write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSourceContext {
    pub frames: Vec<DebugSourceFrame>,
    pub omitted_outer_frames: usize,
}

/// An exact immediate source-field lookup requested by a debug host. Unlike
/// the shallow frame snapshot, it can reach a field beyond the first eight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSourceFieldProbe {
    pub frame_from_inner: usize,
    pub field: String,
    pub preview: Option<DebugInstancePreview>,
}

impl DebugSourceContext {
    fn new(context: &[&Instance]) -> Self {
        let omitted_outer_frames = context.len().saturating_sub(MAX_DEBUG_SOURCE_FRAMES);
        Self {
            frames: context[omitted_outer_frames..]
                .iter()
                .map(|frame| DebugSourceFrame::new(frame))
                .collect(),
            omitted_outer_frames,
        }
    }
}

fn bounded_fields(fields: &[(String, Instance)], limit: usize) -> Vec<DebugDraftField> {
    fields
        .iter()
        .take(limit)
        .map(|(name, instance)| preview_field(name, instance))
        .collect()
}

fn preview_field(name: &str, instance: &Instance) -> DebugDraftField {
    let (name, name_truncated) = bounded_name(name);
    DebugDraftField {
        name,
        name_truncated,
        preview: DebugInstancePreview::new(instance),
    }
}

fn bounded_name(name: &str) -> (String, bool) {
    let mut chars = name.chars();
    let preview = chars.by_ref().take(MAX_DEBUG_FIELD_NAME_CHARS).collect();
    (preview, chars.next().is_some())
}

/// One target field that has been computed but has not yet been inserted.
/// A host can retain this owned, bounded snapshot while execution is paused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTargetWrite {
    pub scope: TraceScope,
    pub positions: Vec<TracePosition>,
    pub source: DebugSourceContext,
    pub source_field_probe: Option<DebugSourceFieldProbe>,
    pub field: String,
    pub field_truncated: bool,
    pub binding: TraceTargetFieldBinding,
    pub pending: DebugInstancePreview,
    pub draft: DebugScopeDraft,
}

/// A bounded snapshot after one graph node succeeds. Expressions can be
/// evaluated outside target construction, so this has no implied target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNodeValue {
    pub node: NodeId,
    pub value: TraceValue,
    pub positions: Vec<TracePosition>,
    pub omitted_outer_positions: usize,
    pub position_paths_truncated: bool,
    pub source: DebugSourceContext,
}

/// One successful node in an isolated reusable function body. Its node ID is
/// local to `function`; positions identify the caller, while source frames are
/// deliberately empty because function bodies cannot read caller source data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFunctionNodeValue {
    pub function: FunctionId,
    pub node: NodeId,
    pub value: TraceValue,
    pub positions: Vec<TracePosition>,
    pub omitted_outer_positions: usize,
    pub position_paths_truncated: bool,
    pub source: DebugSourceContext,
}

/// A bounded value delivered to one recorded input of a consumer graph node.
/// Input indexes are zero-based, as in `TraceEvent::NodeInputValue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNodeInput {
    pub consumer: NodeId,
    pub input: NodeId,
    pub input_index: usize,
    pub value: TraceValue,
    pub positions: Vec<TracePosition>,
    pub omitted_outer_positions: usize,
    pub position_paths_truncated: bool,
    pub source: DebugSourceContext,
}

/// The host decides when to resume a pending write or node evaluation. It may
/// block inside a callback to implement stepping or breakpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugDecision {
    Resume,
    Cancel,
}

/// Optional synchronous control points for writes and graph evaluation.
/// Write callbacks exclude constructors that do not insert ordinary fields.
pub trait DebugHook {
    /// Optionally request one immediate field in one of the four innermost
    /// active source frames. Frame zero is the innermost frame.
    fn source_field_probe(&self) -> Option<(usize, String)> {
        None
    }

    fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision;

    /// Return false when this hook will not inspect successful node values.
    /// The default keeps new hook implementations receiving every value.
    fn wants_node_values(&self) -> bool {
        true
    }

    /// Return true to inspect successful reusable-function body node values.
    /// The separate opt-in prevents existing graph-node hooks from observing
    /// function-local node IDs with overlapping numbers.
    fn wants_function_node_values(&self) -> bool {
        false
    }

    /// Return false when this hook will not inspect delivered graph inputs.
    fn wants_node_inputs(&self) -> bool {
        true
    }

    /// Called after a successful graph evaluation, including filters and
    /// pre-target rules. The host may block here or cancel the run.
    fn after_node_value(&self, _node: &PendingNodeValue) -> DebugDecision {
        DebugDecision::Resume
    }

    /// Called after a successful function-body node and before its caller
    /// continues. The host may block here or cancel the run.
    fn after_function_node_value(&self, _node: &PendingFunctionNodeValue) -> DebugDecision {
        DebugDecision::Resume
    }

    /// Called after a successful value reaches a recorded consumer input and
    /// before the consumer finishes. Untaken inputs do not call this hook.
    fn after_node_input(&self, _input: &PendingNodeInput) -> DebugDecision {
        DebugDecision::Resume
    }
}

pub(crate) fn after_node_value(
    hook: Option<&dyn DebugHook>,
    node: NodeId,
    value: &Value,
    positions: &[PositionFrame],
    context: &[&Instance],
) -> Result<(), EngineError> {
    let Some(hook) = hook.filter(|hook| hook.wants_node_values()) else {
        return Ok(());
    };
    let snapshot = node_value_snapshot(node, value, positions, context);
    match hook.after_node_value(&snapshot) {
        DebugDecision::Resume => Ok(()),
        DebugDecision::Cancel => Err(EngineError::DebugCancelled),
    }
}

pub(crate) fn after_function_node_value(
    hook: Option<&dyn DebugHook>,
    function: FunctionId,
    node: NodeId,
    value: &Value,
    caller_positions: &[PositionFrame],
) -> Result<(), EngineError> {
    let Some(hook) = hook.filter(|hook| hook.wants_function_node_values()) else {
        return Ok(());
    };
    let evaluated = node_value_snapshot(node, value, caller_positions, &[]);
    let snapshot = PendingFunctionNodeValue {
        function,
        node: evaluated.node,
        value: evaluated.value,
        positions: evaluated.positions,
        omitted_outer_positions: evaluated.omitted_outer_positions,
        position_paths_truncated: evaluated.position_paths_truncated,
        source: evaluated.source,
    };
    match hook.after_function_node_value(&snapshot) {
        DebugDecision::Resume => Ok(()),
        DebugDecision::Cancel => Err(EngineError::DebugCancelled),
    }
}

pub(crate) fn after_node_input(
    hook: Option<&dyn DebugHook>,
    consumer: NodeId,
    input: NodeId,
    input_index: usize,
    value: &Value,
    positions: &[PositionFrame],
    context: &[&Instance],
) -> Result<(), EngineError> {
    let Some(hook) = hook.filter(|hook| hook.wants_node_inputs()) else {
        return Ok(());
    };
    let delivered = node_value_snapshot(input, value, positions, context);
    let snapshot = PendingNodeInput {
        consumer,
        input: delivered.node,
        input_index,
        value: delivered.value,
        positions: delivered.positions,
        omitted_outer_positions: delivered.omitted_outer_positions,
        position_paths_truncated: delivered.position_paths_truncated,
        source: delivered.source,
    };
    match hook.after_node_input(&snapshot) {
        DebugDecision::Resume => Ok(()),
        DebugDecision::Cancel => Err(EngineError::DebugCancelled),
    }
}

fn node_value_snapshot(
    node: NodeId,
    value: &Value,
    positions: &[PositionFrame],
    context: &[&Instance],
) -> PendingNodeValue {
    let omitted_outer_positions = positions.len().saturating_sub(MAX_DEBUG_POSITIONS);
    let mut position_paths_truncated = false;
    let positions = positions[omitted_outer_positions..]
        .iter()
        .map(|position| {
            let omitted_segments = position
                .collection
                .len()
                .saturating_sub(MAX_DEBUG_POSITION_SEGMENTS);
            if omitted_segments > 0 {
                position_paths_truncated = true;
            }
            let collection = position.collection[omitted_segments..]
                .iter()
                .map(|segment| {
                    let (name, truncated) = bounded_name(segment);
                    position_paths_truncated |= truncated;
                    name
                })
                .collect();
            let document_path = position.document_path.as_ref().map(|path| {
                let (bounded, truncated) = bounded_name(path);
                position_paths_truncated |= truncated;
                bounded
            });
            TracePosition {
                collection,
                index: position.index,
                grouped: position.grouped,
                join: position.join,
                join_position: position.join_position,
                document_path,
            }
        })
        .collect();
    PendingNodeValue {
        node,
        value: TraceValue::new(value),
        positions,
        omitted_outer_positions,
        position_paths_truncated,
        source: DebugSourceContext::new(context),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn before_target_write(
    hook: Option<&dyn DebugHook>,
    scope: &TraceScope,
    binding: TraceTargetFieldBinding,
    positions: &[PositionFrame],
    context: &[&Instance],
    field: &str,
    pending: &Instance,
    draft: &[(String, Instance)],
) -> Result<(), EngineError> {
    let Some(hook) = hook else {
        return Ok(());
    };
    let (field, field_truncated) = bounded_name(field);
    let source_field_probe = hook
        .source_field_probe()
        .and_then(|(frame_from_inner, name)| {
            if frame_from_inner >= MAX_DEBUG_SOURCE_FRAMES
                || name.is_empty()
                || name.starts_with('\u{1f}')
                || name.chars().count() > MAX_DEBUG_FIELD_NAME_CHARS
            {
                return None;
            }
            let preview = context
                .iter()
                .rev()
                .nth(frame_from_inner)
                .and_then(|frame| frame.field(&name))
                .map(DebugInstancePreview::new);
            Some(DebugSourceFieldProbe {
                frame_from_inner,
                field: name,
                preview,
            })
        });
    let write = PendingTargetWrite {
        scope: scope.clone(),
        positions: trace_positions(positions),
        source: DebugSourceContext::new(context),
        source_field_probe,
        field,
        field_truncated,
        binding,
        pending: DebugInstancePreview::new(pending),
        draft: DebugScopeDraft::new(draft),
    };
    match hook.before_target_write(&write) {
        DebugDecision::Resume => Ok(()),
        DebugDecision::Cancel => Err(EngineError::DebugCancelled),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use ir::Value;

    use super::*;

    #[derive(Default)]
    struct Collector(RefCell<Vec<PendingTargetWrite>>);

    impl DebugHook for Collector {
        fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision {
            self.0.borrow_mut().push(write.clone());
            DebugDecision::Resume
        }
    }

    #[derive(Default)]
    struct NodeCollector(RefCell<Vec<PendingNodeValue>>);

    impl DebugHook for NodeCollector {
        fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
            DebugDecision::Resume
        }

        fn after_node_value(&self, node: &PendingNodeValue) -> DebugDecision {
            self.0.borrow_mut().push(node.clone());
            DebugDecision::Resume
        }
    }

    #[test]
    fn node_snapshot_bounds_value_positions_paths_and_source_frames() {
        let frames = (0..6)
            .map(|index| Instance::Scalar(Value::Int(index)))
            .collect::<Vec<_>>();
        let context = frames.iter().collect::<Vec<_>>();
        let positions = (0..20)
            .map(|index| PositionFrame {
                collection: vec!["é".repeat(300); 20],
                index: index + 1,
                grouped: false,
                join: None,
                join_position: None,
                document_path: Some("é".repeat(300)),
            })
            .collect::<Vec<_>>();
        let collector = NodeCollector::default();
        after_node_value(
            Some(&collector),
            7,
            &Value::String("é".repeat(300)),
            &positions,
            &context,
        )
        .unwrap();
        let nodes = collector.0.borrow();
        let node = &nodes[0];
        assert_eq!(node.node, 7);
        assert_eq!(node.value.preview.chars().count(), 160);
        assert!(node.value.truncated);
        assert_eq!(node.positions.len(), 16);
        assert_eq!(node.omitted_outer_positions, 4);
        assert!(node.position_paths_truncated);
        assert_eq!(node.positions[0].collection.len(), 16);
        assert_eq!(node.positions[0].collection[0].chars().count(), 160);
        assert_eq!(
            node.positions[0]
                .document_path
                .as_ref()
                .unwrap()
                .chars()
                .count(),
            160
        );
        assert_eq!(node.source.frames.len(), 4);
        assert_eq!(node.source.omitted_outer_frames, 2);
    }

    struct ProbingCollector {
        requested_frame: usize,
        requested_field: &'static str,
        writes: RefCell<Vec<PendingTargetWrite>>,
    }

    impl DebugHook for ProbingCollector {
        fn source_field_probe(&self) -> Option<(usize, String)> {
            Some((self.requested_frame, self.requested_field.to_owned()))
        }

        fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision {
            self.writes.borrow_mut().push(write.clone());
            DebugDecision::Resume
        }
    }

    #[test]
    fn pending_and_partial_field_previews_are_bounded() {
        let fields = (0..10)
            .map(|index| {
                (
                    format!("{index}{}", "é".repeat(300)),
                    Instance::Scalar(Value::String("é".repeat(300))),
                )
            })
            .collect::<Vec<_>>();
        let collector = Collector::default();
        before_target_write(
            Some(&collector),
            &TraceScope::primary(),
            TraceTargetFieldBinding::StaticChild,
            &[],
            &[],
            &"é".repeat(300),
            &Instance::Group(vec![("nested".into(), Instance::Scalar(Value::Int(1)))]),
            &fields,
        )
        .unwrap();

        let writes = collector.0.borrow();
        let write = &writes[0];
        assert_eq!(write.field.chars().count(), 160);
        assert!(write.field_truncated);
        assert_eq!(write.pending.length, Some(1));
        assert!(write.pending.value.is_none());
        assert_eq!(write.draft.fields.len(), 8);
        assert_eq!(write.draft.omitted_fields, 2);
        assert_eq!(write.draft.fields[0].name.chars().count(), 160);
        assert!(write.draft.fields[0].name_truncated);
        assert_eq!(
            write.draft.fields[0]
                .preview
                .value
                .as_ref()
                .unwrap()
                .preview
                .chars()
                .count(),
            160
        );
    }

    #[test]
    fn source_snapshot_retains_innermost_frames_with_shallow_bounds() {
        let mut frames = (0..6)
            .map(|index| {
                Instance::Group(vec![("index".into(), Instance::Scalar(Value::Int(index)))])
            })
            .collect::<Vec<_>>();
        let mut source_fields = vec![(
            ir::XML_MIXED_CONTENT_FIELD.into(),
            Instance::Scalar(Value::String("private".into())),
        )];
        source_fields.extend((0..10).map(|index| {
            (
                format!("{index}{}", "é".repeat(300)),
                if index == 0 {
                    Instance::Scalar(Value::String("é".repeat(300)))
                } else {
                    Instance::Repeated(vec![Instance::Scalar(Value::Int(index))])
                },
            )
        }));
        source_fields.push((
            ir::XML_TYPE_FIELD.into(),
            Instance::Scalar(Value::String("private".into())),
        ));
        frames[5] = Instance::Group(source_fields);
        let context = frames.iter().collect::<Vec<_>>();
        let collector = Collector::default();
        before_target_write(
            Some(&collector),
            &TraceScope::primary(),
            TraceTargetFieldBinding::StaticChild,
            &[],
            &context,
            "output",
            &Instance::Scalar(Value::Null),
            &[],
        )
        .unwrap();

        let writes = collector.0.borrow();
        let source = &writes[0].source;
        assert_eq!(source.omitted_outer_frames, 2);
        assert_eq!(source.frames.len(), MAX_DEBUG_SOURCE_FRAMES);
        assert_eq!(source.frames[0].fields[0].name, "index");
        assert_eq!(
            source.frames[0].fields[0]
                .preview
                .value
                .as_ref()
                .unwrap()
                .preview,
            "2"
        );
        let inner = source.frames.last().unwrap();
        assert_eq!(inner.preview.length, Some(10));
        assert_eq!(inner.fields.len(), MAX_DEBUG_SOURCE_FIELDS);
        assert_eq!(inner.omitted_fields, 2);
        assert!(
            inner
                .fields
                .iter()
                .all(|field| !field.name.starts_with('\u{1f}'))
        );
        assert_eq!(
            inner.fields[0].name.chars().count(),
            MAX_DEBUG_FIELD_NAME_CHARS
        );
        assert!(inner.fields[0].name_truncated);
        let scalar = inner.fields[0].preview.value.as_ref().unwrap();
        assert_eq!(scalar.preview.chars().count(), 160);
        assert!(scalar.truncated);
        assert_eq!(inner.fields[1].preview.length, Some(1));
    }

    #[test]
    fn source_probe_reaches_an_exact_field_outside_the_shallow_snapshot() {
        let outer = Instance::Group(vec![(
            "ninth".into(),
            Instance::Scalar(Value::String("outer".into())),
        )]);
        let inner = Instance::Group(
            (0..8)
                .map(|index| (format!("field{index}"), Instance::Scalar(Value::Int(index))))
                .chain([(
                    "ninth".into(),
                    Instance::Scalar(Value::String("inner".into())),
                )])
                .collect(),
        );
        let collector = ProbingCollector {
            requested_frame: 0,
            requested_field: "ninth",
            writes: RefCell::new(Vec::new()),
        };
        before_target_write(
            Some(&collector),
            &TraceScope::primary(),
            TraceTargetFieldBinding::StaticChild,
            &[],
            &[&outer, &inner],
            "output",
            &Instance::Scalar(Value::Null),
            &[],
        )
        .unwrap();
        let writes = collector.writes.borrow();
        let write = &writes[0];
        assert_eq!(write.source.frames[1].omitted_fields, 1);
        let probe = write.source_field_probe.as_ref().unwrap();
        assert_eq!(probe.frame_from_inner, 0);
        assert_eq!(probe.field, "ninth");
        assert_eq!(
            probe
                .preview
                .as_ref()
                .unwrap()
                .value
                .as_ref()
                .unwrap()
                .preview,
            "inner"
        );
        drop(writes);

        let outer_collector = ProbingCollector {
            requested_frame: 1,
            requested_field: "ninth",
            writes: RefCell::new(Vec::new()),
        };
        before_target_write(
            Some(&outer_collector),
            &TraceScope::primary(),
            TraceTargetFieldBinding::StaticChild,
            &[],
            &[&outer, &inner],
            "output",
            &Instance::Scalar(Value::Null),
            &[],
        )
        .unwrap();
        let outer_writes = outer_collector.writes.borrow();
        let outer_probe = outer_writes[0].source_field_probe.as_ref().unwrap();
        assert_eq!(outer_probe.frame_from_inner, 1);
        assert_eq!(
            outer_probe
                .preview
                .as_ref()
                .unwrap()
                .value
                .as_ref()
                .unwrap()
                .preview,
            "outer"
        );
    }
}
