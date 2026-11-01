use ir::Instance;
use mapping::JoinId;

/// A slot indexes the accompanying instance vector, never the position vector.
/// Walks use extension-relative slots; appending rebases them into the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceBinding {
    /// Compatibility for hand-built contexts and unvalidated malformed joins.
    LegacyUnspecified,
    ContextSlot(usize),
    ExplicitUnbound,
}

impl InstanceBinding {
    pub(crate) fn rebased(self, offset: usize) -> Self {
        match self {
            Self::ContextSlot(slot) => slot
                .checked_add(offset)
                .map_or(Self::ExplicitUnbound, Self::ContextSlot),
            other => other,
        }
    }
}

#[derive(Clone)]
pub(super) struct PositionFrame {
    pub(crate) binding: InstanceBinding,
    /// Immutable ordinal of the exact generated sequence, before selection.
    pub(super) generated_origin: Option<(mapping::NodeId, usize)>,
    pub(super) collection: Vec<String>,
    pub(super) index: usize,
    /// The matching context instance has a synthetic named collection
    /// wrapper immediately before it.
    pub(super) grouped: bool,
    /// Join owning this raw source frame, if any.
    pub(super) join: Option<JoinId>,
    /// Flattened tuple position stored without adding a synthetic context
    /// frame. The raw source `index` remains independently addressable.
    pub(super) join_position: Option<(JoinId, usize)>,
    /// Resolved path retained by a document-set boundary for this source frame.
    pub(super) document_path: Option<String>,
}

pub(super) struct WalkExtension<'a> {
    pub(super) instances: Vec<&'a Instance>,
    pub(super) positions: Vec<PositionFrame>,
}

impl<'a> WalkExtension<'a> {
    pub(crate) fn append_positions_to(
        &self,
        context_offset: usize,
        positions: &mut Vec<PositionFrame>,
    ) {
        positions.extend(self.positions.iter().cloned().map(|mut position| {
            position.binding = match position.binding {
                InstanceBinding::ContextSlot(slot) if slot >= self.instances.len() => {
                    InstanceBinding::ExplicitUnbound
                }
                binding => binding.rebased(context_offset),
            };
            position
        }));
    }

    pub(crate) fn append_to(
        &self,
        context: &mut Vec<&'a Instance>,
        positions: &mut Vec<PositionFrame>,
    ) {
        self.append_positions_to(context.len(), positions);
        context.extend(self.instances.iter().copied());
    }
}

