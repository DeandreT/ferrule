use std::collections::VecDeque;

use crate::HistoryLimit;

/// One reversible history step: a retained snapshot of the state it
/// restores, or a typed command that knows how to invert itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryRecord<S, C> {
    Snapshot(S),
    Command(C),
}

#[derive(Clone, Debug)]
struct Entry<S, C> {
    record: HistoryRecord<S, C>,
    label: String,
}

/// Bounded undo/redo history mixing snapshot and command steps.
///
/// This is the migration bridge for hosts whose renderers still mutate a
/// project in place: those edits keep snapshot records, while migrated edit
/// families record commands. Both kinds share one linear order, so undo and
/// redo interleave them exactly as they were performed. Dirty state belongs to
/// [`crate::RevisionTracker`], not to the retained records.
#[derive(Debug)]
pub struct EditHistory<S, C> {
    undo: VecDeque<Entry<S, C>>,
    redo: VecDeque<Entry<S, C>>,
    limit: HistoryLimit,
}

impl<S, C> Default for EditHistory<S, C> {
    fn default() -> Self {
        Self::with_history_limit(HistoryLimit::default())
    }
}

impl<S, C> EditHistory<S, C> {
    pub fn with_history_limit(limit: HistoryLimit) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            limit,
        }
    }

    /// Records a completed edit and discards the redo branch.
    pub fn record(&mut self, record: HistoryRecord<S, C>, label: impl Into<String>) {
        push_bounded(
            &mut self.undo,
            Entry {
                record,
                label: label.into(),
            },
            self.limit,
        );
        self.redo.clear();
    }

    pub fn clear(&mut self) {
        self.undo.clear();
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

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|entry| entry.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.back().map(|entry| entry.label.as_str())
    }

    /// The step the next undo reverses.
    pub fn next_undo(&self) -> Option<&HistoryRecord<S, C>> {
        self.undo.back().map(|entry| &entry.record)
    }

    /// The step the next redo reapplies.
    pub fn next_redo(&self) -> Option<&HistoryRecord<S, C>> {
        self.redo.back().map(|entry| &entry.record)
    }

    /// The step the next undo reverses, for hosts that extend the newest
    /// edit in place instead of recording another step. Its label is kept
    /// and the redo branch is discarded, as for [`Self::record`].
    pub fn newest_undo_mut(&mut self) -> Option<&mut HistoryRecord<S, C>> {
        self.redo.clear();
        self.undo.back_mut().map(|entry| &mut entry.record)
    }

    /// Reverses the newest undo step.
    ///
    /// `reverse` restores the record's state and turns the record into the
    /// step that reapplies the current state: a snapshot record exchanges its
    /// snapshot for the current one, while a command record keeps its
    /// command. When `reverse` fails it must leave the record and host state
    /// untouched; the step then stays on the undo stack.
    pub fn undo<E>(
        &mut self,
        reverse: impl FnOnce(&mut HistoryRecord<S, C>) -> Result<(), E>,
    ) -> Result<Option<String>, E> {
        step(&mut self.undo, &mut self.redo, self.limit, reverse)
    }

    /// Reapplies the newest redo step; `reapply` follows the [`Self::undo`]
    /// contract in the opposite direction.
    pub fn redo<E>(
        &mut self,
        reapply: impl FnOnce(&mut HistoryRecord<S, C>) -> Result<(), E>,
    ) -> Result<Option<String>, E> {
        step(&mut self.redo, &mut self.undo, self.limit, reapply)
    }

    /// Updates every retained snapshot in place, for hosts whose snapshots
    /// must follow a change of the basis they are interpreted against.
    /// Command records are not visited. Stops at the first error.
    pub fn try_for_each_retained_snapshot_mut<E>(
        &mut self,
        mut update: impl FnMut(&mut S) -> Result<(), E>,
    ) -> Result<(), E> {
        self.undo
            .iter_mut()
            .chain(self.redo.iter_mut())
            .try_for_each(|entry| match &mut entry.record {
                HistoryRecord::Snapshot(snapshot) => update(snapshot),
                HistoryRecord::Command(_) => Ok(()),
            })
    }
}

fn step<S, C, E>(
    from: &mut VecDeque<Entry<S, C>>,
    to: &mut VecDeque<Entry<S, C>>,
    limit: HistoryLimit,
    exchange: impl FnOnce(&mut HistoryRecord<S, C>) -> Result<(), E>,
) -> Result<Option<String>, E> {
    let Some(mut entry) = from.pop_back() else {
        return Ok(None);
    };
    if let Err(error) = exchange(&mut entry.record) {
        from.push_back(entry);
        return Err(error);
    }
    let label = entry.label.clone();
    push_bounded(to, entry, limit);
    Ok(Some(label))
}

