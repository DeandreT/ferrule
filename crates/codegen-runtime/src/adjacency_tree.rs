use std::collections::BTreeMap;

use crate::{
    BasicScopeContext, Instance, MAX_GENERATED_SEQUENCE_ITEMS, MAX_RECURSIVE_SEQUENCE_DEPTH,
    RuntimeError, ScopeContext, SourceOwnerRef, Value,
};

pub type AdjacencyTreeRoot = for<'a> fn(&ScopeContext<'a>) -> Result<Value, RuntimeError>;

/// A root expression evaluated once in the receiving owned (parent) context.
pub type OwnedAdjacencyTreeRoot = for<'a> fn(&BasicScopeContext<'a>) -> Result<Value, RuntimeError>;

pub struct AdjacencyTreeFields<'a> {
    pub collection: &'a [&'a str],
    pub key: &'a [&'a str],
    pub parent: &'a [&'a str],
    pub target_key: &'a str,
    pub target_children: &'a str,
}

struct Row {
    key: String,
}

/// Builds one recursive target group from a flat string-keyed adjacency list.
pub fn adjacency_tree(
    context: &ScopeContext<'_>,
    fields: AdjacencyTreeFields<'_>,
    root: Option<AdjacencyTreeRoot>,
) -> Result<Instance, RuntimeError> {
    let collection = context.repeated_source(fields.collection).ok_or_else(|| {
        RuntimeError::MissingAdjacencyCollection {
            path: fields.collection.join("/"),
        }
    })?;
    build(collection, &fields, || root.map(|root| root(context)))
}

/// Builds the same tree from one exact owner's rows. A same-owner frame pins
/// its prefix; other owners and legacy frames never supply rows. Key and parent
/// stay row-relative, and the root is evaluated in the receiving context.
pub fn owned_adjacency_tree(
    context: &BasicScopeContext<'_>,
    owner: SourceOwnerRef<'_>,
    fields: AdjacencyTreeFields<'_>,
    root: Option<OwnedAdjacencyTreeRoot>,
) -> Result<Instance, RuntimeError> {
    let candidates = context.walk_owned_source(owner, fields.collection)?;
    let rows = candidates
        .iter()
        .filter_map(|candidate| candidate.context().legacy_view().current_instance())
        .collect::<Vec<_>>();
    build(rows, &fields, || root.map(|root| root(context)))
}

