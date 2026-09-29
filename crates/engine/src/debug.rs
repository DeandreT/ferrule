//! Opt-in control point for a host-driven live debugger.

use ir::Instance;

use crate::EngineError;
use crate::source_iteration::PositionFrame;
use crate::trace::{
    TraceOutputKind, TracePosition, TraceScope, TraceTargetFieldBinding, TraceValue,
    trace_positions,
};

const MAX_DEBUG_DRAFT_FIELDS: usize = 8;
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

/// One already-inserted field in the current target scope's partial draft.
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
            fields: fields
                .iter()
                .take(MAX_DEBUG_DRAFT_FIELDS)
                .map(|(name, instance)| {
                    let (name, name_truncated) = bounded_name(name);
                    DebugDraftField {
                        name,
                        name_truncated,
                        preview: DebugInstancePreview::new(instance),
                    }
                })
                .collect(),
            omitted_fields: fields.len().saturating_sub(MAX_DEBUG_DRAFT_FIELDS),
        }
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
}
