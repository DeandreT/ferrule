use ir::{SchemaKind, SchemaNode};
use mapping::{
    AdjacencyTreePlan, IterationOutput, Node, PathHierarchyPlan, RecursiveFilterPlan,
    ScopeConstruction, SequenceExpr,
};

use super::function::{FnComponent, RecursiveComponent, read as read_function};
use super::graph::GraphBuilder;
use super::scope::{IterationNodes, ScopeBuilder};
use super::source::SourcePath;

const COMPONENT_NAMES: [&str; 4] = [
    "recursive-collect",
    "recursive-filter",
    "path-hierarchy",
    "adjacency-tree",
];

pub(super) fn is_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("ferrule")
        && component.attribute("kind") == Some("5")
        && component
            .attribute("name")
            .is_some_and(|name| COMPONENT_NAMES.contains(&name))
}

pub(super) fn read_component(component: &roxmltree::Node<'_, '_>) -> Result<FnComponent, String> {
    let metadata = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| {
            let mut entries = data
                .children()
                .filter(|node| node.has_tag_name("ferrule-recursive"));
            let first = entries.next()?;
            entries.next().is_none().then_some(first)
        })
        .ok_or("expected exactly one <ferrule-recursive> metadata element")?;
    if metadata.attribute("version") != Some("1") {
        return Err("requires ferrule recursive metadata version 1".to_string());
    }
    let name = component.attribute("name").unwrap_or_default();
    if metadata.attribute("kind") != Some(name) {
        return Err("component name and recursive metadata kind differ".to_string());
    }
    let recursive = match name {
        "recursive-collect" => RecursiveComponent::Collect {
            collection: path(&metadata, "collection", true)?,
            children: path(&metadata, "children", false)?,
            descent_value: path(&metadata, "descent-value", false)?,
            values: path(&metadata, "values", false)?,
            value: path(&metadata, "value", false)?,
        },
        "recursive-filter" => RecursiveComponent::Filter {
            children: field(&metadata, "children")?,
            items: field(&metadata, "items")?,
        },
        "path-hierarchy" => RecursiveComponent::PathHierarchy {
            collection: path(&metadata, "collection", false)?,
            separator: non_empty_attribute(&metadata, "separator")?.to_string(),
            directories: field(&metadata, "directories")?,
            files: field(&metadata, "files")?,
            name: field(&metadata, "name")?,
        },
        "adjacency-tree" => RecursiveComponent::AdjacencyTree {
            collection: path(&metadata, "collection", false)?,
            key: path(&metadata, "key", false)?,
            parent: path(&metadata, "parent", false)?,
            target_key: field(&metadata, "target-key")?,
            target_children: field(&metadata, "target-children")?,
            has_root: match metadata.attribute("has-root") {
                Some("0") => false,
                Some("1") => true,
                _ => return Err("adjacency-tree has-root must be 0 or 1".to_string()),
            },
        },
        _ => return Err(format!("unsupported ferrule recursive component `{name}`")),
    };
    validate_roles(&metadata, &recursive)?;
    let mut function = read_function(component);
    validate_pin_shape(&function, &recursive)?;
    function.recursive = Some(recursive);
    Ok(function)
}

