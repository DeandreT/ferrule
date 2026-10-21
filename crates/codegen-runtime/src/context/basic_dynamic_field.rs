//! Exact owned runtime-named property reads in the paired Basic context.
//!
//! The open object is selected like an owned scalar field: one owner, its
//! latest same-owner frame or the explicit root pin, and no outward fallback.
//! Only the property lookup keeps the legacy dynamic results: absent and
//! structural properties are Null and a present JSON null stays distinct.

use super::super::{OwnedOccurrenceKind, OwnedSourceError, SourceOwnerRef, SourcePathError};
use super::BasicScopeContext;
use crate::{Instance, RuntimeError, Value};

impl BasicScopeContext<'_> {
    /// Reads `key` from the selected owner's open object. The caller evaluates
    /// the key first; a non-string key reads nothing. `declared_nonrepeating`
    /// is the emitter's proof for the owner's root, used by an empty pin.
    pub fn owned_dynamic_field(
        &self,
        owner: SourceOwnerRef<'_>,
        object: &[&str],
        frame: Option<&[&str]>,
        key: &Value,
        declared_nonrepeating: bool,
    ) -> Result<Value, RuntimeError> {
        let Value::String(key) = key else {
            return Ok(Value::Null);
        };
        self.budget(owner, object, frame, false)?;
        self.owned
            .owned_dynamic_field(owner, object, frame, key, declared_nonrepeating)
            .map_err(Into::into)
    }
}

impl super::super::OwnedScopeContext<'_> {
    fn owned_dynamic_field(
        &self,
        owner: SourceOwnerRef<'_>,
        object: &[&str],
        frame: Option<&[&str]>,
        key: &str,
        declared_nonrepeating: bool,
    ) -> Result<Value, OwnedSourceError> {
        self.query_budget(owner, object, frame)?;
        let (owner_key, root) = self.resolve_owned_owner(owner)?;
        // Selection mirrors `owned_field_with_root_pin` with the object path.
        let selected = if let Some(frame) = frame {
            if !object.starts_with(frame) {
                return Err(OwnedSourceError::IncompatibleFramePath {
                    owner: self.owned_identity(owner_key),
                    frame: super::super::owned_string_path(frame),
                    path: super::super::owned_string_path(object),
                });
            }
            let active = self.owned_frame_index(owner_key, Some(frame));
            if frame.is_empty()
                && ((declared_nonrepeating && !matches!(root, Instance::Repeated(_)))
                    || active.is_some_and(|index| {
                        self.frames[index].kind == OwnedOccurrenceKind::GroupMembers
                    }))
            {
                None
            } else {
                let index = active.ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                    owner: self.owned_identity(owner_key),
                    collection: Some(super::super::owned_string_path(frame)),
                })?;
                Some(&self.frames[index])
            }
        } else {
            self.owned_base(owner_key, object)
        };
        let (mut current, offset) =
            selected.map_or((root, 0), |frame| (frame.instance, frame.collection.len()));
        if selected.is_some_and(|frame| frame.kind == OwnedOccurrenceKind::GroupMembers) {
            let Instance::Repeated(members) = current else {
                unreachable!("private group-members occurrence owns its member collection")
            };
            let Some(first) = members.first() else {
                return Ok(Value::Null);
            };
            current = first;
        }
        for segment in offset..=object.len() {
            match current {
                Instance::Repeated(_) => {
                    return Err(OwnedSourceError::UniteratedRepetition {
                        owner: self.owned_identity(owner_key),
                        path: super::super::owned_string_path(object),
                        segment,
                    });
                }
                Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                    return Err(OwnedSourceError::UnsupportedSourceKind {
                        owner: self.owned_identity(owner_key),
                        path: super::super::owned_string_path(object),
                        found: super::super::owned_instance_kind(current),
                    });
                }
                Instance::Scalar(Value::Null | Value::JsonNull(_)) => return Ok(Value::Null),
                Instance::Group(_) if segment == object.len() => {
                    return Ok(current
                        .field(key)
                        .and_then(Instance::as_scalar)
                        .cloned()
                        .unwrap_or(Value::Null));
                }
                Instance::Group(_) => {
                    let Some(next) = current.field(object[segment]) else {
                        return Ok(Value::Null);
                    };
                    current = next;
                }
                Instance::Scalar(_) => {
                    return Err(OwnedSourceError::Traversal {
                        owner: self.owned_identity(owner_key),
                        source: SourcePathError::CannotTraverse {
                            path: super::super::owned_string_path(object),
                            segment,
                            found: super::super::owned_instance_kind(current),
                        },
                    });
                }
            }
        }
        unreachable!("owned object resolution returns at the complete path")
    }
}

#[cfg(test)]
#[path = "basic_dynamic_field_tests.rs"]
mod tests;
