//! Exact structured element reads for owned XML serialization.
//!
//! The selection rules match owned scalar fields: an explicit frame selects the
//! latest exact same-owner occurrence (or the immutable endpoint for an eligible
//! empty root pin); without a frame, the latest same-owner occurrence prefixing
//! the path is authoritative, otherwise the endpoint root. Absence is reported
//! to the caller and is never repaired from another owner or legacy frame.

use ir::{Instance, Value};
use mapping::{OwnedSourceRef, SourceOwner};

use super::{OccurrenceKind, OwnedContext, OwnedSourceError, OwnerKey};
use crate::resolve::owned_shape;

impl<'a> OwnedContext<'a> {
    /// Returns the selected element, or `None` when the selected owner has no
    /// element at the path.
    pub fn element(
        &self,
        source: &OwnedSourceRef,
        frame: Option<&[String]>,
    ) -> Result<Option<&'a Instance>, OwnedSourceError> {
        let owner_name = match &source.owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        let _work =
            self.roots
                .budget
                .work(owner_name, &source.path, frame, &self.frames, &self.origins)?;
        let key = self.roots.key(&source.owner)?;
        let (base, suffix) = match frame {
            Some(pin) => {
                self.roots.budget.path(pin)?;
                let Some(suffix) = source.path.strip_prefix(pin) else {
                    return Err(OwnedSourceError::UnsupportedFramePathCombination {
                        path: source.path.clone(),
                        frame: pin.to_vec(),
                    });
                };
                let root = self.roots.root(key)?;
                let declared_repeating = match key {
                    OwnerKey::Primary => self.roots.primary_repeating,
                    OwnerKey::Named(index) => self.roots.declared[index].schema.repeating,
                };
                let active = self
                    .frames
                    .iter()
                    .rev()
                    .find(|frame| frame.owner == key && frame.collection == pin);
                if pin.is_empty()
                    && (active.is_some_and(|frame| frame.kind == OccurrenceKind::GroupMembers)
                        || (!declared_repeating && !matches!(root, Instance::Repeated(_))))
                {
                    (root, suffix)
                } else {
                    let active = active.ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                        owner: source.owner.clone(),
                        collection: Some(pin.to_vec()),
                    })?;
                    match representative(active.kind, active.item) {
                        Some(item) => (item, suffix),
                        None => return Ok(None),
                    }
                }
            }
            None => match self
                .frames
                .iter()
                .rev()
                .find(|frame| frame.owner == key && source.path.starts_with(&frame.collection))
            {
                Some(active) => match representative(active.kind, active.item) {
                    Some(item) => (item, &source.path[active.collection.len()..]),
                    None => return Ok(None),
                },
                None => (self.roots.root(key)?, source.path.as_slice()),
            },
        };
        owned_element(base, suffix, &source.path)
    }
}

/// The first member of a group-member occurrence; ordinary items are themselves.
fn representative(kind: OccurrenceKind, item: &Instance) -> Option<&Instance> {
    match (kind, item) {
        (OccurrenceKind::GroupMembers, Instance::Repeated(members)) => members.first(),
        _ => Some(item),
    }
}

/// Follows `suffix` without crossing repetition. Present scalars, groups and
/// XML nil values reach the writer unchanged; an uniterated repetition is never
/// reduced to its first item.
fn owned_element<'a>(
    item: &'a Instance,
    suffix: &[String],
    path: &[String],
) -> Result<Option<&'a Instance>, OwnedSourceError> {
    let mut current = item;
    for segment in suffix {
        match current {
            Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                return Err(OwnedSourceError::UnsupportedSourceDomain {
                    found: owned_shape(current),
                });
            }
            Instance::Repeated(_) => {
                return Err(OwnedSourceError::UniteratedOwnedRepetition {
                    path: path.to_vec(),
                });
            }
            Instance::Scalar(Value::Null | Value::JsonNull(_)) => return Ok(None),
            Instance::Group(fields) => match fields.iter().find(|(name, _)| name == segment) {
                Some((_, next)) => current = next,
                None => return Ok(None),
            },
            Instance::Scalar(_) => {
                return Err(OwnedSourceError::MalformedOwnedTraversal {
                    path: path.to_vec(),
                    found: owned_shape(current),
                });
            }
        }
    }
    match current {
        Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
            Err(OwnedSourceError::UnsupportedSourceDomain {
                found: owned_shape(current),
            })
        }
        Instance::Repeated(_) => Err(OwnedSourceError::UniteratedOwnedRepetition {
            path: path.to_vec(),
        }),
        Instance::Group(_) | Instance::Scalar(_) => Ok(Some(current)),
    }
}
