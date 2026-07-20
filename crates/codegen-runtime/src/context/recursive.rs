use crate::{
    Instance, MAX_GENERATED_SEQUENCE_ITEMS, MAX_RECURSIVE_SEQUENCE_DEPTH, RuntimeError, Value,
};

use crate::generated_sequence::{RecursiveCollectPaths, recursive_scalar_text};

use super::ScopeContext;

impl ScopeContext<'_> {
    pub(crate) fn recursive_collect(
        &self,
        paths: RecursiveCollectPaths<'_>,
        prefix: &str,
        separator: &str,
    ) -> Result<Vec<Value>, RuntimeError> {
        let base = self
            .frames
            .iter()
            .rev()
            .find(|frame| {
                paths
                    .collection
                    .first()
                    .is_none_or(|first| frame.instance.field(first).is_some())
            })
            .map(|frame| (frame.instance, paths.collection));
        let named_base = || {
            let (name, rest) = paths.collection.split_first()?;
            Some((self.named_input(name)?, rest))
        };
        let fallback = || {
            self.frames
                .last()
                .map(|frame| (frame.instance, paths.collection))
        };
        let Some((base, collection)) = base.or_else(named_base).or_else(fallback) else {
            return Ok(Vec::new());
        };
        let mut roots = Vec::new();
        collect_instances(base, collection, &mut roots);
        let mut output = Vec::new();
        for root in roots {
            collect_recursive_group(
                root,
                paths.children,
                paths.descent_value,
                paths.values,
                paths.value,
                prefix,
                separator,
                0,
                &mut output,
            )?;
        }
        Ok(output)
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_recursive_group(
    group: &Instance,
    children: &[&str],
    descent_value: &[&str],
    values: &[&str],
    value: &[&str],
    prefix: &str,
    separator: &str,
    depth: usize,
    output: &mut Vec<Value>,
) -> Result<(), RuntimeError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(RuntimeError::RecursiveSequenceDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    let Some(segment) = scalar_at(group, descent_value) else {
        return Ok(());
    };
    let current_prefix = format!("{prefix}{separator}{}", recursive_scalar_text(segment)?);
    let mut leaves = Vec::new();
    collect_instances(group, values, &mut leaves);
    for leaf in leaves {
        let Some(value) = scalar_at(leaf, value) else {
            continue;
        };
        if output.len() as u128 >= MAX_GENERATED_SEQUENCE_ITEMS {
            return Err(RuntimeError::RecursiveSequenceTooLarge {
                max: MAX_GENERATED_SEQUENCE_ITEMS,
            });
        }
        output.push(Value::String(format!(
            "{current_prefix}{separator}{}",
            recursive_scalar_text(value)?
        )));
    }
    let mut child_groups = Vec::new();
    collect_instances(group, children, &mut child_groups);
    for child in child_groups {
        collect_recursive_group(
            child,
            children,
            descent_value,
            values,
            value,
            &current_prefix,
            separator,
            depth + 1,
            output,
        )?;
    }
    Ok(())
}

fn collect_instances<'a>(instance: &'a Instance, path: &[&str], output: &mut Vec<&'a Instance>) {
    if path.is_empty() {
        match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                output.extend(items.iter());
            }
            Instance::Scalar(_) | Instance::Group(_) => output.push(instance),
            Instance::DocumentSet(documents) => {
                output.extend(documents.iter().map(ir::DocumentMember::value));
            }
        }
        return;
    }
    match instance {
        Instance::Group(fields) => {
            if let Some((_, child)) = fields.iter().find(|(name, _)| name == path[0]) {
                collect_instances(child, &path[1..], output);
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for item in items {
                collect_instances(item, path, output);
            }
        }
        Instance::DocumentSet(documents) => {
            for document in documents {
                collect_instances(document.value(), path, output);
            }
        }
        Instance::Scalar(_) => {}
    }
}

fn scalar_at<'a>(instance: &'a Instance, path: &[&str]) -> Option<&'a Value> {
    if path.is_empty() {
        return instance.as_scalar();
    }
    match instance {
        Instance::Group(fields) => fields
            .iter()
            .find(|(name, _)| name == path[0])
            .and_then(|(_, child)| scalar_at(child, &path[1..])),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            items.first().and_then(|item| scalar_at(item, path))
        }
        Instance::DocumentSet(documents) => documents
            .first()
            .and_then(|document| scalar_at(document.value(), path)),
        Instance::Scalar(_) => None,
    }
}
