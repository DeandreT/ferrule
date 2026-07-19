use std::collections::VecDeque;

use crate::{DocumentOrigin, HistoryLimit};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotEntry<T> {
    snapshot: T,
    label: String,
}

/// One restored snapshot and the user-facing label of its edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryStep<T> {
    snapshot: T,
    label: String,
}

impl<T> HistoryStep<T> {
    pub fn snapshot(&self) -> &T {
        &self.snapshot
    }

    pub fn into_snapshot(self) -> T {
        self.snapshot
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

/// Bounded history for editor state that remains owned by a host.
///
/// This is the migration adapter for renderers that still mutate a project
/// and layout in place. New editing surfaces should prefer [`crate::EditorDocument`]
/// and typed commands.
pub struct SnapshotHistory<T> {
    saved: Option<T>,
    undo: VecDeque<SnapshotEntry<T>>,
    redo: VecDeque<SnapshotEntry<T>>,
    limit: HistoryLimit,
}

impl<T: Clone + PartialEq> SnapshotHistory<T> {
    pub fn new(initial: T, origin: DocumentOrigin) -> Self {
        Self::with_history_limit(initial, origin, HistoryLimit::default())
    }

    pub fn with_history_limit(initial: T, origin: DocumentOrigin, limit: HistoryLimit) -> Self {
        Self {
            saved: (origin == DocumentOrigin::Saved).then_some(initial),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            limit,
        }
    }

    pub fn is_dirty(&self, current: &T) -> bool {
        self.saved.as_ref() != Some(current)
    }

    pub fn is_dirty_by<K: PartialEq>(&self, current: &K, key: impl FnOnce(&T) -> &K) -> bool {
        self.saved.as_ref().map(key) != Some(current)
    }

    pub fn mark_saved(&mut self, current: &T) {
        self.saved = Some(current.clone());
    }

    pub fn mark_unsaved(&mut self) {
        self.saved = None;
    }

    pub fn rebase(&mut self, current: T, origin: DocumentOrigin) {
        self.saved = (origin == DocumentOrigin::Saved).then_some(current);
        self.undo.clear();
        self.redo.clear();
    }

    pub fn record(&mut self, before: T, label: impl Into<String>) {
        self.push_undo(SnapshotEntry {
            snapshot: before,
            label: label.into(),
        });
        self.redo.clear();
    }

    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn undo(&mut self, current: T) -> Option<HistoryStep<T>> {
        let previous = self.undo.pop_back()?;
        let label = previous.label.clone();
        self.push_redo(SnapshotEntry {
            snapshot: current,
            label: previous.label,
        });
        Some(HistoryStep {
            snapshot: previous.snapshot,
            label,
        })
    }

    pub fn redo(&mut self, current: T) -> Option<HistoryStep<T>> {
        let next = self.redo.pop_back()?;
        let label = next.label.clone();
        self.push_undo(SnapshotEntry {
            snapshot: current,
            label: next.label,
        });
        Some(HistoryStep {
            snapshot: next.snapshot,
            label,
        })
    }

    fn push_undo(&mut self, entry: SnapshotEntry<T>) {
        push_bounded(&mut self.undo, entry, self.limit);
    }

    fn push_redo(&mut self, entry: SnapshotEntry<T>) {
        push_bounded(&mut self.redo, entry, self.limit);
    }
}

fn push_bounded<T>(
    entries: &mut VecDeque<SnapshotEntry<T>>,
    entry: SnapshotEntry<T>,
    limit: HistoryLimit,
) {
    if entries.len() == limit.get() {
        entries.pop_front();
    }
    entries.push_back(entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_points_follow_undo_and_redo_snapshots() {
        let mut history = SnapshotHistory::new("saved", DocumentOrigin::Saved);
        history.record("saved", "Edit name");
        assert!(history.is_dirty(&"changed"));

        let previous = history.undo("changed").expect("undo state exists");
        assert_eq!(previous.label(), "Edit name");
        assert_eq!(previous.snapshot(), &"saved");
        assert!(!history.is_dirty(previous.snapshot()));

        let next = history.redo("saved").expect("redo state exists");
        assert_eq!(next.snapshot(), &"changed");
        assert!(history.is_dirty(next.snapshot()));
    }

    #[test]
    fn rebase_and_unsaved_origin_reset_history_and_save_state() {
        let mut history = SnapshotHistory::new(1, DocumentOrigin::Saved);
        history.record(1, "Increment");
        history.rebase(2, DocumentOrigin::Unsaved);

        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert!(history.is_dirty(&2));
        history.mark_saved(&2);
        assert!(!history.is_dirty(&2));
        history.mark_unsaved();
        assert!(history.is_dirty(&2));
    }

    #[test]
    fn history_limit_evicts_the_oldest_snapshot() {
        let limit = HistoryLimit::new(2).expect("test limit is non-zero");
        let mut history = SnapshotHistory::with_history_limit(0, DocumentOrigin::Saved, limit);
        history.record(0, "One");
        history.record(1, "Two");
        history.record(2, "Three");

        assert_eq!(history.undo_len(), 2);
        assert_eq!(history.undo(3).map(HistoryStep::into_snapshot), Some(2));
        assert_eq!(history.undo(2).map(HistoryStep::into_snapshot), Some(1));
        assert!(history.undo(1).is_none());
    }
}
