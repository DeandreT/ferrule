//! Exact owned runtime-named property reads.
//!
//! The open object is selected exactly like an owned scalar field: from one
//! declared endpoint, through its latest same-owner frame or the explicit
//! root pin, with no outward fallback. Only the final property lookup keeps
//! the legacy dynamic-field results: an absent or structural property is
//! `Null`, and a present JSON null or XML nil stays distinct.
use ir::{Instance, Value};
use mapping::{OwnedDynamicSourceField, SourceOwner};

use crate::owned_context::{OccurrenceKind, OwnedContext, OwnedSourceError};

impl OwnedContext<'_> {
    pub fn dynamic_field(
        &self,
        field: &OwnedDynamicSourceField,
        key: &str,
    ) -> Result<Value, OwnedSourceError> {
        let object = &field.object;
        let owner_name = match &object.owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        let _work = self.roots.budget.work(
            owner_name,
            &object.path,
            field.frame.as_deref(),
            &self.frames,
            &self.origins,
        )?;
        let owner = self.roots.key(&object.owner)?;
        fn representative(kind: OccurrenceKind, item: &Instance) -> Option<&Instance> {
            match (kind, item) {
                (OccurrenceKind::GroupMembers, Instance::Repeated(members)) => members.first(),
                (_, item) => Some(item),
            }
        }
        // Selection mirrors `OwnedContext::field`, with the object path in
        // place of the scalar path.
        let (item, suffix) =
            match field.frame.as_deref() {
                Some(pin) => {
                    self.roots.budget.path(pin)?;
                    let Some(suffix) = object.path.strip_prefix(pin) else {
                        return Err(OwnedSourceError::UnsupportedFramePathCombination {
                            path: object.path.clone(),
                            frame: pin.to_vec(),
                        });
                    };
                    let root = self.endpoint_root(owner)?;
                    let frame = self
                        .frames
                        .iter()
                        .rev()
                        .find(|frame| frame.owner == owner && frame.collection == pin);
                    if pin.is_empty()
                        && (frame.is_some_and(|frame| frame.kind == OccurrenceKind::GroupMembers)
                            || (!self.roots.declared_repeating(owner)
                                && !matches!(root, Instance::Repeated(_))))
                    {
                        (root, suffix)
                    } else {
                        let frame = frame.ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                            owner: object.owner.clone(),
                            collection: Some(pin.to_vec()),
                        })?;
                        let Some(item) = representative(frame.kind, frame.item) else {
                            return Ok(Value::Null);
                        };
                        (item, suffix)
                    }
                }
                None => match self.frames.iter().rev().find(|frame| {
                    frame.owner == owner && object.path.starts_with(&frame.collection)
                }) {
                    Some(frame) => {
                        let Some(item) = representative(frame.kind, frame.item) else {
                            return Ok(Value::Null);
                        };
                        (item, &object.path[frame.collection.len()..])
                    }
                    None => (self.endpoint_root(owner)?, object.path.as_slice()),
                },
            };
        let Some(object) = owned_object(item, suffix, &object.path)? else {
            return Ok(Value::Null);
        };
        Ok(object
            .field(key)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null))
    }
}

/// Strict owned traversal to the selected open object. Absent groups and null
/// scalars select no object; crossing a repetition is never repaired by its
/// first item.
fn owned_object<'a>(
    item: &'a Instance,
    suffix: &[String],
    path: &[String],
) -> Result<Option<&'a Instance>, OwnedSourceError> {
    let mut current = item;
    for segment in suffix.iter().map(Some).chain([None]) {
        match current {
            Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                return Err(OwnedSourceError::UnsupportedSourceDomain {
                    found: crate::resolve::owned_shape(current),
                });
            }
            Instance::Repeated(_) => {
                return Err(OwnedSourceError::UniteratedOwnedRepetition {
                    path: path.to_vec(),
                });
            }
            Instance::Scalar(Value::Null | Value::JsonNull(_)) => return Ok(None),
            Instance::Group(fields) => {
                let Some(segment) = segment else {
                    return Ok(Some(current));
                };
                match fields.iter().find(|(name, _)| name == segment) {
                    Some((_, next)) => current = next,
                    None => return Ok(None),
                }
            }
            Instance::Scalar(_) => {
                return Err(OwnedSourceError::MalformedOwnedTraversal {
                    path: path.to_vec(),
                    found: crate::resolve::owned_shape(current),
                });
            }
        }
    }
    unreachable!("owned object traversal returns at the complete path")
}
