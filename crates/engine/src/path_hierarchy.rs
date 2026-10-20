use ir::{Instance, Value};
use mapping::{OwnedPathHierarchyPlan, PathHierarchyPlan};

use crate::EngineError;
use crate::source_iteration::walk;

pub(super) const MAX_PATH_HIERARCHY_DEPTH: usize = 256;
pub(super) const MAX_PATH_HIERARCHY_ITEMS: usize = 1_000_000;

/// Generated target field names; they never select source data.
struct OutputFields<'a> {
    directories: &'a str,
    files: &'a str,
    name: &'a str,
}

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

    fn into_instance(self, fields: &OutputFields<'_>) -> Instance {
        let files = self
            .files
            .into_iter()
            .map(|name| {
                Instance::Group(
                    (vec![(
                        fields.name.to_string(),
                        Instance::Scalar(Value::String(name)),
                    )])
                    .into(),
                )
            })
            .collect();
        let directories = self
            .directories
            .into_iter()
            .map(|directory| directory.into_instance(fields))
            .collect();
        Instance::Group(
            (vec![
                (fields.files.to_string(), Instance::Repeated(files)),
                (
                    fields.directories.to_string(),
                    Instance::Repeated(directories),
                ),
                (
                    fields.name.to_string(),
                    Instance::Scalar(Value::String(self.name)),
                ),
            ])
            .into(),
        )
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

/// One collection item's path text; absent and null items are empty paths.
fn path_value(instance: &Instance) -> Result<&str, EngineError> {
    match instance.as_scalar() {
        Some(Value::String(value)) => Ok(value.as_str()),
        Some(Value::Null | Value::JsonNull(_) | Value::XmlNil(_)) => Ok(""),
        Some(value) => Err(EngineError::PathHierarchyValueType {
            found: value.type_name(),
        }),
        None => Err(EngineError::PathHierarchyValueType {
            found: match instance {
                Instance::Group(_) => "group",
                Instance::Repeated(_) => "repeated collection",
                Instance::MappedSequence(_) => "mapped sequence",
                Instance::Scalar(_) => "scalar",
                Instance::DocumentSet(_) => "document set",
            },
        }),
    }
}

/// Builds the single rooted tree from ordered collection items. Item order,
/// limits and error precedence are shared by legacy and owned collections.
fn build_from<'a>(
    items: impl IntoIterator<Item = &'a Instance>,
    separator: &str,
    fields: &OutputFields<'_>,
) -> Result<Instance, EngineError> {
    let mut roots = Vec::<Directory>::new();
    let mut materialized = 0usize;
    for item in items {
        let value = path_value(item)?;
        if value.is_empty() {
            continue;
        }
        let Some((root, remainder)) = value.split_once(separator) else {
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
        directory.insert(remainder, separator, 1, &mut materialized)?;
    }
    if roots.len() != 1 {
        return Err(EngineError::PathHierarchyRootCount { count: roots.len() });
    }
    let Some(root) = roots.pop() else {
        return Err(EngineError::PathHierarchyRootCount { count: 0 });
    };
    Ok(root.into_instance(fields))
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
    let items = base
        .into_iter()
        .flat_map(|base| walk(base, plan.collection(), &[], &[], &[]))
        .filter_map(|extension| extension.instances.last().copied());
    build_from(
        items,
        plan.separator(),
        &OutputFields {
            directories: plan.directories(),
            files: plan.files(),
            name: plan.name(),
        },
    )
}

/// Builds from the exact owner's collection in the receiving owned context.
/// An active same-owner frame pins its prefix; otherwise the endpoint root is
/// used. Other owners and legacy frames never supply the collection.
pub(super) fn build_owned(
    plan: &OwnedPathHierarchyPlan,
    state: &crate::owned_context::OwnedContext<'_>,
    position_offset: usize,
    scope: &mapping::SourceScopeLocation,
) -> Result<Instance, EngineError> {
    use crate::source_iteration::{ObservedWalkRequest, select_observed};
    let access = |source| match source {
        crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(scope.clone()),
                context: if found == "a document set" {
                    crate::OwnedCapabilityContext::Document
                } else {
                    crate::OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(scope.clone())),
            source,
        },
    };
    let (owner, base, prefix, tail, budget) = state.select(plan.collection()).map_err(access)?;
    let selected = select_observed(
        ObservedWalkRequest {
            base,
            path: tail,
            raw_prefix: prefix,
            canonical_prefix: prefix,
            owner: Some(owner),
            context: state,
            position_offset,
            strict_collection: true,
        },
        &budget,
        false,
    )
    .map_err(access)?;
    build_from(
        selected
            .extensions
            .iter()
            .filter_map(|extension| extension.extension.instances.last().copied()),
        plan.separator(),
        &OutputFields {
            directories: plan.directories(),
            files: plan.files(),
            name: plan.name(),
        },
    )
}
