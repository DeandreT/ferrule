use crate::{
    Instance, MAX_RECURSIVE_SEQUENCE_DEPTH, RuntimeError, ScopeContext, Value, require_bool,
};
use ir::{XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD};

pub type RecursiveFilterPredicate = for<'a> fn(&ScopeContext<'a>) -> Result<Value, RuntimeError>;

/// Clones the current group while recursively filtering one repeated item
/// field at every level of a repeated recursive child field.
pub fn recursive_filter(
    context: &ScopeContext<'_>,
    children: &str,
    items: &str,
    predicate_node: u32,
    predicate: RecursiveFilterPredicate,
) -> Result<Instance, RuntimeError> {
    filter_group(context, children, items, predicate_node, predicate, 0)
}

fn filter_group(
    context: &ScopeContext<'_>,
    children: &str,
    items: &str,
    predicate_node: u32,
    predicate: RecursiveFilterPredicate,
    depth: usize,
) -> Result<Instance, RuntimeError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(RuntimeError::RecursiveFilterDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    let Some(current) = context.current_instance() else {
        return Err(RuntimeError::RecursiveFilterRequiresGroup {
            found: "missing context",
        });
    };
    let Instance::Group(fields) = current else {
        return Err(RuntimeError::RecursiveFilterRequiresGroup {
            found: instance_kind(current),
        });
    };

    let mut output = Vec::with_capacity(fields.len());
    let mut kept_items = None;
    let mut filtered_children = false;
    for (name, value) in fields {
        let value = if name == items {
            let (filtered, kept) = filter_items(context, value, items, predicate_node, predicate)?;
            kept_items = Some(kept);
            filtered
        } else if name == children {
            filtered_children = true;
            filter_children(
                context,
                value,
                children,
                items,
                predicate_node,
                predicate,
                depth,
            )?
        } else {
            value.clone()
        };
        output.push((name.clone(), value));
    }
    if kept_items.is_some() || filtered_children {
        rebuild_ordered_xml(
            fields,
            &mut output,
            items,
            children,
            kept_items.as_deref().unwrap_or(&[]),
        );
    }
    Ok(Instance::Group(output))
}

