//! Separate semantic and layout revisions with a content-addressed save point.
//!
//! A semantic revision identifies persisted mapping content; a layout
//! revision identifies canvas positions and other view state saved beside
//! it. Equal revisions always denote equal content, so derived values such
//! as fingerprints can be cached by revision instead of recomputed from the
//! project.

use crate::DocumentOrigin;

/// Identity of one observed mapping project content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SemanticRevision(u64);

/// Identity of one observed canvas layout and view-state content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayoutRevision(u64);

/// The semantic and layout revisions of one observed editor state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocumentRevision {
    semantic: SemanticRevision,
    layout: LayoutRevision,
}

impl DocumentRevision {
    pub const fn semantic(self) -> SemanticRevision {
        self.semantic
    }

    pub const fn layout(self) -> LayoutRevision {
        self.layout
    }
}

/// How one revision component's content relates to the previous observation
/// and to the save point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentChange {
    /// The content equals the previous observation.
    Unchanged,
    /// The content differs from the previous observation and the save point.
    Changed,
    /// The content differs from the previous observation but equals the save
    /// point, so it adopts the saved revision.
    MatchesSaved,
}

impl ComponentChange {
    /// Classifies content, comparing against the save point only after it
    /// is known to differ from the previous observation.
    pub fn classify<T: PartialEq + ?Sized>(current: &T, previous: &T, saved: Option<&T>) -> Self {
        if current == previous {
            Self::Unchanged
        } else if saved == Some(current) {
            Self::MatchesSaved
        } else {
            Self::Changed
        }
    }
}

/// The classified change of both revision components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionChange {
    pub semantic: ComponentChange,
    pub layout: ComponentChange,
}

impl RevisionChange {
    pub const NONE: Self = Self {
        semantic: ComponentChange::Unchanged,
        layout: ComponentChange::Unchanged,
    };

    pub fn is_unchanged(self) -> bool {
        self == Self::NONE
    }
}

/// Current and saved revisions of one editor document.
///
/// Hosts classify every observed transition against the previous observation
/// and the save point, so the document is clean exactly when the observed
/// content equals the saved content, including after undo or redo returns to
/// it.
#[derive(Debug)]
pub struct RevisionTracker {
    current: DocumentRevision,
    saved: Option<DocumentRevision>,
    next: u64,
}

impl RevisionTracker {
    pub fn new(origin: DocumentOrigin) -> Self {
        let mut tracker = Self {
            current: DocumentRevision {
                semantic: SemanticRevision(0),
                layout: LayoutRevision(0),
            },
            saved: None,
            next: 1,
        };
        tracker.saved = (origin == DocumentOrigin::Saved).then_some(tracker.current);
        tracker
    }

    pub const fn current(&self) -> DocumentRevision {
        self.current
    }

    pub const fn saved(&self) -> Option<DocumentRevision> {
        self.saved
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.current)
    }

    pub fn is_semantically_dirty(&self) -> bool {
        self.saved.map(DocumentRevision::semantic) != Some(self.current.semantic)
    }

    pub fn is_layout_dirty(&self) -> bool {
        self.saved.map(DocumentRevision::layout) != Some(self.current.layout)
    }

    /// Advances the current revision for one observed transition.
    pub fn record(&mut self, change: RevisionChange) -> DocumentRevision {
        let saved = self.saved;
        let semantic = match (change.semantic, saved) {
            (ComponentChange::Unchanged, _) => self.current.semantic,
            (ComponentChange::MatchesSaved, Some(saved)) => saved.semantic,
            (ComponentChange::MatchesSaved | ComponentChange::Changed, _) => {
                SemanticRevision(self.fresh())
            }
        };
        let layout = match (change.layout, saved) {
            (ComponentChange::Unchanged, _) => self.current.layout,
            (ComponentChange::MatchesSaved, Some(saved)) => saved.layout,
            (ComponentChange::MatchesSaved | ComponentChange::Changed, _) => {
                LayoutRevision(self.fresh())
            }
        };
        self.current = DocumentRevision { semantic, layout };
        self.current
    }

    /// Records that the host persisted the currently observed content.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.current);
    }

    /// Records that the host persisted content that differs from the current
    /// observation in the indicated components. Those components receive
    /// fresh saved revisions that the next matching observation adopts.
    pub fn mark_saved_ahead(&mut self, semantic_differs: bool, layout_differs: bool) {
        let semantic = if semantic_differs {
            SemanticRevision(self.fresh())
        } else {
            self.current.semantic
        };
        let layout = if layout_differs {
            LayoutRevision(self.fresh())
        } else {
            self.current.layout
        };
        self.saved = Some(DocumentRevision { semantic, layout });
    }

    pub fn mark_unsaved(&mut self) {
        self.saved = None;
    }

    /// Starts a new revision lineage, as when a different project is opened.
    pub fn rebase(&mut self, origin: DocumentOrigin) {
        self.current = DocumentRevision {
            semantic: SemanticRevision(self.fresh()),
            layout: LayoutRevision(self.fresh()),
        };
        self.saved = (origin == DocumentOrigin::Saved).then_some(self.current);
    }

    fn fresh(&mut self) -> u64 {
        let revision = self.next;
        self.next += 1;
        revision
    }
}

/// A value derived from one revision, recomputed only when the revision
/// changes.
#[derive(Debug)]
pub struct RevisionKeyed<K, T> {
    entry: Option<(K, T)>,
    computations: u64,
}

impl<K, T> Default for RevisionKeyed<K, T> {
    fn default() -> Self {
        Self {
            entry: None,
            computations: 0,
        }
    }
}

