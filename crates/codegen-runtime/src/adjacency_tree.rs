use std::collections::BTreeMap;

use crate::{
    Instance, MAX_GENERATED_SEQUENCE_ITEMS, MAX_RECURSIVE_SEQUENCE_DEPTH, RuntimeError,
    ScopeContext, Value,
};

pub type AdjacencyTreeRoot = for<'a> fn(&ScopeContext<'a>) -> Result<Value, RuntimeError>;

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
    let AdjacencyTreeFields {
        collection,
        key,
        parent,
        target_key,
        target_children,
    } = fields;
    let collection = context.repeated_source(collection).ok_or_else(|| {
        RuntimeError::MissingAdjacencyCollection {
            path: collection.join("/"),
        }
    })?;
    if collection.len() as u128 > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(RuntimeError::AdjacencyTreeTooLarge {
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }

    let mut rows = Vec::with_capacity(collection.len());
    let mut by_key = BTreeMap::new();
    let mut by_parent: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (index, instance) in collection.iter().enumerate() {
        let key = string_field(instance, key, "key")?;
        if by_key.insert(key.clone(), index).is_some() {
            return Err(RuntimeError::DuplicateAdjacencyKey { key });
        }
        let parent = optional_string_field(instance, parent, "parent")?;
        by_parent.entry(parent).or_default().push(index);
        rows.push(Row { key });
    }

    let selected_root = match root {
        Some(root) => match root(context)? {
            Value::Null | Value::JsonNull(_) => None,
            Value::String(value) => Some(value),
            value => {
                return Err(RuntimeError::InvalidAdjacencyRoot {
                    found: value.type_name(),
                });
            }
        },
        None => None,
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
    Ok(Instance::Group(vec![
        (
            target_key.to_string(),
            Instance::Scalar(Value::String(row.key.clone())),
        ),
        (target_children.to_string(), Instance::Repeated(children)),
    ]))
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
        Instance::Group(vec![(
            "Rows".to_string(),
            Instance::Repeated(
                values
                    .iter()
                    .map(|(key, parent)| {
                        Instance::Group(vec![
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
                    })
                    .collect(),
            ),
        )])
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
            Ok(Instance::Group(vec![
                (
                    "name".to_string(),
                    Instance::Scalar(Value::String("Root".to_string())),
                ),
                (
                    "children".to_string(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![
                            (
                                "name".to_string(),
                                Instance::Scalar(Value::String("Beta".to_string())),
                            ),
                            (
                                "children".to_string(),
                                Instance::Repeated(vec![Instance::Group(vec![
                                    (
                                        "name".to_string(),
                                        Instance::Scalar(Value::String("Leaf".to_string())),
                                    ),
                                    ("children".to_string(), Instance::Repeated(Vec::new())),
                                ])]),
                            ),
                        ]),
                        Instance::Group(vec![
                            (
                                "name".to_string(),
                                Instance::Scalar(Value::String("Alpha".to_string())),
                            ),
                            ("children".to_string(), Instance::Repeated(Vec::new())),
                        ]),
                    ]),
                ),
            ]))
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
}