// Reconcile the XML choice reader's private ordered child values with the
// filtered schema fields. The writer consumes these values when they exist.
fn rebuild_ordered_xml(
    source: &[(String, Instance)],
    output: &mut [(String, Instance)],
    items: &str,
    children: &str,
    kept_items: &[bool],
) {
    let Some(Instance::Repeated(ordered)) = source
        .iter()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
        .map(|(_, value)| value)
    else {
        return;
    };
    let filtered_items = output
        .iter()
        .find(|(name, _)| name == items)
        .and_then(|(_, value)| value.as_repeated());
    let filtered_children = output
        .iter()
        .find(|(name, _)| name == children)
        .and_then(|(_, value)| value.as_repeated());
    let mut item_index = 0;
    let mut kept_index = 0;
    let mut child_index = 0;
    let mut rebuilt = Vec::with_capacity(ordered.len());
    for entry in ordered {
        let Some(name) = entry
            .field(XML_NODE_NAME_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
        else {
            rebuilt.push(entry.clone());
            continue;
        };
        let replacement = if name == items {
            let keep = kept_items.get(item_index).copied().unwrap_or(false);
            item_index += 1;
            if !keep {
                continue;
            }
            let value = filtered_items.and_then(|items| items.get(kept_index));
            kept_index += 1;
            value
        } else if name == children {
            let value = filtered_children.and_then(|children| children.get(child_index));
            child_index += 1;
            value
        } else {
            None
        };
        if (name == items || name == children) && replacement.is_none() {
            continue;
        }
        let mut entry = entry.clone();
        if let (Some(replacement), Instance::Group(fields)) = (replacement, &mut entry)
            && let Some((_, value)) = fields
                .iter_mut()
                .find(|(name, _)| name == XML_MIXED_CONTENT_VALUE_FIELD)
        {
            *value = replacement.clone();
        }
        rebuilt.push(entry);
    }
    if let Some((_, value)) = output
        .iter_mut()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    {
        *value = Instance::Repeated(rebuilt);
    }
}

fn filter_items(
    context: &ScopeContext<'_>,
    collection: &Instance,
    items: &str,
    predicate_node: u32,
    predicate: RecursiveFilterPredicate,
) -> Result<(Instance, Vec<bool>), RuntimeError> {
    let Instance::Repeated(values) = collection else {
        return Err(RuntimeError::RecursiveFilterRequiresCollection {
            field: items.to_string(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(values.len());
    let mut kept = Vec::with_capacity(values.len());
    for (index, item) in values.iter().enumerate() {
        let item_context = context.with_recursive_filter_item(item, items, index + 1);
        let keep = require_bool(predicate_node, predicate(&item_context)?)?;
        kept.push(keep);
        if keep {
            output.push(item.clone());
        }
    }
    Ok((Instance::Repeated(output), kept))
}

#[allow(clippy::too_many_arguments)]
fn filter_children(
    context: &ScopeContext<'_>,
    collection: &Instance,
    children: &str,
    items: &str,
    predicate_node: u32,
    predicate: RecursiveFilterPredicate,
    depth: usize,
) -> Result<Instance, RuntimeError> {
    let Instance::Repeated(values) = collection else {
        return Err(RuntimeError::RecursiveFilterRequiresCollection {
            field: children.to_string(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(values.len());
    for (index, child) in values.iter().enumerate() {
        let child_context = context.with_recursive_filter_item(child, children, index + 1);
        output.push(filter_group(
            &child_context,
            children,
            items,
            predicate_node,
            predicate,
            depth + 1,
        )?);
    }
    Ok(Instance::Repeated(output))
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
    use crate::{field, group, integer, repeated, scalar, string};
    use ir::XML_TEXT_FIELD;

    fn file(name: &str, expected_position: i64) -> Instance {
        group([
            field("name", scalar(string(name))),
            field("expected", scalar(integer(expected_position))),
        ])
    }

    fn directory(name: &str, files: Vec<Instance>, children: Vec<Instance>) -> Instance {
        group([
            field("name", scalar(string(name))),
            field("file", repeated(files)),
            field("directory", repeated(children)),
        ])
    }

    fn keep(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
        let Value::String(name) = context.resolve_scalar(&["name"])? else {
            return Ok(Value::Bool(false));
        };
        let Value::String(suffix) = context.resolve_scalar(&["suffix"])? else {
            return Ok(Value::Bool(false));
        };
        let Value::Int(expected) = context.resolve_scalar(&["expected"])? else {
            return Ok(Value::Bool(false));
        };
        Ok(Value::Bool(
            name.ends_with(&suffix) && context.position(&["file"]) == expected as usize,
        ))
    }

    fn always_true(_context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
        Ok(Value::Bool(true))
    }

    #[test]
    fn filters_every_level_with_positions_and_outward_fallback() {
        let source = group([
            field("suffix", scalar(string(".keep"))),
            field("name", scalar(string("root"))),
            field(
                "file",
                repeated([file("drop.txt", 1), file("root.keep", 2)]),
            ),
            field(
                "directory",
                repeated([directory(
                    "nested",
                    vec![file("nested.keep", 1), file("drop.md", 2)],
                    Vec::new(),
                )]),
            ),
        ]);

        let output = recursive_filter(&ScopeContext::new(&source), "directory", "file", 7, keep);
        let Ok(Instance::Group(fields)) = output else {
            panic!("recursive filter succeeds with a group");
        };
        let Some(Instance::Repeated(files)) = fields
            .iter()
            .find(|(name, _)| name == "file")
            .map(|(_, value)| value)
        else {
            panic!("root files remain repeated");
        };
        assert_eq!(files.len(), 1);
        let Some(Instance::Repeated(children)) = fields
            .iter()
            .find(|(name, _)| name == "directory")
            .map(|(_, value)| value)
        else {
            panic!("children remain repeated");
        };
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0]
                .field("file")
                .and_then(Instance::as_repeated)
                .map(<[Instance]>::len),
            Some(1)
        );
    }

    #[test]
    fn reports_shape_boolean_and_depth_errors() {
        let scalar_source = scalar(string("not a group"));
        assert_eq!(
            recursive_filter(
                &ScopeContext::new(&scalar_source),
                "directory",
                "file",
                7,
                always_true,
            ),
            Err(RuntimeError::RecursiveFilterRequiresGroup { found: "scalar" })
        );

        let malformed = group([field("file", scalar(string("not repeated")))]);
        assert_eq!(
            recursive_filter(
                &ScopeContext::new(&malformed),
                "directory",
                "file",
                7,
                always_true,
            ),
            Err(RuntimeError::RecursiveFilterRequiresCollection {
                field: "file".into(),
                found: "scalar",
            })
        );

        fn not_bool(_context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
            Ok(string("no"))
        }
        let one = directory("root", vec![file("x", 1)], Vec::new());
        assert_eq!(
            recursive_filter(&ScopeContext::new(&one), "directory", "file", 7, not_bool,),
            Err(RuntimeError::NotABool {
                node: 7,
                found: "string",
            })
        );

        let mut deep = directory("leaf", Vec::new(), Vec::new());
        for index in 0..255 {
            deep = directory(&format!("level-{index}"), Vec::new(), vec![deep]);
        }
        assert!(
            recursive_filter(
                &ScopeContext::new(&deep),
                "directory",
                "file",
                7,
                always_true,
            )
            .is_ok()
        );
        deep = directory("overflow", Vec::new(), vec![deep]);
        assert_eq!(
            recursive_filter(
                &ScopeContext::new(&deep),
                "directory",
                "file",
                7,
                always_true,
            ),
            Err(RuntimeError::RecursiveFilterDepth { limit: 256 })
        );
    }

    #[test]
    fn absent_collection_fields_are_preserved_without_runtime_errors() {
        let source = group([field("name", scalar(string("sparse")))]);
        assert_eq!(
            recursive_filter(
                &ScopeContext::new(&source),
                "directory",
                "file",
                7,
                always_true,
            ),
            Ok(source)
        );
    }

    #[test]
    fn ordered_xml_values_follow_filtered_items_and_nested_children() {
        let nested = with_ordered_content(
            directory(
                "nested",
                vec![file("nested.keep", 1), file("drop.md", 2)],
                Vec::new(),
            ),
            &["file", "file"],
        );
        let source = with_ordered_content(
            group([
                field("suffix", scalar(string(".keep"))),
                field("name", scalar(string("root"))),
                field(
                    "file",
                    repeated([file("drop.txt", 1), file("root.keep", 2)]),
                ),
                field("directory", repeated([nested])),
            ]),
            &["file", "directory", "file"],
        );
        let expected_nested = with_ordered_content(
            directory("nested", vec![file("nested.keep", 1)], Vec::new()),
            &["file"],
        );
        let expected = with_ordered_content(
            group([
                field("suffix", scalar(string(".keep"))),
                field("name", scalar(string("root"))),
                field("file", repeated([file("root.keep", 2)])),
                field("directory", repeated([expected_nested])),
            ]),
            &["directory", "file"],
        );

        assert_eq!(
            recursive_filter(&ScopeContext::new(&source), "directory", "file", 7, keep),
            Ok(expected.clone())
        );
        let mut stale = source;
        let Instance::Group(fields) = &mut stale else {
            unreachable!("ordered source is a group");
        };
        let Some((_, Instance::Repeated(ordered))) = fields
            .iter_mut()
            .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
        else {
            unreachable!("ordered source has XML metadata");
        };
        ordered.push(ordered[0].clone());
        assert_eq!(
            recursive_filter(&ScopeContext::new(&stale), "directory", "file", 7, keep),
            Ok(expected)
        );
    }

    fn with_ordered_content(mut directory: Instance, order: &[&str]) -> Instance {
        let Instance::Group(fields) = &mut directory else {
            unreachable!("directory helper produces a group");
        };
        let mut file_index = 0;
        let mut child_index = 0;
        let ordered = order
            .iter()
            .map(|name| {
                let index = if *name == "file" {
                    let index = file_index;
                    file_index += 1;
                    index
                } else {
                    let index = child_index;
                    child_index += 1;
                    index
                };
                let value = fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .and_then(|(_, value)| value.as_repeated())
                    .and_then(|values| values.get(index))
                    .expect("ordered XML item has a visible child")
                    .clone();
                group([
                    field(XML_NODE_NAME_FIELD, scalar(string(*name))),
                    field(XML_TEXT_FIELD, scalar(string(""))),
                    field(XML_MIXED_CONTENT_VALUE_FIELD, value),
                ])
            })
            .collect();
        fields.push((XML_MIXED_CONTENT_FIELD.into(), Instance::Repeated(ordered)));
        directory
    }
}
