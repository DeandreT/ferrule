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

    /// Appends source frames from one host-loaded dynamic document while
    /// retaining the exact driver context used to compute its path.
    pub(crate) fn walk_loaded_source<'b>(
        &'b self,
        source: &str,
        document: &'b Instance,
        tail: &[&str],
    ) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        let prefix = [source.to_string()];
        walk_source_frames(document, tail, &prefix, &[])
            .into_iter()
            .map(|extension| {
                let mut frames = self
                    .frames
                    .iter()
                    .map(|frame| ScopeFrame {
                        instance: frame.instance,
                        collection: frame.collection.clone(),
                        document_path: frame.document_path,
                        join: frame.join,
                        join_position: frame.join_position,
                    })
                    .collect::<Vec<_>>();
                frames.extend(extension);
                ScopeContext {
                    primary_source: self.primary_source,
                    frames,
                    named_inputs: self.named_inputs,
                    execution: self.execution,
                    dynamic_source_loader: self.dynamic_source_loader,
                    filter_map: self.filter_map.clone(),
                }
            })
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

    /// Scans one resolved repeated collection in source order.
    /// Repeated ancestors are flattened left-to-right, while row fields never
    /// fall back to an enclosing source frame.
    pub fn lookup(
        &self,
        collection: &[&str],
        key: &[&str],
        needle: &Value,
        value: &[&str],
    ) -> Result<Value, SourcePathError> {
        let items = self.lookup_items(collection)?;
        Ok(items
            .iter()
            .find_map(|item| {
                direct_scalar(item, key)
                    .is_some_and(|candidate| candidate == needle)
                    .then(|| direct_scalar(item, value).cloned().unwrap_or(Value::Null))
            })
            .unwrap_or(Value::Null))
    }

    fn lookup_items(&self, path: &[&str]) -> Result<Vec<&Instance>, SourcePathError> {
        if let Some((first, rest)) = path.split_first() {
            for frame in self.frames.iter().rev() {
                let Some(base) = frame.instance.field(first) else {
                    continue;
                };
                let mut items = Vec::new();
                visit_lookup_items(base, rest, &mut items);
                return Ok(items);
            }
            if let Some(input) = self.named_input(first) {
                let mut items = Vec::new();
                visit_lookup_items(input, rest, &mut items);
                return Ok(items);
            }
        } else if let Some(Instance::Repeated(items)) = self
            .frames
            .iter()
            .rev()
            .map(|frame| frame.instance)
            .find(|instance| matches!(instance, Instance::Repeated(_)))
        {
            return Ok(items.iter().collect());
        }
        Err(SourcePathError::MissingCollection {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
        })
    }

    fn extend(&self, extension: Vec<ScopeFrame<'a>>) -> Self {
        let mut frames = self.frames.clone();
        frames.extend(extension);
        Self {
            primary_source: self.primary_source,
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
            dynamic_source_loader: self.dynamic_source_loader,
            filter_map: self.filter_map.clone(),
        }
    }
}

fn visit_lookup_items<'a>(current: &'a Instance, path: &[&str], output: &mut Vec<&'a Instance>) {
    if let Instance::Repeated(items) = current {
        if path.is_empty() {
            output.extend(items);
        } else {
            for item in items {
                visit_lookup_items(item, path, output);
            }
        }
        return;
    }
    if let Some((first, rest)) = path.split_first()
        && let Some(next) = current.field(first)
    {
        visit_lookup_items(next, rest, output);
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

// One cursor per active repeated ancestor; never enqueue all source items.
enum OwnedWalkTask<'a> {
    Visit {
        instance: &'a Instance,
        consumed: usize,
        restore: usize,
    },
    Repeated {
        items: &'a [Instance],
        consumed: usize,
        next: usize,
        restore: usize,
    },
}

