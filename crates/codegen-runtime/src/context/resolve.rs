use std::fmt;

use crate::{Instance, Value};

use super::{ScopeContext, has_prefix, has_suffix, same_path};

/// The structural kind encountered while resolving a scalar source path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceKind {
    Scalar,
    Group,
    Repeated,
    DocumentSet,
    MappedSequence,
}

impl InstanceKind {
    fn of(instance: &Instance) -> Self {
        match instance {
            Instance::Scalar(_) => Self::Scalar,
            Instance::Group(_) => Self::Group,
            Instance::Repeated(_) => Self::Repeated,
            Instance::DocumentSet(_) => Self::DocumentSet,
            Instance::MappedSequence(_) => Self::MappedSequence,
        }
    }
}

impl fmt::Display for InstanceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Scalar => "scalar",
            Self::Group => "group",
            Self::Repeated => "repeated value",
            Self::DocumentSet => "document set",
            Self::MappedSequence => "mapped sequence",
        })
    }
}

/// Failure to resolve a generated mapping's static scalar source path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourcePathError {
    /// A lookup collection path was absent or did not end in a repetition.
    MissingCollection { path: Vec<String> },
    /// A frame-pinned source field was evaluated without its collection active.
    MissingFrame {
        frame: Vec<String>,
        path: Vec<String>,
    },
    /// A named field was absent from the current group or first document.
    MissingField {
        path: Vec<String>,
        segment: usize,
        field: String,
    },
    /// A path segment attempted to traverse a scalar or unsupported sequence.
    CannotTraverse {
        path: Vec<String>,
        segment: usize,
        found: InstanceKind,
    },
    /// The complete path selected a structural value instead of a scalar.
    ExpectedScalar {
        path: Vec<String>,
        found: InstanceKind,
    },
}

impl fmt::Display for SourcePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCollection { path } => write!(
                formatter,
                "source collection {} does not exist in the active scope context",
                display_path(path)
            ),
            Self::MissingFrame { frame, path } => write!(
                formatter,
                "source frame {} is not active while resolving {}",
                display_path(frame),
                display_path(path)
            ),
            Self::MissingField {
                path,
                segment,
                field,
            } => write!(
                formatter,
                "source path {} is missing field {field:?} at segment {segment}",
                display_path(path)
            ),
            Self::CannotTraverse {
                path,
                segment,
                found,
            } => write!(
                formatter,
                "source path {} cannot traverse {found} at segment {segment}",
                display_path(path)
            ),
            Self::ExpectedScalar { path, found } => write!(
                formatter,
                "source path {} resolved to {found}, expected scalar",
                display_path(path)
            ),
        }
    }
}

impl std::error::Error for SourcePathError {}

impl ScopeContext<'_> {
    /// Resolves a scalar using active collection identity before ordinary
    /// innermost-to-outermost fallback.
    ///
    /// Uniterated repetitions contribute their first item. An empty
    /// repetition resolves to `Null` immediately and therefore shadows an
    /// outer field with the same path.
    pub fn resolve_scalar(&self, path: &[&str]) -> Result<Value, SourcePathError> {
        let owned_path = owned_path(path);
        let mut first_error = None;

        for frame in self.frames.iter().rev() {
            let Some(collection) = &frame.collection else {
                continue;
            };
            let prefix = collection.path();
            if prefix.is_empty() || !has_prefix(path, prefix) {
                continue;
            }
            match resolve_scalar_in(
                frame.instance,
                &path[prefix.len()..],
                &owned_path,
                prefix.len(),
            ) {
                Ok(value) => return Ok(value),
                Err(error) => first_error.get_or_insert(error),
            };
        }

        for frame in self.frames.iter().rev() {
            match resolve_scalar_in(frame.instance, path, &owned_path, 0) {
                Ok(value) => return Ok(value),
                Err(error) => first_error.get_or_insert(error),
            };
        }

        Err(first_error.unwrap_or(SourcePathError::ExpectedScalar {
            path: owned_path,
            found: InstanceKind::Group,
        }))
    }

    /// Resolves `path` only against the innermost active collection matching
    /// the absolute `frame` path.
    ///
    /// Nested scopes can retain collection paths relative to their parent, so
    /// an absolute frame also matches a non-empty active suffix. Unlike
    /// [`Self::resolve_scalar`], a pinned lookup never falls back to another
    /// source frame.
    pub fn resolve_scalar_in_frame(
        &self,
        frame: &[&str],
        path: &[&str],
    ) -> Result<Value, SourcePathError> {
        let mut absolute_path = owned_path(frame);
        absolute_path.extend(path.iter().map(|segment| (*segment).to_string()));
        let Some(owner) = self.frames.iter().rev().find(|scope_frame| {
            scope_frame.collection.as_ref().is_some_and(|collection| {
                same_path(frame, collection.path())
                    || !collection.path().is_empty() && has_suffix(frame, collection.path())
            })
        }) else {
            return Err(SourcePathError::MissingFrame {
                frame: owned_path(frame),
                path: owned_path(path),
            });
        };

        resolve_scalar_in(owner.instance, path, &absolute_path, frame.len())
    }
}

/// Resolves one scalar without outward context fallback or scalar coercion.
///
/// Every uniterated [`Instance::Repeated`] in the path contributes its first
/// item. [`Instance::DocumentSet`] traversal remains transparent through
/// [`Instance::field`], which selects its first document. An empty path is
/// valid only when `source` itself is scalar.
pub fn resolve_scalar(source: &Instance, path: &[&str]) -> Result<Value, SourcePathError> {
    ScopeContext::new(source).resolve_scalar(path)
}

/// Resolves and clones one scalar value for independent target ownership.
pub fn clone_scalar(source: &Instance, path: &[&str]) -> Result<Value, SourcePathError> {
    resolve_scalar(source, path)
}

fn first_repeated(instance: &Instance) -> Option<&Instance> {
    match instance {
        Instance::Repeated(items) => items.first(),
        _ => Some(instance),
    }
}

fn resolve_scalar_in(
    source: &Instance,
    path: &[&str],
    owned_path: &[String],
    segment_offset: usize,
) -> Result<Value, SourcePathError> {
    let mut current = source;
    for (segment, field_name) in path.iter().enumerate() {
        let Some(next) = first_repeated(current) else {
            return Ok(Value::Null);
        };
        current = next;
        current = current.field(field_name).ok_or_else(|| {
            let found = InstanceKind::of(current);
            if matches!(found, InstanceKind::Group | InstanceKind::DocumentSet) {
                SourcePathError::MissingField {
                    path: owned_path.to_vec(),
                    segment: segment_offset + segment,
                    field: field_name.to_string(),
                }
            } else {
                SourcePathError::CannotTraverse {
                    path: owned_path.to_vec(),
                    segment: segment_offset + segment,
                    found,
                }
            }
        })?;
    }

    let Some(current) = first_repeated(current) else {
        return Ok(Value::Null);
    };
    current
        .as_scalar()
        .cloned()
        .ok_or_else(|| SourcePathError::ExpectedScalar {
            path: owned_path.to_vec(),
            found: InstanceKind::of(current),
        })
}

fn owned_path(path: &[&str]) -> Vec<String> {
    path.iter().map(|segment| (*segment).to_string()).collect()
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
    }
}
