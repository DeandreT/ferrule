use crate::{Instance, Value};

use super::{
    CollectionIdentity, ScopeContext, ScopeFrame, SourcePathError, collection_frame, document_frame,
};

impl<'a> ScopeContext<'a> {
    pub(crate) fn repeated_source(&self, path: &[&str]) -> Option<&'a [Instance]> {
        for frame in self.frames.iter().rev() {
            let mut current = frame.instance;
            let mut found = true;
            for segment in path {
                let Some(next) = current.field(segment) else {
                    found = false;
                    break;
                };
                current = next;
            }
            if found && let Some(items) = current.as_repeated() {
                return Some(items);
            }
        }
        if let Some((name, rest)) = path.split_first()
            && let Some(input) = self.named_input(name)
        {
            let mut current = input;
            for segment in rest {
                current = current.field(segment)?;
            }
            return current.as_repeated();
        }
        None
    }

    /// Produces one child context for every item selected by `path`.
    ///
    /// The path is evaluated from the innermost frame that owns its first
    /// field, falling back to the current frame. An empty path iterates the
    /// current repeated/document value, or selects one ordinary current
    /// value. Repetition crossed at any depth branches in source order.
    pub fn walk_source(&self, path: &[&str]) -> Vec<Self> {
        let base = self.frames.iter().rev().find(|frame| match path.first() {
            Some(first) => frame.instance.field(first).is_some(),
            None => true,
        });
        let extensions = if let Some(base) = base {
            let prefix = if path.is_empty()
                && base
                    .collection
                    .as_ref()
                    .is_some_and(CollectionIdentity::is_grouped)
            {
                base.collection
                    .as_ref()
                    .map(CollectionIdentity::path)
                    .unwrap_or_default()
            } else {
                &[]
            };
            walk_source_frames(base.instance, path, prefix, &[])
        } else if let Some((name, rest)) = path.split_first()
            && let Some(input) = self.named_input(name)
        {
            walk_source_frames(input, rest, &[(*name).to_string()], &[])
        } else if let Some(base) = self.frames.last() {
            walk_source_frames(base.instance, path, &[], &[])
        } else {
            Vec::new()
        };

        extensions
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
        let extensions = if let Some(base) = base {
            let prefix = if path.is_empty()
                && base
                    .collection
                    .as_ref()
                    .is_some_and(CollectionIdentity::is_grouped)
            {
                base.collection
                    .as_ref()
                    .map(CollectionIdentity::path)
                    .unwrap_or_default()
            } else {
                &[]
            };
            walk_source_frames(base.instance, path, prefix, &[])
        } else if let Some((name, rest)) = path.split_first()
            && let Some(input) = self.named_input(name)
        {
            walk_source_frames(input, rest, &[(*name).to_string()], &[])
        } else {
            Vec::new()
        };

        extensions
            .into_iter()
            .map(|extension| self.extend(extension))
            .collect()
    }

    /// Enumerates collection-find candidates with the interpreter's strict
    /// root selection and repeated-only traversal semantics.
    ///
    /// Unlike aggregate traversal, a missing collection is an error and an
    /// empty path can start only at an active [`Instance::Repeated`] frame.
    /// Document sets remain ordinary values here; they are not expanded into
    /// document candidates.
    pub fn collection_find_items(&self, path: &[&str]) -> Result<Vec<Self>, SourcePathError> {
        let base = match path.first() {
            Some(first) => self
                .frames
                .iter()
                .rev()
                .find(|frame| frame.instance.field(first).is_some())
                .map(|frame| (frame.instance, path, Vec::new())),
            None => self
                .frames
                .iter()
                .rev()
                .find(|frame| matches!(frame.instance, Instance::Repeated(_)))
                .map(|frame| (frame.instance, path, Vec::new())),
        }
        .or_else(|| {
            let (name, rest) = path.split_first()?;
            self.named_input(name)
                .map(|input| (input, rest, vec![(*name).to_string()]))
        })
        .ok_or_else(|| SourcePathError::MissingCollection {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
        })?;

        let mut extensions = Vec::new();
        visit_collection_find_frames(base.0, base.1, &base.2, 0, &[], &mut extensions);
        Ok(extensions
            .into_iter()
            .map(|extension| self.extend(extension))
            .collect())
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

    /// Scans one directly resolved repeated collection in source order.
    /// Lookup paths never flatten an intermediate repetition and row fields
    /// never fall back to an enclosing source frame.
    pub fn lookup(
        &self,
        collection: &[&str],
        key: &[&str],
        needle: &Value,
        value: &[&str],
    ) -> Result<Value, SourcePathError> {
        let items = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| direct_repeated(frame.instance, collection))
            .or_else(|| {
                let (name, rest) = collection.split_first()?;
                direct_repeated(self.named_input(name)?, rest)
            })
            .ok_or_else(|| SourcePathError::MissingCollection {
                path: collection
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect(),
            })?;
        Ok(items
            .iter()
            .find(|item| direct_scalar(item, key).is_some_and(|candidate| candidate == needle))
            .and_then(|item| direct_scalar(item, value).cloned())
            .unwrap_or(Value::Null))
    }

    fn extend(&self, extension: Vec<ScopeFrame<'a>>) -> Self {
        let mut frames = self.frames.clone();
        frames.extend(extension);
        Self {
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
        }
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
                    next.push(document_frame(
                        document.value(),
                        CollectionIdentity::Document {
                            path: prefix.to_vec(),
                            index: index + 1,
                        },
                        document.source_path(),
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
                    document_path: None,
                    join: None,
                    join_position: None,
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
                        next.push(document_frame(
                            document.value(),
                            CollectionIdentity::Document {
                                path: prefix.to_vec(),
                                index: index + 1,
                            },
                            document.source_path(),
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

fn visit_collection_find_frames<'a>(
    current: &'a Instance,
    path: &[&str],
    prefix: &[String],
    consumed: usize,
    acc: &[ScopeFrame<'a>],
    output: &mut Vec<Vec<ScopeFrame<'a>>>,
) {
    if let Instance::Repeated(items) = current {
        let mut collection_path = prefix.to_vec();
        collection_path.extend(
            path[..consumed]
                .iter()
                .map(|segment| (*segment).to_string()),
        );
        for (index, item) in items.iter().enumerate() {
            let mut next = acc.to_vec();
            next.push(collection_frame(
                item,
                CollectionIdentity::Repeated {
                    path: collection_path.clone(),
                    index: index + 1,
                },
            ));
            visit_collection_find_frames(item, path, prefix, consumed, &next, output);
        }
        return;
    }

    if let Some(segment) = path.get(consumed) {
        if let Some(next) = current.field(segment) {
            visit_collection_find_frames(next, path, prefix, consumed + 1, acc, output);
        }
        return;
    }

    output.push(acc.to_vec());
}

fn direct_scalar<'a>(source: &'a Instance, path: &[&str]) -> Option<&'a Value> {
    let mut current = source;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

fn direct_repeated<'a>(source: &'a Instance, path: &[&str]) -> Option<&'a [Instance]> {
    let mut current = source;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_repeated()
}
