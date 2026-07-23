use crate::{
    Instance, MAX_RECURSIVE_SEQUENCE_DEPTH, RuntimeError, ScopeContext, Value, require_bool,
};

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
    for (name, value) in fields {
        let value = if name == items {
            filter_items(context, value, items, predicate_node, predicate)?
        } else if name == children {
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
    Ok(Instance::Group(output))
}

fn filter_items(
    context: &ScopeContext<'_>,
    collection: &Instance,
    items: &str,
    predicate_node: u32,
    predicate: RecursiveFilterPredicate,
) -> Result<Instance, RuntimeError> {
    let Instance::Repeated(values) = collection else {
        return Err(RuntimeError::RecursiveFilterRequiresCollection {
            field: items.to_string(),
            found: instance_kind(collection),
        });
    };
    let mut output = Vec::with_capacity(values.len());
    for (index, item) in values.iter().enumerate() {
        let item_context = context.with_recursive_filter_item(item, items, index + 1);
        if require_bool(predicate_node, predicate(&item_context)?)? {
            output.push(item.clone());
        }
    }
    Ok(Instance::Repeated(output))
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
}