pub(super) fn accept_target(
    target_path: &[String],
    target_node: &SchemaNode,
    feed: u32,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> bool {
    let Some(index) = builder.fn_by_output.get(&feed).copied() else {
        return false;
    };
    let Some(recursive) = builder
        .fn_components
        .get(index)
        .and_then(|component| component.recursive.clone())
    else {
        return false;
    };
    if matches!(recursive, RecursiveComponent::Invalid) {
        builder.sequence_scope_components.insert(index);
        return true;
    }
    if !builder.sequence_scope_components.insert(index) {
        builder.warnings.push(format!(
            "ferrule recursive component feeding `{}` is connected to more than one target; later connection skipped",
            target_path.join("/")
        ));
        return true;
    }
    let result = apply_target(target_path, target_node, index, recursive, builder, scopes);
    if let Err(reason) = result {
        builder.warnings.push(format!(
            "ferrule recursive component feeding `{}` is invalid: {reason}",
            target_path.join("/")
        ));
    }
    true
}

fn apply_target(
    target_path: &[String],
    target_node: &SchemaNode,
    index: usize,
    recursive: RecursiveComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> Result<(), String> {
    match recursive {
        RecursiveComponent::Invalid => {}
        RecursiveComponent::Collect {
            collection,
            children,
            descent_value,
            values,
            value,
        } => {
            if !target_node.repeating || !matches!(target_node.kind, SchemaKind::Scalar { .. }) {
                return Err(
                    "recursive-collect output must feed one repeating scalar target".into(),
                );
            }
            require_source_path(builder, input_feed(builder, index, 0)?, &collection)?;
            let prefix = scalar_input(builder, index, 1)?;
            let separator = scalar_input(builder, index, 2)?;
            let item = builder.alloc(Node::SourceField {
                path: Vec::new(),
                frame: None,
            });
            scopes.add_sequence(
                target_path,
                SequenceExpr::RecursiveCollect {
                    collection,
                    children,
                    descent_value,
                    values,
                    value,
                    prefix,
                    separator,
                    item,
                },
                IterationNodes::default(),
                IterationOutput::Repeated,
            );
            scopes.ensure_scope(target_path).construction =
                ScopeConstruction::Scalar { value: item };
        }
        RecursiveComponent::Filter { children, items } => {
            require_group_target(target_node, "recursive-filter")?;
            require_source_path(builder, input_feed(builder, index, 0)?, &[])?;
            builder.note_framed_prefixes(&SourcePath {
                source: 0,
                path: vec![items.clone()],
            });
            let predicate_feed = input_feed(builder, index, 1)?;
            let predicate = builder
                .scalar_node_at_anchor(predicate_feed, std::slice::from_ref(&items))
                .ok_or("input pin 1 is not a scalar expression in the filtered item context")?;
            let plan = RecursiveFilterPlan::new(children, items, predicate)
                .ok_or("recursive-filter fields are empty or collide")?;
            scopes.ensure_scope(target_path).construction =
                ScopeConstruction::RecursiveFilter { plan };
        }
        RecursiveComponent::PathHierarchy {
            collection,
            separator,
            directories,
            files,
            name,
        } => {
            require_group_target(target_node, "path-hierarchy")?;
            require_source_path(builder, input_feed(builder, index, 0)?, &collection)?;
            let plan = PathHierarchyPlan::new(collection, separator, directories, files, name)
                .ok_or("path-hierarchy metadata does not form a valid plan")?;
            scopes.ensure_scope(target_path).construction =
                ScopeConstruction::PathHierarchy { plan };
        }
        RecursiveComponent::AdjacencyTree {
            collection,
            key,
            parent,
            target_key,
            target_children,
            has_root,
        } => {
            require_group_target(target_node, "adjacency-tree")?;
            require_source_path(builder, input_feed(builder, index, 0)?, &collection)?;
            let root = has_root
                .then(|| scalar_input(builder, index, 1))
                .transpose()?;
            let plan =
                AdjacencyTreePlan::new(collection, key, parent, target_key, target_children, root)
                    .ok_or("adjacency-tree metadata does not form a valid plan")?;
            scopes.ensure_scope(target_path).construction =
                ScopeConstruction::AdjacencyTree { plan };
        }
    }
    Ok(())
}

fn input_feed(builder: &GraphBuilder<'_>, index: usize, position: usize) -> Result<u32, String> {
    builder
        .fn_components
        .get(index)
        .and_then(|component| component.inputs.get(position))
        .copied()
        .flatten()
        .and_then(|input| builder.edge_from.get(&input).copied())
        .ok_or_else(|| format!("input pin {position} is not connected"))
}

fn scalar_input(
    builder: &mut GraphBuilder<'_>,
    index: usize,
    position: usize,
) -> Result<mapping::NodeId, String> {
    let feed = input_feed(builder, index, position)?;
    builder
        .sequence_scalar_input(feed)
        .ok_or_else(|| format!("input pin {position} is not a scalar expression"))
}

fn require_source_path(
    builder: &GraphBuilder<'_>,
    feed: u32,
    expected: &[String],
) -> Result<(), String> {
    let source = builder
        .source_abs_path(feed)
        .ok_or("collection input is not a direct source schema port")?;
    let actual = builder.context_path(&source);
    (actual == expected).then_some(()).ok_or_else(|| {
        format!(
            "collection pin resolves to `{}`, expected `{}`",
            actual.join("/"),
            expected.join("/")
        )
    })
}

fn require_group_target(target: &SchemaNode, kind: &str) -> Result<(), String> {
    (!target.repeating && matches!(target.kind, SchemaKind::Group { .. }))
        .then_some(())
        .ok_or_else(|| format!("{kind} output must feed one non-repeating target group"))
}

fn validate_pin_shape(
    function: &FnComponent,
    recursive: &RecursiveComponent,
) -> Result<(), String> {
    if function
        .output_pins
        .as_slice()
        .iter()
        .filter(|pin| pin.is_some())
        .count()
        != 1
        || function.output_pins.len() != 1
    {
        return Err("requires exactly one keyed output pin".to_string());
    }
    let expected = match recursive {
        RecursiveComponent::Invalid => return Err("recursive metadata is invalid".to_string()),
        RecursiveComponent::Collect { .. } => 3,
        RecursiveComponent::Filter { .. } => 2,
        RecursiveComponent::PathHierarchy { .. } => 1,
        RecursiveComponent::AdjacencyTree { has_root, .. } => usize::from(*has_root) + 1,
    };
    (function.inputs.len() == expected && function.inputs.iter().all(Option::is_some))
        .then_some(())
        .ok_or_else(|| format!("requires exactly {expected} keyed input pin(s)"))
}

fn validate_roles(
    metadata: &roxmltree::Node<'_, '_>,
    recursive: &RecursiveComponent,
) -> Result<(), String> {
    let (paths, fields): (&[&str], &[&str]) = match recursive {
        RecursiveComponent::Invalid => return Err("recursive metadata is invalid".to_string()),
        RecursiveComponent::Collect { .. } => (
            &["collection", "children", "descent-value", "values", "value"],
            &[],
        ),
        RecursiveComponent::Filter { .. } => (&[], &["children", "items"]),
        RecursiveComponent::PathHierarchy { .. } => {
            (&["collection"], &["directories", "files", "name"])
        }
        RecursiveComponent::AdjacencyTree { .. } => (
            &["collection", "key", "parent"],
            &["target-key", "target-children"],
        ),
    };
    for child in metadata.children().filter(roxmltree::Node::is_element) {
        let role = child.attribute("role").unwrap_or_default();
        let valid = (child.has_tag_name("path") && paths.contains(&role))
            || (child.has_tag_name("field") && fields.contains(&role));
        if !valid {
            return Err(format!(
                "contains unknown metadata element or role `{role}`"
            ));
        }
    }
    Ok(())
}

fn path(
    metadata: &roxmltree::Node<'_, '_>,
    role: &str,
    allow_empty: bool,
) -> Result<Vec<String>, String> {
    let matches = metadata
        .children()
        .filter(|node| node.has_tag_name("path") && node.attribute("role") == Some(role))
        .collect::<Vec<_>>();
    let [path] = matches.as_slice() else {
        return Err(format!("requires exactly one `{role}` path"));
    };
    let segments = path
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|segment| {
            if !segment.has_tag_name("segment") {
                return Err(format!("path `{role}` contains a non-segment element"));
            }
            non_empty_attribute(&segment, "name").map(str::to_string)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !allow_empty && segments.is_empty() {
        return Err(format!("path `{role}` must not be empty"));
    }
    Ok(segments)
}

fn field(metadata: &roxmltree::Node<'_, '_>, role: &str) -> Result<String, String> {
    let matches = metadata
        .children()
        .filter(|node| node.has_tag_name("field") && node.attribute("role") == Some(role))
        .collect::<Vec<_>>();
    let [field] = matches.as_slice() else {
        return Err(format!("requires exactly one `{role}` field"));
    };
    non_empty_attribute(field, "name").map(str::to_string)
}

fn non_empty_attribute<'a>(
    node: &'a roxmltree::Node<'_, '_>,
    attribute: &str,
) -> Result<&'a str, String> {
    node.attribute(attribute)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("attribute `{attribute}` must not be empty"))
}
