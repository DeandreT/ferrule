//! Exact structured element reads for owned XML serialization.
//!
//! Selection matches owned scalar fields: an explicit frame selects the latest
//! exact same-owner occurrence (or the immutable endpoint for an eligible empty
//! root pin); otherwise the latest same-owner occurrence prefixing the path is
//! authoritative, then the endpoint root. A missing element is a typed failure
//! and is never repaired from another owner or legacy frame.

use super::super::{
    OwnedOccurrenceKind, OwnedScopeContext, OwnedSourceError, SourceOwnerRef, SourcePathError,
    owned_instance_kind, owned_string_path,
};
use super::BasicScopeContext;
use crate::{Instance, RuntimeError, Value};

impl<'ctx> BasicScopeContext<'ctx> {
    /// Reads the complete element an owned XML serializer writes. Basic emitters
    /// supply the exact declared root's repetition metadata for empty pins.
    pub fn owned_xml_instance(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        declared_nonrepeating: bool,
    ) -> Result<&'ctx Instance, RuntimeError> {
        self.budget(owner, path, frame, false)?;
        self.owned
            .owned_xml_instance(owner, path, frame, declared_nonrepeating)
            .map_err(Into::into)
    }
}

impl<'a> OwnedScopeContext<'a> {
    fn owned_xml_instance(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        declared_nonrepeating: bool,
    ) -> Result<&'a Instance, OwnedSourceError> {
        self.query_budget(owner, path, frame)?;
        let (key, root) = self.resolve_owned_owner(owner)?;
        let missing = || OwnedSourceError::Traversal {
            owner: self.owned_identity(key),
            source: SourcePathError::MissingInstance {
                path: owned_string_path(path),
            },
        };
        let selected = if let Some(frame) = frame {
            if !path.starts_with(frame) {
                return Err(OwnedSourceError::IncompatibleFramePath {
                    owner: self.owned_identity(key),
                    frame: owned_string_path(frame),
                    path: owned_string_path(path),
                });
            }
            let active = self.owned_frame_index(key, Some(frame));
            if frame.is_empty()
                && ((declared_nonrepeating && !matches!(root, Instance::Repeated(_)))
                    || active.is_some_and(|index| {
                        self.frames[index].kind == OwnedOccurrenceKind::GroupMembers
                    }))
            {
                None
            } else {
                let index = active.ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                    owner: self.owned_identity(key),
                    collection: Some(owned_string_path(frame)),
                })?;
                Some(&self.frames[index])
            }
        } else {
            self.owned_base(key, path)
        };
        let (mut current, offset) =
            selected.map_or((root, 0), |frame| (frame.instance, frame.collection.len()));
        if selected.is_some_and(|frame| frame.kind == OwnedOccurrenceKind::GroupMembers) {
            let Instance::Repeated(members) = current else {
                unreachable!("private group-members occurrence owns its member collection")
            };
            current = members.first().ok_or_else(missing)?;
        }
        for segment in offset..=path.len() {
            match current {
                Instance::Repeated(_) => {
                    return Err(OwnedSourceError::UniteratedRepetition {
                        owner: self.owned_identity(key),
                        path: owned_string_path(path),
                        segment,
                    });
                }
                Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                    return Err(OwnedSourceError::UnsupportedSourceKind {
                        owner: self.owned_identity(key),
                        path: owned_string_path(path),
                        found: owned_instance_kind(current),
                    });
                }
                _ => {}
            }
            // Present groups, scalars and XML nil values reach the writer.
            if segment == path.len() {
                return Ok(current);
            }
            match current {
                Instance::Scalar(Value::Null | Value::JsonNull(_)) => return Err(missing()),
                Instance::Group(_) => current = current.field(path[segment]).ok_or_else(missing)?,
                _ => {
                    return Err(OwnedSourceError::Traversal {
                        owner: self.owned_identity(key),
                        source: SourcePathError::CannotTraverse {
                            path: owned_string_path(path),
                            segment,
                            found: owned_instance_kind(current),
                        },
                    });
                }
            }
        }
        unreachable!("owned element resolution returns at the complete path")
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        BasicSourceRoots, DeclaredNamedSourceRoot, Instance, OwnedSourceError, OwnedSourceIdentity,
        OwnedSourceLimits, RuntimeError, SourceOwnerRef, SourcePathError, Value,
    };

    fn group(fields: Vec<(&str, Instance)>) -> Instance {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect::<Vec<_>>()
                .into(),
        )
    }

    fn detail(code: &str) -> Instance {
        group(vec![(
            "Detail",
            group(vec![("code", Instance::Scalar(Value::String(code.into())))]),
        )])
    }

    fn input(header: &str, rows: Vec<Instance>) -> Instance {
        group(vec![
            (
                "Header",
                group(vec![("id", Instance::Scalar(Value::String(header.into())))]),
            ),
            ("Rows", Instance::Repeated(rows)),
        ])
    }

    const LIMITS: OwnedSourceLimits = OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 8,
        max_metadata_bytes: 4096,
        max_walk_contexts: 64,
    };

    fn missing(owner: OwnedSourceIdentity, path: &[&str]) -> RuntimeError {
        RuntimeError::OwnedSource(OwnedSourceError::Traversal {
            owner,
            source: SourcePathError::MissingInstance {
                path: path.iter().map(|segment| (*segment).to_string()).collect(),
            },
        })
    }

    #[test]
    fn owned_elements_come_only_from_the_exact_owner_and_frame() {
        let primary = input("p", vec![detail("a"), group(vec![])]);
        let alias = input("h", vec![]);
        let declared = [DeclaredNamedSourceRoot {
            name: "Header",
            instance: Some(&alias),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, LIMITS).unwrap();
        let root = roots.root_context().unwrap();
        let header = |owner| root.owned_xml_instance(owner, &["Header"], None, false);
        assert_eq!(
            header(SourceOwnerRef::Primary).unwrap().field("id"),
            Some(&Instance::Scalar(Value::String("p".into())))
        );
        assert_eq!(
            header(SourceOwnerRef::Named("Header")).unwrap().field("id"),
            Some(&Instance::Scalar(Value::String("h".into())))
        );
        // The root never reduces an uniterated repetition to its first item.
        assert_eq!(
            root.owned_xml_instance(SourceOwnerRef::Primary, &["Rows", "Detail"], None, false),
            Err(RuntimeError::OwnedSource(
                OwnedSourceError::UniteratedRepetition {
                    owner: OwnedSourceIdentity::Primary,
                    path: vec!["Rows".into(), "Detail".into()],
                    segment: 1,
                }
            ))
        );

        let rows = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let read = |index: usize, frame: Option<&[&str]>| {
            rows[index].context().owned_xml_instance(
                SourceOwnerRef::Primary,
                &["Rows", "Detail"],
                frame,
                false,
            )
        };
        assert_eq!(
            read(0, Some(&["Rows"])).unwrap(),
            &detail("a").field("Detail").cloned().unwrap()
        );
        assert_eq!(read(0, None), read(0, Some(&["Rows"])));
        // The second row's absent element never borrows the first row's.
        let absent = || missing(OwnedSourceIdentity::Primary, &["Rows", "Detail"]);
        assert_eq!(read(1, Some(&["Rows"])), Err(absent()));
        assert_eq!(read(1, None), Err(absent()));
        // An explicit root pin reads the declared nonrepeating endpoint root.
        assert_eq!(
            rows[1]
                .context()
                .owned_xml_instance(SourceOwnerRef::Primary, &["Header"], Some(&[]), true)
                .unwrap()
                .field("id"),
            Some(&Instance::Scalar(Value::String("p".into())))
        );
        assert!(matches!(
            rows[1].context().owned_xml_instance(
                SourceOwnerRef::Named("Header"),
                &["Rows", "Detail"],
                Some(&["Rows"]),
                false,
            ),
            Err(RuntimeError::OwnedSource(
                OwnedSourceError::InactiveSourceFrame { .. }
            ))
        ));

        let missing_alias = input("p", vec![]);
        let declared = [DeclaredNamedSourceRoot {
            name: "Header",
            instance: Some(&missing_alias),
        }];
        let empty = group(vec![]);
        let roots = BasicSourceRoots::try_new(&empty, &declared, LIMITS).unwrap();
        assert_eq!(
            roots.root_context().unwrap().owned_xml_instance(
                SourceOwnerRef::Primary,
                &["Header"],
                None,
                false,
            ),
            Err(missing(OwnedSourceIdentity::Primary, &["Header"]))
        );
    }
}