impl<K: Copy + Eq, T> RevisionKeyed<K, T> {
    pub fn get(&self, revision: K) -> Option<&T> {
        self.entry
            .as_ref()
            .filter(|(key, _)| *key == revision)
            .map(|(_, value)| value)
    }

    pub fn get_or_compute(&mut self, revision: K, compute: impl FnOnce() -> T) -> &T {
        let entry = match self.entry.take() {
            Some((key, value)) if key == revision => (key, value),
            _ => {
                self.computations += 1;
                (revision, compute())
            }
        };
        &self.entry.insert(entry).1
    }

    /// Number of times a value was computed because its revision changed.
    pub const fn computations(&self) -> u64 {
        self.computations
    }

    pub fn clear(&mut self) {
        self.entry = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEMANTIC: RevisionChange = RevisionChange {
        semantic: ComponentChange::Changed,
        layout: ComponentChange::Unchanged,
    };
    const LAYOUT: RevisionChange = RevisionChange {
        semantic: ComponentChange::Unchanged,
        layout: ComponentChange::Changed,
    };

    #[test]
    fn semantic_and_layout_changes_advance_independent_components() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Saved);
        let saved = tracker.current();

        let semantic = tracker.record(SEMANTIC);
        assert_ne!(semantic.semantic(), saved.semantic());
        assert_eq!(semantic.layout(), saved.layout());
        assert!(tracker.is_dirty() && tracker.is_semantically_dirty());
        assert!(!tracker.is_layout_dirty());

        let layout = tracker.record(LAYOUT);
        assert_eq!(layout.semantic(), semantic.semantic());
        assert_ne!(layout.layout(), saved.layout());
        assert!(tracker.is_layout_dirty());

        assert_eq!(tracker.record(RevisionChange::NONE), layout);
    }

    #[test]
    fn layout_only_changes_never_mark_the_document_semantically_dirty() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Saved);
        tracker.record(LAYOUT);
        assert!(tracker.is_dirty());
        assert!(!tracker.is_semantically_dirty());
    }

    #[test]
    fn content_matching_the_save_point_adopts_its_revision() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Saved);
        let saved = tracker.current();
        tracker.record(SEMANTIC);
        tracker.record(LAYOUT);
        let restored = tracker.record(RevisionChange {
            semantic: ComponentChange::MatchesSaved,
            layout: ComponentChange::MatchesSaved,
        });
        assert_eq!(restored, saved);
        assert!(!tracker.is_dirty());
    }

    #[test]
    fn unsaved_documents_never_adopt_a_save_point() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Unsaved);
        let initial = tracker.current();
        let next = tracker.record(RevisionChange {
            semantic: ComponentChange::MatchesSaved,
            layout: ComponentChange::Unchanged,
        });
        assert_ne!(next.semantic(), initial.semantic());
        assert!(tracker.is_dirty());
        tracker.mark_saved();
        assert!(!tracker.is_dirty());
    }

    #[test]
    fn a_save_ahead_of_the_observation_is_adopted_by_the_matching_observation() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Unsaved);
        let observed = tracker.current();
        tracker.mark_saved_ahead(true, false);
        assert!(tracker.is_semantically_dirty() && !tracker.is_layout_dirty());

        let next = tracker.record(RevisionChange {
            semantic: ComponentChange::MatchesSaved,
            layout: ComponentChange::Unchanged,
        });
        assert_ne!(next.semantic(), observed.semantic());
        assert!(!tracker.is_dirty());
    }

    #[test]
    fn rebase_starts_a_fresh_lineage() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Saved);
        let first = tracker.current();
        tracker.rebase(DocumentOrigin::Unsaved);
        assert_ne!(tracker.current(), first);
        assert!(tracker.is_dirty());
        tracker.rebase(DocumentOrigin::Saved);
        assert!(!tracker.is_dirty());
    }

    #[test]
    fn classification_compares_the_save_point_only_for_changed_content() {
        assert_eq!(
            ComponentChange::classify("a", "a", Some("b")),
            ComponentChange::Unchanged
        );
        assert_eq!(
            ComponentChange::classify("b", "a", Some("b")),
            ComponentChange::MatchesSaved
        );
        assert_eq!(
            ComponentChange::classify("c", "a", Some("b")),
            ComponentChange::Changed
        );
        assert_eq!(
            ComponentChange::classify("c", "a", None),
            ComponentChange::Changed
        );
    }

    #[test]
    fn keyed_values_recompute_only_when_the_revision_changes() {
        let mut tracker = RevisionTracker::new(DocumentOrigin::Saved);
        let mut cache = RevisionKeyed::default();
        let mut computed = 0;
        let mut fingerprint = |cache: &mut RevisionKeyed<SemanticRevision, u64>,
                               revision: DocumentRevision| {
            *cache.get_or_compute(revision.semantic(), || {
                computed += 1;
                computed
            })
        };

        let first = fingerprint(&mut cache, tracker.current());
        for _ in 0..5 {
            assert_eq!(fingerprint(&mut cache, tracker.current()), first);
        }
        assert_eq!(cache.computations(), 1);

        let layout_only = tracker.record(LAYOUT);
        assert_eq!(fingerprint(&mut cache, layout_only), first);
        assert_eq!(cache.computations(), 1);

        let semantic = tracker.record(SEMANTIC);
        assert_ne!(fingerprint(&mut cache, semantic), first);
        assert_eq!(fingerprint(&mut cache, semantic), 2);
        assert_eq!(cache.computations(), 2);
        assert!(cache.get(layout_only.semantic()).is_none());
    }
}
