use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode};
use mapping::{Node, NodeId, Project, Scope, ScopeConstruction, SequenceExpr};

use crate::MfdError;

use super::schema::{KeyAlloc, PortTree, xml_escape};
use super::source::SourceExports;

pub(super) struct RenderArgs<'a> {
    pub(super) scope: &'a Scope,
    pub(super) sources: &'a SourceExports<'a>,
    pub(super) target_ports: &'a PortTree,
    pub(super) node_out_key: &'a std::collections::BTreeMap<NodeId, u32>,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
}

pub(super) fn validate_target(
    project: &Project,
    target: &SchemaNode,
    root: &Scope,
) -> Result<(), MfdError> {
    validate_scope(project, target, root, &mut Vec::new(), &[])
}

pub(super) fn requires_root_port(scope: &Scope) -> bool {
    matches!(
        scope.construction,
        ScopeConstruction::RecursiveFilter { .. }
            | ScopeConstruction::PathHierarchy { .. }
            | ScopeConstruction::AdjacencyTree { .. }
    )
}

pub(super) fn seed_context_fields(
    project: &Project,
    sources: &SourceExports<'_>,
    node_out_key: &mut std::collections::BTreeMap<NodeId, u32>,
) {
    let mut pending = Vec::new();
    collect_filter_contexts(&project.root, &mut pending);
    for target in &project.extra_targets {
        collect_filter_contexts(&target.root, &mut pending);
    }
    while let Some((node, collection)) = pending.pop() {
        let Some(value) = project.graph.nodes.get(&node) else {
            continue;
        };
        if let Node::SourceField { path, frame: None } = value {
            let mut absolute = collection.clone();
            absolute.extend(path.iter().cloned());
            if let Some(port) = sources.key_for_abs(&absolute) {
                node_out_key.entry(node).or_insert(port);
            }
        }
        pending.extend(
            node_inputs(value)
                .into_iter()
                .map(|input| (input, collection.clone())),
        );
    }
}

