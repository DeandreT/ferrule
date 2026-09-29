//! Opt-in control point for a host-driven live debugger.

use ir::Instance;

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
    pub field: String,
    pub field_truncated: bool,
    pub binding: TraceTargetFieldBinding,
    pub pending: DebugInstancePreview,
    pub draft: DebugScopeDraft,
}

/// The host decides when to resume the current write. It may block inside the
/// callback to implement stepping or breakpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugDecision {
    Resume,
    Cancel,
}

/// Optional, synchronous control point before an ordinary target-field write.
/// No callback is made for constructors that do not insert ordinary fields.
pub trait DebugHook {
    fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision;
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
    let write = PendingTargetWrite {
        scope: scope.clone(),
        positions: trace_positions(positions),
        source: DebugSourceContext::new(context),
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
}