/// Walks `path` from `base`, branching (and pushing one context frame) each
/// time it crosses a repeating element -- whether mid-path or, if `path` is
/// exhausted and the final value is itself repeating (e.g. `path` is empty
/// and `base` is a CSV file's rows), at the very end. Returns one extension
/// (the new frames to push, innermost last) per produced item. Repeating
/// frames also retain their collection path and 1-based source position.
pub(super) fn walk<'a>(
    base: &'a Instance,
    path: &[String],
    prefix: &[String],
    acc: &[&'a Instance],
    positions: &[PositionFrame],
) -> Vec<WalkExtension<'a>> {
    if !path.is_empty()
        && let Instance::Repeated(items) = base
    {
        return items
            .iter()
            .enumerate()
            .flat_map(|(index, item)| {
                let mut next_instances = acc.to_vec();
                next_instances.push(item);
                let mut next_positions = positions.to_vec();
                next_positions.push(PositionFrame {
                    binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                    generated_origin: None,
                    collection: prefix.to_vec(),
                    index: index + 1,
                    grouped: false,
                    join: None,
                    join_position: None,
                    document_path: None,
                });
                walk(item, path, prefix, &next_instances, &next_positions)
            })
            .collect();
    }
    match path.split_first() {
        None => match base {
            Instance::DocumentSet(documents) => documents
                .iter()
                .enumerate()
                .map(|(index, document)| {
                    let mut next_instances = acc.to_vec();
                    next_instances.push(document.value());
                    let mut next_positions = positions.to_vec();
                    next_positions.push(PositionFrame {
                        binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                        generated_origin: None,
                        collection: prefix.to_vec(),
                        index: index + 1,
                        grouped: false,
                        join: None,
                        join_position: None,
                        document_path: Some(document.source_path().to_string()),
                    });
                    WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    }
                })
                .collect(),
            Instance::Repeated(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut next_instances = acc.to_vec();
                    next_instances.push(item);
                    let mut next_positions = positions.to_vec();
                    next_positions.push(PositionFrame {
                        binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                        generated_origin: None,
                        collection: prefix.to_vec(),
                        index: index + 1,
                        grouped: false,
                        join: None,
                        join_position: None,
                        document_path: None,
                    });
                    WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    }
                })
                .collect(),
            _ => {
                let mut next_instances = acc.to_vec();
                if !prefix.is_empty() {
                    next_instances.push(base);
                }
                vec![WalkExtension {
                    // An empty path over a non-collection is an identity
                    // iteration. `base` is already the active context frame;
                    // pushing it again would create a redundant frame.
                    // A non-empty prefix means a named path was consumed to
                    // reach this terminal value, which must remain available
                    // to singleton joins and copy construction.
                    instances: next_instances,
                    positions: positions.to_vec(),
                }]
            }
        },
        Some((segment, rest)) => {
            if let Instance::DocumentSet(documents) = base {
                return documents
                    .iter()
                    .enumerate()
                    .flat_map(|(index, document)| {
                        let mut next_instances = acc.to_vec();
                        next_instances.push(document.value());
                        let mut next_positions = positions.to_vec();
                        next_positions.push(PositionFrame {
                            binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                            generated_origin: None,
                            collection: prefix.to_vec(),
                            index: index + 1,
                            grouped: false,
                            join: None,
                            join_position: None,
                            document_path: Some(document.source_path().to_string()),
                        });
                        walk(
                            document.value(),
                            path,
                            prefix,
                            &next_instances,
                            &next_positions,
                        )
                    })
                    .collect();
            }
            let mut collection_path = prefix.to_vec();
            collection_path.push(segment.clone());
            match base.field(segment) {
                None => Vec::new(),
                Some(Instance::Repeated(items)) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(index, item)| {
                        let mut next_instances = acc.to_vec();
                        next_instances.push(item);
                        let mut next_positions = positions.to_vec();
                        next_positions.push(PositionFrame {
                            binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                            generated_origin: None,
                            collection: collection_path.clone(),
                            index: index + 1,
                            grouped: false,
                            join: None,
                            join_position: None,
                            document_path: None,
                        });
                        if rest.is_empty() {
                            vec![WalkExtension {
                                instances: next_instances,
                                positions: next_positions,
                            }]
                        } else {
                            walk(
                                item,
                                rest,
                                &collection_path,
                                &next_instances,
                                &next_positions,
                            )
                        }
                    })
                    .collect(),
                Some(other) => walk(other, rest, &collection_path, acc, positions),
            }
        }
    }
}

/// Owned-source metadata is separate from instance/position correspondence.
pub(crate) struct ObservedExtension<'a> {
    pub extension: WalkExtension<'a>,
    pub frames: Vec<crate::owned_context::SourceFrame<'a>>,
    pub origins: Vec<crate::owned_context::SourceOrigin<'a>>,
}

