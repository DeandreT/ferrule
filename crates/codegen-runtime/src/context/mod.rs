mod recursive;
mod resolve;
mod walk;

pub use resolve::{InstanceKind, SourcePathError, clone_scalar, resolve_scalar};

use crate::{Instance, Value};

/// Owns scalar instances materialized by one generated sequence while its
/// borrowed candidate contexts are evaluated.
pub struct GeneratedItems {
    items: Vec<Instance>,
}

impl GeneratedItems {
    pub fn new(values: Vec<Value>) -> Self {
        Self {
            items: values.into_iter().map(Instance::Scalar).collect(),
        }
    }
}

/// Ordered source frames visible to one generated target scope.
///
/// Cloning a context clones only its frame metadata; source instances remain
/// borrowed from the input. Source iteration appends one frame for every
/// repeated or document collection it crosses, plus a plain terminal frame
/// when the selected value is not itself a collection.
#[derive(Clone)]
pub struct ScopeContext<'a> {
    frames: Vec<ScopeFrame<'a>>,
}

#[derive(Clone)]
struct ScopeFrame<'a> {
    instance: &'a Instance,
    collection: Option<CollectionIdentity>,
}

#[derive(Clone)]
enum CollectionIdentity {
    Repeated { path: Vec<String>, index: usize },
    Document { path: Vec<String>, index: usize },
}

impl CollectionIdentity {
    fn path(&self) -> &[String] {
        match self {
            Self::Repeated { path, .. } | Self::Document { path, .. } => path,
        }
    }

    const fn index(&self) -> usize {
        match self {
            Self::Repeated { index, .. } | Self::Document { index, .. } => *index,
        }
    }

    const fn set_index(&mut self, compact_index: usize) {
        match self {
            Self::Repeated { index, .. } | Self::Document { index, .. } => {
                *index = compact_index;
            }
        }
    }
}

impl<'a> ScopeContext<'a> {
    /// Creates the root context for one generated mapping execution.
    pub fn new(source: &'a Instance) -> Self {
        Self {
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
            }],
        }
    }

    /// Returns the 1-based position of the innermost active collection whose
    /// path ends with `collection`. An empty path selects the innermost active
    /// collection. Missing collection context has the engine's default value
    /// of `1`.
    pub fn position(&self, collection: &[&str]) -> usize {
        self.frames
            .iter()
            .rev()
            .filter_map(|frame| frame.collection.as_ref())
            .find(|identity| {
                collection.is_empty() || string_path_has_suffix(identity.path(), collection)
            })
            .map(CollectionIdentity::index)
            .unwrap_or(1)
    }

    /// Clones this context and replaces only its innermost collection's
    /// position. Source instances remain borrowed and unchanged.
    ///
    /// Generated filtered scopes use this view for output expressions after
    /// evaluating the predicate against the original candidate context.
    pub fn with_compact_last_position(&self, compact_index: usize) -> Self {
        let mut compact = self.clone();
        if let Some(collection) = compact
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.collection.as_mut())
        {
            collection.set_index(compact_index.max(1));
        }
        compact
    }

    /// Reborrows the parent frames and appends each generated scalar as an
    /// empty-path collection frame with its raw one-based position.
    pub fn generated_item_contexts<'b>(
        &'b self,
        items: &'b GeneratedItems,
    ) -> impl Iterator<Item = ScopeContext<'b>> + 'b
    where
        'a: 'b,
    {
        items.items.iter().enumerate().map(move |(index, item)| {
            let mut frames = self
                .frames
                .iter()
                .map(|frame| ScopeFrame {
                    instance: frame.instance,
                    collection: frame.collection.clone(),
                })
                .collect::<Vec<_>>();
            frames.push(collection_frame(
                item,
                CollectionIdentity::Repeated {
                    path: Vec::new(),
                    index: index + 1,
                },
            ));
            ScopeContext { frames }
        })
    }

    /// Materializes all generated-item contexts for scope sorting, filtering,
    /// and windowing while retaining the lazy iterator's frame semantics.
    pub fn generated_items<'b>(&'b self, items: &'b GeneratedItems) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        self.generated_item_contexts(items).collect()
    }
}

fn collection_frame<'a>(instance: &'a Instance, collection: CollectionIdentity) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
    }
}

fn has_prefix(path: &[&str], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(segment, expected)| *segment == expected)
}

fn has_suffix(path: &[&str], suffix: &[String]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| *segment == expected)
}

fn same_path(path: &[&str], expected: &[String]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(segment, expected)| *segment == expected)
}

fn string_path_has_suffix(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| segment == expected)
}