pub(super) fn render_construction(args: RenderArgs<'_>) -> Result<(), MfdError> {
    let RenderArgs {
        scope,
        sources,
        target_ports,
        node_out_key,
        keys,
        uid,
        components,
        edges,
    } = args;
    let target = target_ports
        .key_for_abs(&[])
        .ok_or_else(|| unsupported("recursive root construction has no target root port"))?;
    match &scope.construction {
        ScopeConstruction::RecursiveFilter { plan } => {
            let source = sources
                .key_for_abs(&[])
                .ok_or_else(|| unsupported("recursive-filter has no source root port"))?;
            let predicate = node_out_key
                .get(&plan.predicate())
                .copied()
                .ok_or_else(|| unsupported("recursive-filter predicate node was not rendered"))?;
            let input_source = keys.next();
            let input_predicate = keys.next();
            let output = keys.next();
            *uid += 1;
            let metadata = metadata(
                "recursive-filter",
                None,
                &[],
                &[("children", plan.children()), ("items", plan.items())],
            );
            render_component(
                components,
                *uid,
                "recursive-filter",
                &[input_source, input_predicate],
                output,
                &metadata,
            );
            edges.extend([
                (source, input_source),
                (predicate, input_predicate),
                (output, target),
            ]);
        }
        ScopeConstruction::PathHierarchy { plan } => {
            let source = sources.key_for_abs(plan.collection()).ok_or_else(|| {
                unsupported("path-hierarchy collection has no source schema port")
            })?;
            let input = keys.next();
            let output = keys.next();
            *uid += 1;
            let metadata = metadata(
                "path-hierarchy",
                Some(("separator", plan.separator())),
                &[("collection", plan.collection())],
                &[
                    ("directories", plan.directories()),
                    ("files", plan.files()),
                    ("name", plan.name()),
                ],
            );
            render_component(
                components,
                *uid,
                "path-hierarchy",
                &[input],
                output,
                &metadata,
            );
            edges.extend([(source, input), (output, target)]);
        }
        ScopeConstruction::AdjacencyTree { plan } => {
            let source = sources.key_for_abs(plan.collection()).ok_or_else(|| {
                unsupported("adjacency-tree collection has no source schema port")
            })?;
            let input_collection = keys.next();
            let input_root = plan.root().map(|_| keys.next());
            let output = keys.next();
            let root = plan
                .root()
                .map(|root| {
                    node_out_key
                        .get(&root)
                        .copied()
                        .ok_or_else(|| unsupported("adjacency-tree root node was not rendered"))
                })
                .transpose()?;
            *uid += 1;
            let has_root = if input_root.is_some() { "1" } else { "0" };
            let metadata = metadata(
                "adjacency-tree",
                Some(("has-root", has_root)),
                &[
                    ("collection", plan.collection()),
                    ("key", plan.key()),
                    ("parent", plan.parent()),
                ],
                &[
                    ("target-key", plan.target_key()),
                    ("target-children", plan.target_children()),
                ],
            );
            let inputs = input_root.map_or_else(
                || vec![input_collection],
                |input_root| vec![input_collection, input_root],
            );
            render_component(
                components,
                *uid,
                "adjacency-tree",
                &inputs,
                output,
                &metadata,
            );
            edges.push((source, input_collection));
            if let (Some(root), Some(input_root)) = (root, input_root) {
                edges.push((root, input_root));
            }
            edges.push((output, target));
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn collect_metadata(
    collection: &[String],
    children: &[String],
    descent_value: &[String],
    values: &[String],
    value: &[String],
) -> String {
    metadata(
        "recursive-collect",
        None,
        &[
            ("collection", collection),
            ("children", children),
            ("descent-value", descent_value),
            ("values", values),
            ("value", value),
        ],
        &[],
    )
}

fn validate_scope(
    project: &Project,
    target: &SchemaNode,
    scope: &Scope,
    chain: &mut Vec<String>,
    source_anchor: &[String],
) -> Result<(), MfdError> {
    let source_collection = scope.source().map(|source| {
        if is_named_source_path(project, source) {
            source.to_vec()
        } else {
            let mut absolute = source_anchor.to_vec();
            absolute.extend_from_slice(source);
            absolute
        }
    });
    match &scope.construction {
        ScopeConstruction::Scalar { value } => match scope.sequence() {
            Some(SequenceExpr::RecursiveCollect {
                collection,
                prefix,
                separator,
                item,
                ..
            }) if value == item => {
                if !project.graph.nodes.contains_key(prefix)
                    || !project.graph.nodes.contains_key(separator)
                    || !project.graph.nodes.contains_key(item)
                {
                    return Err(unsupported(
                        "recursive-collect references a missing graph node",
                    ));
                }
                let source = source_node(project, collection).ok_or_else(|| {
                    unsupported("recursive-collect collection does not exist in a source schema")
                })?;
                if !matches!(source.kind, SchemaKind::Group { .. }) {
                    return Err(unsupported(
                        "recursive-collect collection must identify a source group",
                    ));
                }
                let target_node = target_node(target, chain)
                    .ok_or_else(|| unsupported("recursive-collect target path does not exist"))?;
                if !target_node.repeating || !matches!(target_node.kind, SchemaKind::Scalar { .. })
                {
                    return Err(unsupported(
                        "recursive-collect must produce one repeating scalar target",
                    ));
                }
                if has_controls_or_content(scope) {
                    return Err(unsupported(
                        "recursive-collect with controls or descendant content is not exportable canonically",
                    ));
                }
            }
            _ => {
                validate_scalar_identity_scope(
                    project,
                    target,
                    scope,
                    chain,
                    source_collection.as_deref(),
                    *value,
                )?;
            }
        },
        ScopeConstruction::RecursiveFilter { plan } => {
            require_root_only(chain, scope, "recursive-filter")?;
            if !project.graph.nodes.contains_key(&plan.predicate()) {
                return Err(unsupported(
                    "recursive-filter predicate references a missing graph node",
                ));
            }
        }
        ScopeConstruction::PathHierarchy { .. } => {
            require_root_only(chain, scope, "path-hierarchy")?;
        }
        ScopeConstruction::AdjacencyTree { plan } => {
            require_root_only(chain, scope, "adjacency-tree")?;
            if plan
                .root()
                .is_some_and(|root| !project.graph.nodes.contains_key(&root))
            {
                return Err(unsupported(
                    "adjacency-tree root references a missing graph node",
                ));
            }
        }
        ScopeConstruction::Constructed
        | ScopeConstruction::CopyCurrentSource
        | ScopeConstruction::XmlMixedContent { .. } => {}
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        validate_scope(
            project,
            target,
            child,
            chain,
            source_collection.as_deref().unwrap_or(source_anchor),
        )?;
        chain.pop();
    }
    for child in &scope.dynamic_children {
        chain.push(child.scope.target_field.clone());
        validate_scope(
            project,
            target,
            &child.scope,
            chain,
            source_collection.as_deref().unwrap_or(source_anchor),
        )?;
        chain.pop();
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            validate_scope(project, target, segment, chain, source_anchor)?;
        }
    }
    Ok(())
}

fn validate_scalar_identity_scope(
    project: &Project,
    target: &SchemaNode,
    scope: &Scope,
    target_path: &[String],
    source_collection: Option<&[String]>,
    value: NodeId,
) -> Result<(), MfdError> {
    let source_collection = source_collection
        .ok_or_else(|| unsupported("scalar scope construction requires a source collection"))?;
    let source = source_node(project, source_collection)
        .filter(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
        .ok_or_else(|| {
            unsupported("scalar scope construction requires a repeating scalar source")
        })?;
    let target = target_node(target, target_path)
        .filter(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
        .ok_or_else(|| {
            unsupported("scalar scope construction requires a repeating scalar target")
        })?;
    let _ = (source, target);
    if !scope.bindings.is_empty()
        || !scope.dynamic_bindings.is_empty()
        || !scope.children.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
        || scope.join().is_some()
        || scope.concatenated().is_some()
    {
        return Err(unsupported(
            "scalar identity construction cannot contain fields, joins, or concatenated branches",
        ));
    }
    let Some(Node::SourceField { path, frame }) = project.graph.nodes.get(&value) else {
        return Err(unsupported(
            "scalar scope construction export requires the exact current source value",
        ));
    };
    let mut absolute = frame.clone().unwrap_or_default();
    absolute.extend(path.iter().cloned());
    if absolute != source_collection {
        return Err(unsupported(
            "scalar scope construction export requires the exact current source value",
        ));
    }
    Ok(())
}

fn is_named_source_path(project: &Project, path: &[String]) -> bool {
    path.first().is_some_and(|name| {
        project
            .extra_sources
            .iter()
            .any(|source| source.name == *name)
    })
}

fn collect_filter_contexts(scope: &Scope, contexts: &mut Vec<(NodeId, Vec<String>)>) {
    if let ScopeConstruction::RecursiveFilter { plan } = &scope.construction {
        contexts.push((plan.predicate(), vec![plan.items().to_string()]));
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            collect_filter_contexts(segment, contexts);
        }
    }
    for child in &scope.children {
        collect_filter_contexts(child, contexts);
    }
    for child in &scope.dynamic_children {
        collect_filter_contexts(&child.scope, contexts);
    }
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

fn require_root_only(chain: &[String], scope: &Scope, kind: &str) -> Result<(), MfdError> {
    if !chain.is_empty() || has_controls_or_content(scope) || scope.sequence().is_some() {
        return Err(unsupported(&format!(
            "{kind} export currently requires one uncontrolled document-root construction"
        )));
    }
    Ok(())
}

fn has_controls_or_content(scope: &Scope) -> bool {
    scope.source().is_some()
        || scope.join().is_some()
        || scope.filter.is_some()
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_adjacent_by.is_some()
        || scope.group_ending_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some()
        || !scope.windows.is_empty()
        || !scope.bindings.is_empty()
        || !scope.children.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.concatenated().is_some()
}

fn source_node<'a>(project: &'a Project, path: &[String]) -> Option<&'a SchemaNode> {
    let (schema, local) = path
        .first()
        .and_then(|name| {
            project
                .extra_sources
                .iter()
                .find(|source| source.name == *name)
                .map(|source| (&source.schema, &path[1..]))
        })
        .unwrap_or((&project.source, path));
    target_node(schema, local)
}

fn target_node<'a>(root: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = root;
    for segment in path {
        if let Some(anchor) = &node.recursive_ref {
            node = find_concrete_schema_group(root, anchor)?;
        }
        node = node.child(segment)?;
    }
    match &node.recursive_ref {
        Some(anchor) => find_concrete_schema_group(root, anchor),
        None => Some(node),
    }
}

fn find_concrete_schema_group<'a>(schema: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if schema.recursive_ref.is_none()
        && schema.name == anchor
        && matches!(schema.kind, SchemaKind::Group { .. })
    {
        return Some(schema);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

fn render_component(
    components: &mut String,
    uid: u32,
    name: &str,
    inputs: &[u32],
    output: u32,
    metadata: &str,
) {
    let mut pins = String::new();
    for (position, key) in inputs.iter().enumerate() {
        let _ = write!(pins, "<datapoint pos=\"{position}\" key=\"{key}\"/>");
    }
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"{name}\" library=\"ferrule\" uid=\"{uid}\" kind=\"5\">\n\
         \t\t\t\t\t<sources>{pins}</sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
         \t\t\t\t\t<data>{metadata}</data>\n\
         \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"140\" rby=\"60\"/>\n\
         \t\t\t\t</component>\n"
    );
}

fn metadata(
    kind: &str,
    attribute: Option<(&str, &str)>,
    paths: &[(&str, &[String])],
    fields: &[(&str, &str)],
) -> String {
    let mut out = format!(
        "<ferrule-recursive version=\"1\" kind=\"{}\"",
        xml_escape(kind)
    );
    if let Some((name, value)) = attribute {
        let _ = write!(out, " {name}=\"{}\"", xml_escape(value));
    }
    out.push('>');
    for (role, segments) in paths {
        let _ = write!(out, "<path role=\"{}\">", xml_escape(role));
        for segment in *segments {
            let _ = write!(out, "<segment name=\"{}\"/>", xml_escape(segment));
        }
        out.push_str("</path>");
    }
    for (role, name) in fields {
        let _ = write!(
            out,
            "<field role=\"{}\" name=\"{}\"/>",
            xml_escape(role),
            xml_escape(name)
        );
    }
    out.push_str("</ferrule-recursive>");
    out
}

fn unsupported(message: &str) -> MfdError {
    MfdError::Unsupported(message.to_string())
}