pub(crate) fn reserve_observed_copy(
    budget: &crate::owned_context::WorkBudget,
    frames: &[crate::owned_context::SourceFrame<'_>],
    origins: &[crate::owned_context::SourceOrigin<'_>],
    positions: &[PositionFrame],
    instances: usize,
) -> Result<(), crate::OwnedSourceError> {
    let paths = frames
        .iter()
        .map(|frame| frame.collection.as_slice())
        .chain(origins.iter().map(|origin| origin.path.as_slice()))
        .chain(
            positions
                .iter()
                .map(|position| position.collection.as_slice()),
        );
    let mut bytes = 0usize;
    let mut slots = instances
        .checked_add(frames.len())
        .and_then(|n| n.checked_add(origins.len()))
        .and_then(|n| n.checked_add(positions.len()))
        .ok_or_else(|| budget.slot_overflow())?;
    for path in paths {
        budget.path(path)?;
        slots = slots
            .checked_add(path.len())
            .ok_or_else(|| budget.slot_overflow())?;
        bytes = path
            .iter()
            .try_fold(bytes, |sum, part| sum.checked_add(part.len()))
            .ok_or_else(|| budget.overflow())?;
    }
    budget.reserve(slots)?;
    budget.charge(bytes)
}

/// Borrowed selection request; retaining one candidate never narrows traversal.
#[derive(Clone, Copy)]
pub(crate) struct ObservedWalkRequest<'a, 'query> {
    pub base: &'a Instance,
    pub path: &'query [String],
    pub raw_prefix: &'query [String],
    pub canonical_prefix: &'query [String],
    pub owner: Option<crate::owned_context::OwnerKey>,
    pub context: &'query crate::owned_context::OwnedContext<'a>,
    pub position_offset: usize,
    pub strict_collection: bool,
}

pub(crate) struct ObservedReplay<'a, 'query> {
    pub request: ObservedWalkRequest<'a, 'query>,
    pub budget: crate::owned_context::WorkBudget,
}

pub(crate) struct ObservedSelection<'a, 'query> {
    pub extensions: Vec<ObservedExtension<'a>>,
    /// Complete candidate count, independent of the retained vector's length.
    pub candidates: usize,
    pub replay: Option<ObservedReplay<'a, 'query>>,
}

impl<'a> ObservedWalkRequest<'a, '_> {
    pub fn visit(
        self,
        budget: &crate::owned_context::WorkBudget,
        output: &mut impl FnMut(ObservedExtension<'a>) -> Result<(), crate::OwnedSourceError>,
    ) -> Result<(), crate::OwnedSourceError> {
        let identity = self.strict_collection
            && self.path.is_empty()
            && self.owner.is_some_and(|owner| {
                self.context
                    .active_identity(owner, self.canonical_prefix, self.base)
            });
        observe_walk(
            self.base,
            self.path,
            self.raw_prefix,
            self.canonical_prefix,
            self.owner,
            self.position_offset,
            &[],
            &[],
            &self.context.frames,
            &self.context.origins,
            self.strict_collection,
            identity,
            budget,
            output,
        )
    }
}

pub(crate) fn select_observed<'a, 'query>(
    request: ObservedWalkRequest<'a, 'query>,
    budget: &crate::owned_context::WorkBudget,
    retain_first: bool,
) -> Result<ObservedSelection<'a, 'query>, crate::OwnedSourceError> {
    if !retain_first {
        let extensions = walk_observed(
            request.base,
            request.path,
            request.raw_prefix,
            request.canonical_prefix,
            request.owner,
            request.context,
            request.position_offset,
            request.strict_collection,
            budget,
        )?;
        return Ok(ObservedSelection {
            candidates: extensions.len(),
            extensions,
            replay: None,
        });
    }
    // Save the initial ledger, before traversal charges. Only a successful full
    // traversal permits replay of the later no-op filter accounting.
    let replay = retain_first.then(|| ObservedReplay {
        request,
        budget: budget.fork(),
    });
    let mut extensions = Vec::new();
    let mut candidates = 0usize;
    request.visit(budget, &mut |extension| {
        candidates = candidates
            .checked_add(1)
            .ok_or_else(|| budget.slot_overflow())?;
        if !retain_first || extensions.is_empty() {
            extensions.push(extension);
        }
        Ok(())
    })?;
    Ok(ObservedSelection {
        extensions,
        candidates,
        replay,
    })
}

/// Observe each candidate synchronously before retaining or dropping it. The
/// accounting replay deliberately carries no observer, so First emits once.
pub(crate) fn select_observed_with_candidate_observer<'a, 'query>(
    request: ObservedWalkRequest<'a, 'query>,
    budget: &crate::owned_context::WorkBudget,
    retain_first: bool,
    observer: &mut impl FnMut(&mut ObservedExtension<'a>, usize),
) -> Result<ObservedSelection<'a, 'query>, crate::OwnedSourceError> {
    let replay = retain_first.then(|| ObservedReplay {
        request,
        budget: budget.fork(),
    });
    let mut extensions = Vec::new();
    let mut candidates = 0usize;
    request.visit(budget, &mut |mut extension| {
        candidates = candidates
            .checked_add(1)
            .ok_or_else(|| budget.slot_overflow())?;
        observer(&mut extension, candidates);
        if !retain_first || extensions.is_empty() {
            extensions.push(extension);
        }
        Ok(())
    })?;
    Ok(ObservedSelection {
        extensions,
        candidates,
        replay,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_observed<'a>(
    base: &'a Instance,
    path: &[String],
    raw_prefix: &[String],
    canonical_prefix: &[String],
    owner: Option<crate::owned_context::OwnerKey>,
    context: &crate::owned_context::OwnedContext<'a>,
    position_offset: usize,
    strict_collection: bool,
    budget: &crate::owned_context::WorkBudget,
) -> Result<Vec<ObservedExtension<'a>>, crate::OwnedSourceError> {
    let request = ObservedWalkRequest {
        base,
        path,
        raw_prefix,
        canonical_prefix,
        owner,
        context,
        position_offset,
        strict_collection,
    };
    let mut output = Vec::new();
    request.visit(budget, &mut |extension| {
        output.push(extension);
        Ok(())
    })?;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn observe_walk<'a>(
    base: &'a Instance,
    path: &[String],
    raw_prefix: &[String],
    canonical_prefix: &[String],
    owner: Option<crate::owned_context::OwnerKey>,
    position_offset: usize,
    instances: &[&'a Instance],
    positions: &[PositionFrame],
    frames: &[crate::owned_context::SourceFrame<'a>],
    origins: &[crate::owned_context::SourceOrigin<'a>],
    strict: bool,
    identity: bool,
    budget: &crate::owned_context::WorkBudget,
    output: &mut impl FnMut(ObservedExtension<'a>) -> Result<(), crate::OwnedSourceError>,
) -> Result<(), crate::OwnedSourceError> {
    use crate::OwnedSourceError;
    use crate::owned_context::{SourceFrame, SourceOrigin};
    budget.path(canonical_prefix)?;
    budget.path(raw_prefix)?;
    // New owned walks prune absent/null items before identity or terminal publication.
    // The observed legacy walk intentionally retains its original null-item behavior.
    if strict
        && matches!(
            base,
            Instance::Scalar(ir::Value::Null | ir::Value::JsonNull(_))
        )
    {
        return Ok(());
    }
    if identity {
        budget.emit()?;
        reserve_observed_copy(budget, frames, origins, positions, instances.len())?;
        budget.reserve(1)?;
        output(ObservedExtension {
            extension: WalkExtension {
                instances: instances.to_vec(),
                positions: positions.to_vec(),
            },
            frames: frames.to_vec(),
            origins: origins.to_vec(),
        })?;
        return Ok(());
    }
    if let Instance::Repeated(items) = base {
        for (index, item) in items.iter().enumerate() {
            if strict
                && matches!(
                    item,
                    Instance::Scalar(ir::Value::Null | ir::Value::JsonNull(_))
                )
            {
                continue;
            }
            if owner.is_some()
                && (frames.len() >= budget.limits.depth || origins.len() >= budget.limits.depth)
            {
                return Err(OwnedSourceError::ResourceLimit {
                    resource: "frame depth",
                    limit: budget.limits.depth,
                });
            }
            let slot = position_offset
                .checked_add(positions.len())
                .ok_or_else(|| budget.slot_overflow())?;
            let position = index.checked_add(1).ok_or_else(|| budget.slot_overflow())?;
            if path.is_empty() {
                budget.emit()?;
                budget.reserve(1)?;
            }
            reserve_observed_copy(budget, frames, origins, positions, instances.len())?;
            let path_slots = canonical_prefix
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(raw_prefix.len()))
                .and_then(|n| n.checked_add(4))
                .ok_or_else(|| budget.slot_overflow())?;
            budget.reserve(path_slots)?;
            let bytes = canonical_prefix
                .iter()
                .try_fold(0usize, |sum, part| {
                    part.len().checked_mul(2).and_then(|n| sum.checked_add(n))
                })
                .and_then(|sum| {
                    raw_prefix
                        .iter()
                        .try_fold(sum, |sum, part| sum.checked_add(part.len()))
                })
                .ok_or_else(|| budget.overflow())?;
            budget.charge(bytes)?;
            let mut next_instances = instances.to_vec();
            next_instances.push(item);
            let mut next_positions = positions.to_vec();
            next_positions.push(PositionFrame {
                binding: InstanceBinding::ContextSlot(next_instances.len() - 1),
                generated_origin: None,
                collection: raw_prefix.to_vec(),
                index: position,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            });
            let mut next_frames = frames.to_vec();
            let mut next_origins = origins.to_vec();
            if let Some(owner) = owner {
                next_frames.push(SourceFrame {
                    owner,
                    collection: canonical_prefix.to_vec(),
                    item,
                    position_slot: slot,
                    kind: crate::owned_context::OccurrenceKind::Item,
                });
                next_origins.push(SourceOrigin {
                    owner,
                    path: canonical_prefix.to_vec(),
                    item,
                    kind: crate::owned_context::SourceOriginKind::Ordinary,
                });
            }
            if path.is_empty() {
                output(ObservedExtension {
                    extension: WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    },
                    frames: next_frames,
                    origins: next_origins,
                })?;
            } else {
                observe_walk(
                    item,
                    path,
                    raw_prefix,
                    canonical_prefix,
                    owner,
                    position_offset,
                    &next_instances,
                    &next_positions,
                    &next_frames,
                    &next_origins,
                    strict,
                    false,
                    budget,
                    output,
                )?;
            }
        }
        return Ok(());
    }
    if matches!(base, Instance::DocumentSet(_) | Instance::MappedSequence(_)) {
        return Err(OwnedSourceError::UnsupportedSourceDomain {
            found: crate::resolve::owned_shape(base),
        });
    }
    match path.split_first() {
        Some((segment, rest)) => {
            let next = match base {
                Instance::Group(fields) => fields
                    .iter()
                    .find(|(name, _)| name == segment)
                    .map(|(_, value)| value),
                _ if !strict => None,
                _ => {
                    return Err(OwnedSourceError::MalformedOwnedTraversal {
                        path: canonical_prefix.to_vec(),
                        found: crate::resolve::owned_shape(base),
                    });
                }
            };
            if let Some(next) = next {
                let lengths = raw_prefix
                    .len()
                    .checked_add(canonical_prefix.len())
                    .and_then(|n| n.checked_add(2))
                    .ok_or_else(|| budget.slot_overflow())?;
                budget.reserve(lengths)?;
                let copied = raw_prefix
                    .iter()
                    .chain(canonical_prefix)
                    .try_fold(0usize, |sum, part| sum.checked_add(part.len()))
                    .and_then(|bytes| {
                        segment
                            .len()
                            .checked_mul(2)
                            .and_then(|n| bytes.checked_add(n))
                    })
                    .ok_or_else(|| budget.overflow())?;
                budget.charge(copied)?;
                let mut raw = raw_prefix.to_vec();
                raw.push(segment.clone());
                let mut canonical = canonical_prefix.to_vec();
                canonical.push(segment.clone());
                observe_walk(
                    next,
                    rest,
                    &raw,
                    &canonical,
                    owner,
                    position_offset,
                    instances,
                    positions,
                    frames,
                    origins,
                    strict,
                    false,
                    budget,
                    output,
                )?;
            }
        }
        None => {
            // Root [] over a singleton Group is an identity, never a position frame.
            if strict && (!canonical_prefix.is_empty() || !matches!(base, Instance::Group(_))) {
                return Err(OwnedSourceError::MalformedOwnedTraversal {
                    path: canonical_prefix.to_vec(),
                    found: crate::resolve::owned_shape(base),
                });
            }
            let add_origin = !raw_prefix.is_empty()
                && owner.is_some_and(|owner| {
                    !origins.last().is_some_and(|origin| {
                        origin.owner == owner
                            && origin.path == canonical_prefix
                            && std::ptr::eq(origin.item, base)
                    })
                });
            if add_origin && origins.len() >= budget.limits.depth {
                return Err(OwnedSourceError::ResourceLimit {
                    resource: "source provenance depth",
                    limit: budget.limits.depth,
                });
            }
            budget.emit()?;
            budget.reserve(1)?;
            reserve_observed_copy(budget, frames, origins, positions, instances.len())?;
            if add_origin {
                budget.reserve(
                    canonical_prefix
                        .len()
                        .checked_add(2)
                        .ok_or_else(|| budget.slot_overflow())?,
                )?;
                budget.charge(
                    canonical_prefix
                        .iter()
                        .try_fold(0usize, |sum, part| sum.checked_add(part.len()))
                        .ok_or_else(|| budget.overflow())?,
                )?;
            } else if !raw_prefix.is_empty() {
                budget.reserve(1)?;
            }
            let mut instances = instances.to_vec();
            let mut origins = origins.to_vec();
            if !raw_prefix.is_empty() {
                instances.push(base);
            }
            if add_origin && let Some(owner) = owner {
                origins.push(SourceOrigin {
                    owner,
                    path: canonical_prefix.to_vec(),
                    item: base,
                    kind: crate::owned_context::SourceOriginKind::Ordinary,
                });
            }
            output(ObservedExtension {
                extension: WalkExtension {
                    instances,
                    positions: positions.to_vec(),
                },
                frames: frames.to_vec(),
                origins,
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ir::{DocumentMember, Instance, Value};

    use super::walk;

    #[test]
    fn walks_descendants_across_a_document_set_and_retains_paths() {
        let document = |path: &str, value: &str| {
            DocumentMember::new(
                path,
                Instance::Group(
                    (vec![(
                        "items".into(),
                        Instance::Repeated(vec![Instance::Scalar(Value::String(value.into()))]),
                    )])
                    .into(),
                ),
            )
        };
        let Some(first) = document("a.xml", "a") else {
            panic!("valid first document member")
        };
        let Some(second) = document("b.xml", "b") else {
            panic!("valid second document member")
        };
        let source = Instance::DocumentSet(vec![first, second]);

        let walked = walk(&source, &["items".into()], &[], &[], &[]);

        assert_eq!(walked.len(), 2);
        assert_eq!(walked[0].instances.len(), 2);
        assert_eq!(walked[1].instances.len(), 2);
        assert_eq!(walked[0].positions[0].collection, Vec::<String>::new());
        assert_eq!(walked[0].positions[0].index, 1);
        assert_eq!(walked[1].positions[0].index, 2);
        assert_eq!(
            walked[0].positions[0].document_path.as_deref(),
            Some("a.xml")
        );
        assert_eq!(
            walked[1].positions[0].document_path.as_deref(),
            Some("b.xml")
        );
        assert_eq!(walked[0].positions[1].collection, ["items"]);
        assert_eq!(walked[1].positions[1].index, 1);
    }

    #[test]
    fn walks_named_descendants_across_an_unnamed_repeating_root() {
        let child = |value: &str| {
            Instance::Group(
                (vec![(
                    "value".into(),
                    Instance::Scalar(Value::String(value.into())),
                )])
                .into(),
            )
        };
        let parent = |children| {
            Instance::Group((vec![("children".into(), Instance::Repeated(children))]).into())
        };
        let source = Instance::Repeated(vec![
            parent(vec![child("a"), child("b")]),
            parent(vec![child("c")]),
        ]);

        let walked = walk(&source, &["children".into()], &[], &[], &[]);

        assert_eq!(walked.len(), 3);
        assert_eq!(walked[0].instances.len(), 2);
        assert_eq!(walked[0].positions[0].collection, Vec::<String>::new());
        assert_eq!(walked[0].positions[0].index, 1);
        assert_eq!(walked[1].positions[1].collection, ["children"]);
        assert_eq!(walked[1].positions[1].index, 2);
        assert_eq!(walked[2].positions[0].index, 2);
        assert_eq!(walked[2].positions[1].index, 1);
    }
}
