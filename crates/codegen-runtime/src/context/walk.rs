use crate::{Instance, Value};

use super::{CollectionIdentity, ScopeContext, ScopeFrame, collection_frame};

impl<'a> ScopeContext<'a> {
    /// Produces one child context for every item selected by `path`.
    ///
    /// The path is evaluated from the innermost frame that owns its first
    /// field, falling back to the current frame. An empty path iterates the
    /// current repeated/document value, or selects one ordinary current
    /// value. Repetition crossed at any depth branches in source order.
    pub fn walk_source(&self, path: &[&str]) -> Vec<Self> {
        let Some(base) = self
            .frames
            .iter()
            .rev()
            .find(|frame| match path.first() {
                Some(first) => frame.instance.field(first).is_some(),
                None => true,
            })
            .or_else(|| self.frames.last())
        else {
            return Vec::new();
        };

        walk_source_frames(base.instance, path, &[], &[])
            .into_iter()
            .map(|extension| self.extend(extension))
            .collect()
    }

    /// Enumerates collection items with the engine's aggregate root-selection
    /// rules.
    ///
    /// A named path starts at the innermost frame that owns its first segment.
    /// An empty path starts at the innermost repeated or document-set frame.
    /// When no such frame exists the collection is empty; this operation never
    /// falls back to an unrelated ordinary frame.
    pub fn aggregate_items(&self, path: &[&str]) -> Vec<Self> {
        let base = match path.first() {
            Some(first) => self
                .frames
                .iter()
                .rev()
                .find(|frame| frame.instance.field(first).is_some()),
            None => self.frames.iter().rev().find(|frame| {
                matches!(
                    frame.instance,
                    Instance::Repeated(_) | Instance::DocumentSet(_)
                )
            }),
        };
        let Some(base) = base else {
            return Vec::new();
        };

        walk_source_frames(base.instance, path, &[], &[])
            .into_iter()
            .map(|extension| self.extend(extension))
            .collect()
    }

    /// Reads a direct aggregate value path from only the current terminal
    /// frame. Missing and structural values reduce as `Null`; enclosing frames
    /// are never consulted.
    pub fn aggregate_current_scalar(&self, path: &[&str]) -> Value {
        self.frames
            .last()
            .and_then(|frame| direct_scalar(frame.instance, path))
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn extend(&self, extension: Vec<ScopeFrame<'a>>) -> Self {
        let mut frames = self.frames.clone();
        frames.extend(extension);
        Self { frames }
    }
}

fn walk_source_frames<'a>(
    base: &'a Instance,
    path: &[&str],
    prefix: &[String],
    acc: &[ScopeFrame<'a>],
) -> Vec<Vec<ScopeFrame<'a>>> {
    if !path.is_empty()
        && let Instance::Repeated(items) = base
    {
        return items
            .iter()
            .enumerate()
            .flat_map(|(index, item)| {
                let mut next = acc.to_vec();
                next.push(collection_frame(
                    item,
                    CollectionIdentity::Repeated {
                        path: prefix.to_vec(),
                        index: index + 1,
                    },
                ));
                walk_source_frames(item, path, prefix, &next)
            })
            .collect();
    }

    match path.split_first() {
        None => match base {
            Instance::DocumentSet(documents) => documents
                .iter()
                .enumerate()
                .map(|(index, document)| {
                    let mut next = acc.to_vec();
                    next.push(collection_frame(
                        document.value(),
                        CollectionIdentity::Document {
                            path: prefix.to_vec(),
                            index: index + 1,
                        },
                    ));
                    next
                })
                .collect(),
            Instance::Repeated(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut next = acc.to_vec();
                    next.push(collection_frame(
                        item,
                        CollectionIdentity::Repeated {
                            path: prefix.to_vec(),
                            index: index + 1,
                        },
                    ));
                    next
                })
                .collect(),
            _ => {
                let mut next = acc.to_vec();
                next.push(ScopeFrame {
                    instance: base,
                    collection: None,
                });
                vec![next]
            }
        },
        Some((segment, rest)) => {
            if let Instance::DocumentSet(documents) = base {
                return documents
                    .iter()
                    .enumerate()
                    .flat_map(|(index, document)| {
                        let mut next = acc.to_vec();
                        next.push(collection_frame(
                            document.value(),
                            CollectionIdentity::Document {
                                path: prefix.to_vec(),
                                index: index + 1,
                            },
                        ));
                        walk_source_frames(document.value(), path, prefix, &next)
                    })
                    .collect();
            }

            let mut collection_path = prefix.to_vec();
            collection_path.push((*segment).to_string());
            match base.field(segment) {
                None => Vec::new(),
                Some(Instance::Repeated(items)) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(index, item)| {
                        let mut next = acc.to_vec();
                        next.push(collection_frame(
                            item,
                            CollectionIdentity::Repeated {
                                path: collection_path.clone(),
                                index: index + 1,
                            },
                        ));
                        if rest.is_empty() {
                            vec![next]
                        } else {
                            walk_source_frames(item, rest, &collection_path, &next)
                        }
                    })
                    .collect(),
                Some(other) => walk_source_frames(other, rest, &collection_path, acc),
            }
        }
    }
}

fn direct_scalar<'a>(source: &'a Instance, path: &[&str]) -> Option<&'a Value> {
    let mut current = source;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}