fn push_bounded<S, C>(
    entries: &mut VecDeque<Entry<S, C>>,
    entry: Entry<S, C>,
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

    /// A host whose state is one integer: snapshots hold whole values and
    /// commands add a signed delta.
    struct Host {
        value: i32,
        history: EditHistory<i32, i32>,
    }

    impl Host {
        fn new() -> Self {
            Self {
                value: 0,
                history: EditHistory::default(),
            }
        }

        fn snapshot_edit(&mut self, value: i32) {
            self.history
                .record(HistoryRecord::Snapshot(self.value), "set");
            self.value = value;
        }

        fn command_edit(&mut self, delta: i32) {
            self.value += delta;
            self.history.record(HistoryRecord::Command(delta), "add");
        }

        fn exchange(value: &mut i32, record: &mut HistoryRecord<i32, i32>, sign: i32) {
            match record {
                HistoryRecord::Snapshot(snapshot) => std::mem::swap(value, snapshot),
                HistoryRecord::Command(delta) => *value += sign * *delta,
            }
        }

        fn undo(&mut self) -> Option<String> {
            let value = &mut self.value;
            let result: Result<_, ()> = self.history.undo(|record| {
                Self::exchange(value, record, -1);
                Ok(())
            });
            result.ok().flatten()
        }

        fn redo(&mut self) -> Option<String> {
            let value = &mut self.value;
            let result: Result<_, ()> = self.history.redo(|record| {
                Self::exchange(value, record, 1);
                Ok(())
            });
            result.ok().flatten()
        }
    }

    #[test]
    fn snapshot_and_command_steps_interleave_in_edit_order() {
        let mut host = Host::new();
        host.snapshot_edit(10);
        host.command_edit(5);
        host.snapshot_edit(40);
        host.command_edit(-1);
        assert_eq!(host.value, 39);

        let mut undone = Vec::new();
        while let Some(label) = host.undo() {
            undone.push((label, host.value));
        }
        assert_eq!(
            undone,
            [
                ("add".to_owned(), 40),
                ("set".to_owned(), 15),
                ("add".to_owned(), 10),
                ("set".to_owned(), 0)
            ]
        );

        let mut redone = Vec::new();
        while let Some(label) = host.redo() {
            redone.push((label, host.value));
        }
        assert_eq!(
            redone,
            [
                ("set".to_owned(), 10),
                ("add".to_owned(), 15),
                ("set".to_owned(), 40),
                ("add".to_owned(), 39)
            ]
        );
    }

    #[test]
    fn a_partial_undo_then_new_edit_discards_the_mixed_redo_branch() {
        let mut host = Host::new();
        host.command_edit(1);
        host.snapshot_edit(7);
        assert!(host.undo().is_some());
        assert!(host.history.can_redo());

        host.command_edit(2);
        assert!(!host.history.can_redo());
        assert_eq!(host.undo(), Some("add".to_owned()));
        assert_eq!(host.value, 1);
        assert_eq!(host.undo(), Some("add".to_owned()));
        assert_eq!(host.value, 0);
    }

    #[test]
    fn a_rejected_step_stays_in_place_without_changing_either_stack() {
        let mut history: EditHistory<i32, i32> = EditHistory::default();
        history.record(HistoryRecord::Command(3), "add");

        assert_eq!(history.undo(|_| Err("stale")), Err("stale"));
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.redo_len(), 0);
        assert_eq!(history.undo_label(), Some("add"));

        let undone: Result<_, ()> = history.undo(|_| Ok(()));
        assert_eq!(undone, Ok(Some("add".to_owned())));
        assert_eq!(history.redo(|_| Err("stale")), Err("stale"));
        assert_eq!(history.redo_len(), 1);
        assert_eq!(history.redo_label(), Some("add"));
    }

    #[test]
    fn retained_snapshot_updates_skip_command_records() {
        let mut history: EditHistory<i32, i32> = EditHistory::default();
        history.record(HistoryRecord::Snapshot(1), "set");
        history.record(HistoryRecord::Command(2), "add");
        history.record(HistoryRecord::Snapshot(3), "set");
        let redo: Result<_, ()> = history.undo(|_| Ok(()));
        assert!(redo.is_ok());

        let mut visited = Vec::new();
        let updated: Result<(), ()> = history.try_for_each_retained_snapshot_mut(|snapshot| {
            visited.push(*snapshot);
            *snapshot *= 10;
            Ok(())
        });
        assert!(updated.is_ok());
        assert_eq!(visited, [1, 3]);

        let mut records = Vec::new();
        let collect = |record: &mut HistoryRecord<i32, i32>| {
            records.push(record.clone());
            Ok::<(), ()>(())
        };
        assert!(history.redo(collect).is_ok());
        assert_eq!(records, [HistoryRecord::Snapshot(30)]);
    }

    #[test]
    fn history_limit_evicts_the_oldest_record_of_either_kind() {
        let limit = HistoryLimit::new(2).expect("test limit is non-zero");
        let mut history: EditHistory<i32, i32> = EditHistory::with_history_limit(limit);
        history.record(HistoryRecord::Command(1), "first");
        history.record(HistoryRecord::Snapshot(2), "second");
        history.record(HistoryRecord::Command(3), "third");
        assert_eq!(history.undo_len(), 2);

        let mut labels = Vec::new();
        while let Ok(Some(label)) = history.undo(|_| Ok::<(), ()>(())) {
            labels.push(label);
        }
        assert_eq!(labels, ["third", "second"]);
    }
}
