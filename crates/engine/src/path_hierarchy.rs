use ir::{Instance, Value};
use mapping::PathHierarchyPlan;

use crate::EngineError;
use crate::source_iteration::walk;

pub(super) const MAX_PATH_HIERARCHY_DEPTH: usize = 256;
pub(super) const MAX_PATH_HIERARCHY_ITEMS: usize = 1_000_000;

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
    ) -> Result<(), EngineError> {
        if depth >= MAX_PATH_HIERARCHY_DEPTH {
            return Err(EngineError::PathHierarchyDepth {
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
                let Some(child) = self.directories.last_mut() else {
                    return Err(EngineError::PathHierarchyTooLarge {
                        max: MAX_PATH_HIERARCHY_ITEMS,
                    });
                };
                child
            }
        };
        child.insert(remainder, separator, depth + 1, materialized)
    }

    fn into_instance(self, plan: &PathHierarchyPlan) -> Instance {
        let files = self
            .files
            .into_iter()
            .map(|name| {
                Instance::Group(vec![(
                    plan.name().to_string(),
                    Instance::Scalar(Value::String(name)),
                )])
            })
            .collect();
        let directories = self
            .directories
            .into_iter()
            .map(|directory| directory.into_instance(plan))
            .collect();
        Instance::Group(vec![
            (plan.files().to_string(), Instance::Repeated(files)),
            (
                plan.directories().to_string(),
                Instance::Repeated(directories),
            ),
            (
                plan.name().to_string(),
                Instance::Scalar(Value::String(self.name)),
            ),
        ])
    }
}

fn reserve_item(materialized: &mut usize) -> Result<(), EngineError> {
    *materialized = materialized
        .checked_add(1)
        .ok_or(EngineError::PathHierarchyTooLarge {
            max: MAX_PATH_HIERARCHY_ITEMS,
        })?;
    if *materialized > MAX_PATH_HIERARCHY_ITEMS {
        return Err(EngineError::PathHierarchyTooLarge {
            max: MAX_PATH_HIERARCHY_ITEMS,
        });
    }
    Ok(())
}

pub(super) fn build(
    plan: &PathHierarchyPlan,
    context: &[&Instance],
) -> Result<Instance, EngineError> {
    let base = context
        .iter()
        .rev()
        .find(|frame| {
            plan.collection()
                .first()
                .is_none_or(|first| frame.field(first).is_some())
        })
        .copied()
        .or_else(|| context.last().copied());
    let values = base
        .into_iter()
        .flat_map(|base| walk(base, plan.collection(), &[], &[], &[]))
        .filter_map(|extension| extension.instances.last().copied())
        .map(|instance| match instance.as_scalar() {
            Some(Value::String(value)) => Ok(value.as_str()),
            Some(Value::Null | Value::XmlNil(_)) => Ok(""),
            Some(value) => Err(EngineError::PathHierarchyValueType {
                found: value.type_name(),
            }),
            None => Err(EngineError::PathHierarchyValueType {
                found: match instance {
                    Instance::Group(_) => "group",
                    Instance::Repeated(_) => "repeated collection",
                    Instance::MappedSequence(_) => "mapped sequence",
                    Instance::Scalar(_) => "scalar",
                },
            }),
        });

    let mut roots = Vec::<Directory>::new();
    let mut materialized = 0usize;
    for value in values {
        let value = value?;
        if value.is_empty() {
            continue;
        }
        let Some((root, remainder)) = value.split_once(plan.separator()) else {
            // The public UDF port exposes directory results; direct files at
            // the synthetic top level are intentionally not connected.
            continue;
        };
        let directory = match roots.iter().position(|candidate| candidate.name == root) {
            Some(index) => &mut roots[index],
            None => {
                reserve_item(&mut materialized)?;
                roots.push(Directory::new(root.to_string()));
                let Some(directory) = roots.last_mut() else {
                    return Err(EngineError::PathHierarchyTooLarge {
                        max: MAX_PATH_HIERARCHY_ITEMS,
                    });
                };
                directory
            }
        };
        directory.insert(remainder, plan.separator(), 1, &mut materialized)?;
    }
    if roots.len() != 1 {
        return Err(EngineError::PathHierarchyRootCount { count: roots.len() });
    }
    let Some(root) = roots.pop() else {
        return Err(EngineError::PathHierarchyRootCount { count: 0 });
    };
    Ok(root.into_instance(plan))
}
