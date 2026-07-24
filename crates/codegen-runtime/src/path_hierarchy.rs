use crate::{Instance, RuntimeError, ScopeContext, Value};

pub const MAX_PATH_HIERARCHY_DEPTH: usize = 256;
pub const MAX_PATH_HIERARCHY_ITEMS: usize = 1_000_000;

#[derive(Debug)]
struct Directory {
    name: String,
    files: Vec<String>,
    directories: Vec<Self>,
}

impl Directory {
    fn new(name: String) -> Self {
        Self {
            name,
            files: Vec::new(),
            directories: Vec::new(),
        }
    }

    fn insert(
        &mut self,
        path: &str,
        separator: &str,
        depth: usize,
        materialized: &mut usize,
    ) -> Result<(), RuntimeError> {
        if depth >= MAX_PATH_HIERARCHY_DEPTH {
            return Err(RuntimeError::PathHierarchyDepth {
                limit: MAX_PATH_HIERARCHY_DEPTH,
            });
        }
        let Some((directory, remainder)) = path.split_once(separator) else {
            reserve_item(materialized)?;
            self.files.push(path.to_string());
            return Ok(());
        };
        let child = match self
            .directories
            .iter()
            .position(|child| child.name == directory)
        {
            Some(index) => &mut self.directories[index],
            None => {
                reserve_item(materialized)?;
                self.directories.push(Self::new(directory.to_string()));
                self.directories
                    .last_mut()
                    .ok_or(RuntimeError::PathHierarchyTooLarge {
                        max: MAX_PATH_HIERARCHY_ITEMS,
                    })?
            }
        };
        child.insert(remainder, separator, depth + 1, materialized)
    }

    fn into_instance(
        self,
        directories_field: &str,
        files_field: &str,
        name_field: &str,
    ) -> Instance {
        let files = self
            .files
            .into_iter()
            .map(|name| {
                Instance::Group(vec![(
                    name_field.to_string(),
                    Instance::Scalar(Value::String(name)),
                )])
            })
            .collect();
        let directories = self
            .directories
            .into_iter()
            .map(|directory| directory.into_instance(directories_field, files_field, name_field))
            .collect();
        Instance::Group(vec![
            (files_field.to_string(), Instance::Repeated(files)),
            (
                directories_field.to_string(),
                Instance::Repeated(directories),
            ),
            (
                name_field.to_string(),
                Instance::Scalar(Value::String(self.name)),
            ),
        ])
    }
}

fn reserve_item(materialized: &mut usize) -> Result<(), RuntimeError> {
    *materialized = materialized
        .checked_add(1)
        .ok_or(RuntimeError::PathHierarchyTooLarge {
            max: MAX_PATH_HIERARCHY_ITEMS,
        })?;
    if *materialized > MAX_PATH_HIERARCHY_ITEMS {
        return Err(RuntimeError::PathHierarchyTooLarge {
            max: MAX_PATH_HIERARCHY_ITEMS,
        });
    }
    Ok(())
}

/// Builds one recursively nested directory tree from a repeated scalar path.
pub fn path_hierarchy(
    context: &ScopeContext<'_>,
    collection: &[&str],
    separator: &str,
    directories_field: &str,
    files_field: &str,
    name_field: &str,
) -> Result<Instance, RuntimeError> {
    let mut roots = Vec::<Directory>::new();
    let mut materialized = 0usize;
    for candidate in context.walk_source(collection) {
        let value = match candidate.current_instance() {
            Some(Instance::Scalar(Value::String(value))) => value.as_str(),
            Some(Instance::Scalar(Value::Null | Value::JsonNull(_) | Value::XmlNil(_))) => "",
            Some(Instance::Scalar(value)) => {
                return Err(RuntimeError::PathHierarchyValueType {
                    found: value.type_name(),
                });
            }
            Some(instance) => {
                return Err(RuntimeError::PathHierarchyValueType {
                    found: instance_kind(instance),
                });
            }
            None => continue,
        };
        if value.is_empty() {
            continue;
        }
        let Some((root, remainder)) = value.split_once(separator) else {
            continue;
        };
        let directory = match roots.iter().position(|candidate| candidate.name == root) {
            Some(index) => &mut roots[index],
            None => {
                reserve_item(&mut materialized)?;
                roots.push(Directory::new(root.to_string()));
                roots
                    .last_mut()
                    .ok_or(RuntimeError::PathHierarchyTooLarge {
                        max: MAX_PATH_HIERARCHY_ITEMS,
                    })?
            }
        };
        directory.insert(remainder, separator, 1, &mut materialized)?;
    }
    if roots.len() != 1 {
        return Err(RuntimeError::PathHierarchyRootCount { count: roots.len() });
    }
    let root = roots
        .pop()
        .ok_or(RuntimeError::PathHierarchyRootCount { count: 0 })?;
    Ok(root.into_instance(directories_field, files_field, name_field))
}

const fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "scalar",
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated collection",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(values: impl IntoIterator<Item = Value>) -> Instance {
        Instance::Group(vec![(
            "Paths".to_string(),
            Instance::Repeated(values.into_iter().map(Instance::Scalar).collect::<Vec<_>>()),
        )])
    }

    #[test]
    fn preserves_first_seen_order_and_duplicate_files() {
        let source = source([
            Value::String("root/b/b.txt".to_string()),
            Value::String("root/a.txt".to_string()),
            Value::String("root/b/b.txt".to_string()),
            Value::Null,
            Value::String("top-level.txt".to_string()),
        ]);

        let actual = path_hierarchy(
            &ScopeContext::new(&source),
            &["Paths"],
            "/",
            "directories",
            "files",
            "name",
        );

        assert_eq!(
            actual,
            Ok(Instance::Group(vec![
                (
                    "files".to_string(),
                    Instance::Repeated(vec![Instance::Group(vec![(
                        "name".to_string(),
                        Instance::Scalar(Value::String("a.txt".to_string())),
                    )])]),
                ),
                (
                    "directories".to_string(),
                    Instance::Repeated(vec![Instance::Group(vec![
                        (
                            "files".to_string(),
                            Instance::Repeated(vec![
                                Instance::Group(vec![(
                                    "name".to_string(),
                                    Instance::Scalar(Value::String("b.txt".to_string())),
                                )]),
                                Instance::Group(vec![(
                                    "name".to_string(),
                                    Instance::Scalar(Value::String("b.txt".to_string())),
                                )]),
                            ]),
                        ),
                        ("directories".to_string(), Instance::Repeated(Vec::new())),
                        (
                            "name".to_string(),
                            Instance::Scalar(Value::String("b".to_string())),
                        ),
                    ])]),
                ),
                (
                    "name".to_string(),
                    Instance::Scalar(Value::String("root".to_string())),
                ),
            ]))
        );
    }

    #[test]
    fn reports_multiple_roots_and_non_string_values() {
        let multiple = source([
            Value::String("one/a.txt".to_string()),
            Value::String("two/b.txt".to_string()),
        ]);
        assert_eq!(
            path_hierarchy(
                &ScopeContext::new(&multiple),
                &["Paths"],
                "/",
                "directories",
                "files",
                "name",
            ),
            Err(RuntimeError::PathHierarchyRootCount { count: 2 })
        );

        let invalid = source([Value::Int(1)]);
        assert_eq!(
            path_hierarchy(
                &ScopeContext::new(&invalid),
                &["Paths"],
                "/",
                "directories",
                "files",
                "name",
            ),
            Err(RuntimeError::PathHierarchyValueType { found: "int" })
        );
    }
}