/// Shared by legacy and owned rows: source order, limits and error precedence
/// (rows, then root, then root count and recursion) are identical.
fn build<'a, I>(
    collection: I,
    fields: &AdjacencyTreeFields<'_>,
    root: impl FnOnce() -> Option<Result<Value, RuntimeError>>,
) -> Result<Instance, RuntimeError>
where
    I: IntoIterator<Item = &'a Instance>,
    I::IntoIter: ExactSizeIterator,
{
    let AdjacencyTreeFields {
        key,
        parent,
        target_key,
        target_children,
        ..
    } = *fields;
    let collection = collection.into_iter();
    if collection.len() as u128 > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(RuntimeError::AdjacencyTreeTooLarge {
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }

    let mut rows = Vec::with_capacity(collection.len());
    let mut by_key = BTreeMap::new();
    let mut by_parent: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (index, instance) in collection.enumerate() {
        let key = string_field(instance, key, "key")?;
        if by_key.insert(key.clone(), index).is_some() {
            return Err(RuntimeError::DuplicateAdjacencyKey { key });
        }
        let parent = optional_string_field(instance, parent, "parent")?;
        by_parent.entry(parent).or_default().push(index);
        rows.push(Row { key });
    }

    let selected_root = match root().transpose()? {
        Some(Value::Null | Value::JsonNull(_)) | None => None,
        Some(Value::String(value)) => Some(value),
        Some(value) => {
            return Err(RuntimeError::InvalidAdjacencyRoot {
                found: value.type_name(),
            });
        }
    };
    let roots = by_parent
        .get(&selected_root)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if roots.len() != 1 {
        return Err(RuntimeError::AdjacencyRootCount { count: roots.len() });
    }
    build_row(
        roots[0],
        &rows,
        &by_parent,
        target_key,
        target_children,
        0,
        &mut Vec::new(),
    )
}

fn build_row(
    index: usize,
    rows: &[Row],
    by_parent: &BTreeMap<Option<String>, Vec<usize>>,
    target_key: &str,
    target_children: &str,
    depth: usize,
    active: &mut Vec<usize>,
) -> Result<Instance, RuntimeError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(RuntimeError::AdjacencyTreeDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    if active.contains(&index) {
        return Err(RuntimeError::AdjacencyCycle {
            key: rows[index].key.clone(),
        });
    }
    active.push(index);
    let row = &rows[index];
    let child_indices = by_parent
        .get(&Some(row.key.clone()))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let children = child_indices
        .iter()
        .map(|child| {
            build_row(
                *child,
                rows,
                by_parent,
                target_key,
                target_children,
                depth + 1,
                active,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    active.pop();
    Ok(Instance::Group(
        (vec![
            (
                target_key.to_string(),
                Instance::Scalar(Value::String(row.key.clone())),
            ),
            (target_children.to_string(), Instance::Repeated(children)),
        ])
        .into(),
    ))
}

fn string_field(
    instance: &Instance,
    path: &[&str],
    role: &'static str,
) -> Result<String, RuntimeError> {
    match field_scalar(instance, path) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(value) => Err(RuntimeError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: value.type_name(),
        }),
        None => Err(RuntimeError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: "missing value",
        }),
    }
}

fn optional_string_field(
    instance: &Instance,
    path: &[&str],
    role: &'static str,
) -> Result<Option<String>, RuntimeError> {
    match field_scalar(instance, path) {
        Some(Value::Null | Value::JsonNull(_)) | None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(value) => Err(RuntimeError::InvalidAdjacencyField {
            role,
            path: path.join("/"),
            found: value.type_name(),
        }),
    }
}

fn field_scalar<'a>(instance: &'a Instance, path: &[&str]) -> Option<&'a Value> {
    let mut current = instance;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(values: &[(&str, Option<&str>)]) -> Instance {
        Instance::Group(
            (vec![(
                "Rows".to_string(),
                Instance::Repeated(
                    values
                        .iter()
                        .map(|(key, parent)| {
                            Instance::Group(
                                (vec![
                                    (
                                        "Key".to_string(),
                                        Instance::Scalar(Value::String((*key).to_string())),
                                    ),
                                    (
                                        "Parent".to_string(),
                                        Instance::Scalar(
                                            parent
                                                .map(|parent| Value::String(parent.to_string()))
                                                .unwrap_or(Value::Null),
                                        ),
                                    ),
                                ])
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )])
            .into(),
        )
    }

    #[test]
    fn preserves_reachable_source_order_and_omits_unreachable_cycles() {
        let source = rows(&[
            ("Root", None),
            ("Beta", Some("Root")),
            ("Alpha", Some("Root")),
            ("Leaf", Some("Beta")),
            ("Unreachable", Some("Unreachable")),
        ]);
        let actual = adjacency_tree(
            &ScopeContext::new(&source),
            AdjacencyTreeFields {
                collection: &["Rows"],
                key: &["Key"],
                parent: &["Parent"],
                target_key: "name",
                target_children: "children",
            },
            None,
        );
        assert_eq!(
            actual,
            Ok(Instance::Group(
                (vec![
                    (
                        "name".to_string(),
                        Instance::Scalar(Value::String("Root".to_string())),
                    ),
                    (
                        "children".to_string(),
                        Instance::Repeated(vec![
                            Instance::Group(
                                (vec![
                                    (
                                        "name".to_string(),
                                        Instance::Scalar(Value::String("Beta".to_string())),
                                    ),
                                    (
                                        "children".to_string(),
                                        Instance::Repeated(vec![Instance::Group(
                                            (vec![
                                                (
                                                    "name".to_string(),
                                                    Instance::Scalar(Value::String(
                                                        "Leaf".to_string()
                                                    )),
                                                ),
                                                (
                                                    "children".to_string(),
                                                    Instance::Repeated(Vec::new())
                                                ),
                                            ])
                                            .into()
                                        )]),
                                    ),
                                ])
                                .into()
                            ),
                            Instance::Group(
                                (vec![
                                    (
                                        "name".to_string(),
                                        Instance::Scalar(Value::String("Alpha".to_string())),
                                    ),
                                    ("children".to_string(), Instance::Repeated(Vec::new())),
                                ])
                                .into()
                            ),
                        ]),
                    ),
                ])
                .into()
            ))
        );
    }

    #[test]
    fn reports_duplicate_keys_and_reachable_cycles() {
        let duplicate = rows(&[("Root", None), ("Root", Some("Root"))]);
        assert_eq!(
            adjacency_tree(
                &ScopeContext::new(&duplicate),
                AdjacencyTreeFields {
                    collection: &["Rows"],
                    key: &["Key"],
                    parent: &["Parent"],
                    target_key: "name",
                    target_children: "children",
                },
                None,
            ),
            Err(RuntimeError::DuplicateAdjacencyKey { key: "Root".into() })
        );

        fn loop_root(_: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
            Ok(Value::String("Loop".into()))
        }
        let cycle = rows(&[("Loop", Some("Loop"))]);
        assert_eq!(
            adjacency_tree(
                &ScopeContext::new(&cycle),
                AdjacencyTreeFields {
                    collection: &["Rows"],
                    key: &["Key"],
                    parent: &["Parent"],
                    target_key: "name",
                    target_children: "children",
                },
                Some(loop_root),
            ),
            Err(RuntimeError::AdjacencyCycle { key: "Loop".into() })
        );
    }

    fn owned_rows(root: &str, rows: &[(&str, Option<&str>)]) -> Instance {
        let Instance::Group(fields) = self::rows(rows) else {
            unreachable!("rows are a group");
        };
        let mut fields = fields.into_iter().collect::<Vec<_>>();
        fields.push((
            "Root".to_string(),
            Instance::Scalar(Value::String(root.into())),
        ));
        Instance::Group(fields.into())
    }

    fn owned(
        context: &crate::BasicScopeContext<'_>,
        owner: SourceOwnerRef<'_>,
        root: Option<OwnedAdjacencyTreeRoot>,
    ) -> Result<Instance, RuntimeError> {
        owned_adjacency_tree(
            context,
            owner,
            AdjacencyTreeFields {
                collection: &["Rows"],
                key: &["Key"],
                parent: &["Parent"],
                target_key: "name",
                target_children: "children",
            },
            root,
        )
    }

    fn name(instance: &Instance) -> Option<&Value> {
        instance.field("name").and_then(Instance::as_scalar)
    }

    #[test]
    fn owned_rows_read_only_their_exact_endpoint_and_root_reads_the_parent() {
        let limits = crate::OwnedSourceLimits {
            max_roots: 8,
            max_active_frames: 8,
            max_path_segments: 8,
            max_metadata_bytes: 4096,
            max_walk_contexts: 64,
        };
        let primary = owned_rows("n1", &[("p", None)]);
        let audit = owned_rows("-", &[("n", None), ("n1", Some("n")), ("n2", Some("n1"))]);
        let alias = owned_rows("-", &[("q", None)]);
        let declared = [
            crate::DeclaredNamedSourceRoot {
                name: "audit",
                instance: Some(&audit),
            },
            crate::DeclaredNamedSourceRoot {
                name: "Rows",
                instance: Some(&alias),
            },
            crate::DeclaredNamedSourceRoot {
                name: "missing",
                instance: None,
            },
        ];
        let roots = crate::BasicSourceRoots::try_new(&primary, &declared, limits).unwrap();
        let context = roots.root_context().unwrap();
        let tree = owned(&context, SourceOwnerRef::Primary, None).unwrap();
        assert_eq!(name(&tree), Some(&Value::String("p".into())));
        let named = owned(&context, SourceOwnerRef::Named("audit"), None).unwrap();
        assert_eq!(name(&named), Some(&Value::String("n".into())));
        let aliased = owned(&context, SourceOwnerRef::Named("Rows"), None).unwrap();
        assert_eq!(name(&aliased), Some(&Value::String("q".into())));

        // The audit rows are selected by the primary Root value.
        fn primary_root(context: &crate::BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            context.owned_field(SourceOwnerRef::Primary, &["Root"], None)
        }
        let rooted = owned(&context, SourceOwnerRef::Named("audit"), Some(primary_root)).unwrap();
        assert_eq!(name(&rooted), Some(&Value::String("n2".into())));
        assert!(matches!(
            owned(&context, SourceOwnerRef::Named("missing"), None),
            Err(RuntimeError::OwnedSource(
                crate::OwnedSourceError::SourceUnavailable { .. }
            ))
        ));

        let empty = owned_rows("-", &[]);
        let roots = crate::BasicSourceRoots::try_new(&empty, &[], limits).unwrap();
        assert_eq!(
            owned(
                &roots.root_context().unwrap(),
                SourceOwnerRef::Primary,
                None
            ),
            Err(RuntimeError::AdjacencyRootCount { count: 0 })
        );
    }
}