impl<'a> super::OwnedScopeContext<'a> {
    /// Walks an exact static source owner with canonical endpoint-relative paths.
    /// Every repeated boundary records its actual item and one-based position.
    pub fn walk_owned_source(
        &self,
        owner: super::SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Result<Vec<Self>, super::OwnedSourceError> {
        use super::{OwnedSourceError, OwnedSourceLimitKind};

        let mut budget = self.query_budget(owner, path, None)?;
        let (key, root) = self.resolve_owned_owner(owner)?;
        let base = self.owned_base(key, path);
        if base.is_some_and(|frame| frame.collection.len() == path.len()) {
            super::owned_charge(
                OwnedSourceLimitKind::WalkContexts,
                0,
                1,
                self.registry.limits.max_walk_contexts,
            )?;
            budget.slots.charge(1)?;
            return Ok(vec![Self {
                registry: std::sync::Arc::clone(&self.registry),
                frames: self.copy_owned_frames(&self.frames, &mut budget)?,
            }]);
        }
        let (instance, consumed) =
            base.map_or((root, 0), |frame| (frame.instance, frame.collection.len()));
        let mut frames = self.copy_owned_frames(&self.frames, &mut budget)?;
        budget.slots.charge(1)?; // Current task reservation, retained through the walk.
        budget.slots.charge(1)?; // Initial pending Visit cursor.
        let mut pending = vec![OwnedWalkTask::Visit {
            instance,
            consumed,
            restore: frames.len(),
        }];
        let mut output = Vec::new();
        while let Some(task) = pending.pop() {
            budget.slots.release(1)?;
            match task {
                OwnedWalkTask::Repeated {
                    items,
                    consumed,
                    next,
                    restore,
                } => {
                    for frame in &frames[restore..] {
                        budget.slots.release(1)?;
                        budget.slots.release(frame.collection.len())?;
                    }
                    frames.truncate(restore);
                    let Some(item) = items.get(next) else {
                        continue;
                    };
                    let index = next.checked_add(1).ok_or(
                        OwnedSourceError::MetadataArithmeticOverflow {
                            kind: OwnedSourceLimitKind::WalkContexts,
                        },
                    )?;
                    super::owned_charge(
                        OwnedSourceLimitKind::Frames,
                        frames.len(),
                        1,
                        self.registry.limits.max_active_frames,
                    )?;
                    budget.slots.charge(1)?;
                    budget.slots.charge(consumed)?;
                    budget.borrowed_path(&path[..consumed])?;
                    frames.push(super::OwnedSourceFrame {
                        owner: key,
                        collection: super::owned_string_path(&path[..consumed]),
                        instance: item,
                        index,
                    });
                    budget.slots.charge(2)?;
                    pending.push(OwnedWalkTask::Repeated {
                        items,
                        consumed,
                        next: index,
                        restore,
                    });
                    pending.push(OwnedWalkTask::Visit {
                        instance: item,
                        consumed,
                        restore: frames.len(),
                    });
                }
                OwnedWalkTask::Visit {
                    mut instance,
                    mut consumed,
                    restore,
                } => {
                    for frame in &frames[restore..] {
                        budget.slots.release(1)?;
                        budget.slots.release(frame.collection.len())?;
                    }
                    frames.truncate(restore);
                    loop {
                        match instance {
                            Instance::Scalar(Value::Null | Value::JsonNull(_)) => break,
                            Instance::Repeated(items) => {
                                budget.slots.charge(1)?;
                                pending.push(OwnedWalkTask::Repeated {
                                    items,
                                    consumed,
                                    next: 0,
                                    restore: frames.len(),
                                });
                                break;
                            }
                            Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                                return Err(OwnedSourceError::UnsupportedSourceKind {
                                    owner: self.owned_identity(key),
                                    path: super::owned_string_path(path),
                                    found: super::owned_instance_kind(instance),
                                });
                            }
                            _ => {}
                        }
                        if consumed == path.len() {
                            super::owned_charge(
                                OwnedSourceLimitKind::WalkContexts,
                                output.len(),
                                1,
                                self.registry.limits.max_walk_contexts,
                            )?;
                            budget.slots.charge(1)?;
                            let copied = self.copy_owned_frames(&frames, &mut budget)?;
                            output.push(Self {
                                registry: std::sync::Arc::clone(&self.registry),
                                frames: copied,
                            });
                            break;
                        }
                        match instance {
                            Instance::Group(_) => {
                                let Some(next) = instance.field(path[consumed]) else {
                                    break;
                                };
                                instance = next;
                                consumed += 1;
                            }
                            _ => {
                                return Err(OwnedSourceError::Traversal {
                                    owner: self.owned_identity(key),
                                    source: SourcePathError::CannotTraverse {
                                        path: super::owned_string_path(path),
                                        segment: consumed,
                                        found: super::owned_instance_kind(instance),
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(output)
    }
}
