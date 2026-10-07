//! Conservative, explicit native item-order import. Legacy globals stay separate.

use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{
    Graph, IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration,
};

use super::super::function::{is_filter, read as read_function};
use super::super::graph::GraphBuilder;
use super::super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::{Recipe, unsupported_control};

const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 64;

pub(in crate::import) struct Attachment {
    name: String,
    root: Scope,
    graph: Graph,
    decimal_names: BTreeMap<NodeId, String>,
}

pub(in crate::import) fn has_exception(structure: roxmltree::Node<'_, '_>) -> bool {
    structure.descendants().any(|node| {
        node.has_tag_name("component")
            && node.attribute("library") == Some("core")
            && node.attribute("kind") == Some("18")
    })
}

/// No unsupported opted-in recipe is ever lowered into a global FailureRule.
pub(in crate::import) fn attach(
    recipes: Vec<Recipe>,
    builder: &mut GraphBuilder<'_>,
    root: &mut Scope,
    target: &SchemaComponent,
    structure: roxmltree::Node<'_, '_>,
    ordinary_single_boundary: bool,
) -> Option<Attachment> {
    if recipes.is_empty() {
        return None;
    }
    let name = recipes[0].name.clone();
    let result = plan(
        &recipes,
        builder,
        root,
        target,
        structure,
        ordinary_single_boundary,
    );
    let plan = match result {
        Ok(plan) => plan,
        Err(reason) => {
            warn(&mut builder.warnings, &name, reason);
            return None;
        }
    };
    // Only the admitted candidate is cloned, once. The census below bounds
    // these snapshots; they allow a failed full-project validation to undo it.
    let attachment = Attachment {
        name: name.clone(),
        root: root.clone(),
        graph: builder.graph.clone(),
        decimal_names: builder.native_decimal_input_names.clone(),
    };
    let previous_next = builder.next_id;
    let message = match plan.message_feed {
        Some(feed) => match builder.scalar_node_at_anchor(feed, &plan.collection) {
            Some(node) => Some(node),
            None => {
                rollback_builder(builder, attachment, previous_next);
                warn(
                    &mut builder.warnings,
                    &name,
                    "message has no scalar expression in the raw item frame",
                );
                return None;
            }
        },
        None => None,
    };
    if builder.graph.nodes.len() > MAX_NODES - 3 || !bounded_expressions(&builder.graph) {
        rollback_builder(builder, attachment, previous_next);
        warn(
            &mut builder.warnings,
            &name,
            "message expression exceeds the bounded item-ordered import shape",
        );
        return None;
    }
    if !owner_scalar_roots(
        root,
        &plan,
        message,
        &builder.graph,
        &builder.sources[0].schema,
    ) {
        rollback_builder(builder, attachment, previous_next);
        warn(
            &mut builder.warnings,
            &name,
            "scalar expressions must read the exact owner item or its current position",
        );
        return None;
    }
    let Some(end) = builder.next_id.checked_add(3) else {
        rollback_builder(builder, attachment, previous_next);
        warn(
            &mut builder.warnings,
            &name,
            "three guard node identities cannot be reserved",
        );
        return None;
    };
    let raise = builder.next_id;
    let yes = raise + 1;
    let guard = raise + 2;
    if (raise..end).any(|id| builder.graph.nodes.contains_key(&id)) {
        rollback_builder(builder, attachment, previous_next);
        warn(
            &mut builder.warnings,
            &name,
            "guard node identities are already owned",
        );
        return None;
    }
    builder.graph.nodes.insert(raise, Node::Raise { message });
    builder.graph.nodes.insert(
        yes,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    builder.graph.nodes.insert(
        guard,
        Node::If {
            condition: plan.predicate,
            then: if plan.throw_true { raise } else { yes },
            else_: if plan.throw_true { yes } else { raise },
        },
    );
    builder.next_id = end;
    scope_at_mut(root, &plan.owner).filter = Some(guard);
    Some(attachment)
}

pub(in crate::import) fn finish(
    project: &mut Project,
    attachment: Option<Attachment>,
    warnings: &mut Vec<String>,
) {
    let Some(attachment) = attachment else {
        return;
    };
    let issues = engine::validate(project);
    if issues.is_empty() {
        return;
    }
    project.root = attachment.root;
    project.graph = attachment.graph;
    project.source_options.mfd_decimal_input_names = attachment.decimal_names;
    warn(
        warnings,
        &attachment.name,
        "attached guard failed complete project validation; no global fallback was used",
    );
}

fn rollback_builder(builder: &mut GraphBuilder<'_>, attachment: Attachment, next_id: NodeId) {
    builder.graph = attachment.graph;
    builder.native_decimal_input_names = attachment.decimal_names;
    builder.next_id = next_id;
    // Attachment is the final materialization phase. No subsequent expression
    // materialization consults the builder's caches after a rejected recipe.
}

struct Plan {
    owner: Vec<usize>,
    collection: Vec<String>,
    predicate: NodeId,
    throw_true: bool,
    message_feed: Option<u32>,
}

fn plan(
    recipes: &[Recipe],
    builder: &GraphBuilder<'_>,
    root: &Scope,
    target: &SchemaComponent,
    structure: roxmltree::Node<'_, '_>,
    ordinary_single_boundary: bool,
) -> Result<Plan, &'static str> {
    if !unique_physical_ports(structure) {
        return Err("physical port identities are duplicated or exceed the import census");
    }
    if recipes.len() != 1
        || structure
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && node.attribute("library") == Some("core")
                    && node.attribute("kind") == Some("18")
            })
            .count()
            != 1
        || !ordinary_single_boundary
        || builder.sources.len() != 1
        || !builder.intermediates.is_empty()
    {
        return Err(
            "requires one exception in one ordinary primary mapping without extra boundaries",
        );
    }
    let source = builder.sources[0];
    if source.format != ComponentFormat::Xml
        || target.format != ComponentFormat::Xml
        || source.is_variable
        || source.is_pass_through
        || target.is_variable
        || target.is_pass_through
        || !local_options(&source.options)
        || !local_options(&target.options)
        || !local_hint(source.input_instance.as_deref())
        || !local_hint(target.output_instance.as_deref())
        || !local_hint(target.input_instance.as_deref())
    {
        return Err("requires local plain primary XML source and target boundaries");
    }
    if !bounded_schema(&source.schema)
        || !bounded_schema(&target.schema)
        || !bounded_scopes(root)
        || !bounded_expressions(&builder.graph)
    {
        return Err("requires bounded closed schemas and ordinary scalar graph/scope shapes");
    }
    let physical_exception = structure
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("core")
                && node.attribute("kind") == Some("18")
        })
        .ok_or("physical exception component is missing")?;
    let physical_exception = read_function(&physical_exception);
    if physical_exception.inputs.len() > 2 || !physical_exception.outputs.is_empty() {
        return Err("exception has unsupported input or output ports");
    }
    let recipe = &recipes[0];
    let throw_input = recipe.throw_input.ok_or("exception has no throw input")?;
    let edges = physical_edges(structure)?;
    let throw_feed = unique_feed(&edges, throw_input)?;
    let index = *builder
        .fn_by_output
        .get(&throw_feed)
        .ok_or("exception must be driven directly by an ordinary filter")?;
    let filter = &builder.fn_components[index];
    if !is_filter(filter) || filter.inputs.len() != 2 || filter.output_pins.len() != 2 {
        return Err("exception requires one ordinary two-output filter");
    }
    let left = filter.output_pins[0].ok_or("filter true output is missing")?;
    let right = filter.output_pins[1].ok_or("filter false output is missing")?;
    if left == right || !matches!(throw_feed, feed if feed == left || feed == right) {
        return Err("exception requires distinct filter output identities");
    }
    let throw_true = throw_feed == left;
    let keep = if throw_true { right } else { left };
    if consumers(&edges, throw_feed) != vec![throw_input] {
        return Err("throw branch has shared or ambiguous consumers");
    }
    let keep_sinks = consumers(&edges, keep);
    let [target_input] = keep_sinks.as_slice() else {
        return Err("opposite filter branch must have exactly one target owner");
    };
    if !target.input_keys.contains(target_input) {
        return Err("opposite filter branch does not drive the primary target structural input");
    }
    let target_path = target
        .ports
        .get(target_input)
        .ok_or("target input has no schema path")?;
    if target_path.is_empty()
        || target.ports.iter().any(|(key, path)| {
            key != target_input && path == target_path && target.input_keys.contains(key)
        })
    {
        return Err("target structural path has multiple physical entry identities");
    }
    let target_node = schema_node_at(&target.schema, target_path)
        .ok_or("target structural path is unresolved")?;
    if !target_node.repeating
        || !matches!(target_node.kind, SchemaKind::Group { .. })
        || !plain_collection(&target.schema, target_path)
    {
        return Err("opposite output must drive a declared repeating target group");
    }
    let raw_input = filter.inputs[0].ok_or("filter node input is missing")?;
    let predicate_input = filter.inputs[1].ok_or("filter predicate input is missing")?;
    let raw = unique_feed(&edges, raw_input)?;
    if consumers(&edges, raw) != vec![raw_input] {
        return Err("raw collection is shared with another structural consumer");
    }
    unique_feed(&edges, predicate_input)?;
    let collection = source
        .ports
        .get(&raw)
        .filter(|_| source.output_keys.contains(&raw))
        .ok_or("filter nodes must come directly from the primary source collection")?;
    if collection.is_empty() || !plain_collection(&source.schema, collection) {
        return Err(
            "raw primary feed must be one declared repeated group without repeated ancestors",
        );
    }
    let feed = builder.resolve_iteration_feed(raw);
    if unsupported_control(&feed).is_some() || feed.sequence_component.is_some() {
        return Err("raw source feed cannot carry sequence controls or generated items");
    }
    let mut found = Vec::new();
    collect_owners(
        root,
        target_path,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut found,
    );
    let [owner] = found.as_slice() else {
        return Err("target structural input has no unique lowered scope owner");
    };
    if owner.is_empty() || !safe_scope_tree(root, owner, 0, false) {
        return Err(
            "owner, ancestors, or independent targets have unsupported evaluation controls",
        );
    }
    let scope = scope_at(root, owner);
    if scope.source() != Some(collection.as_slice()) {
        return Err("target owner does not retain the exact raw primary collection");
    }
    let filter_node = scope
        .filter
        .ok_or("target owner has no matching branch filter")?;
    // The exact physical opposite output was used by build_target_scope. With
    // raw source/no inherited controls, its filter is P or the one not(P)
    // wrapper. Reuse that existing P identity rather than rebuilding it.
    let predicate = if throw_true {
        match builder.graph.nodes.get(&filter_node) {
            Some(Node::Call { function, args }) if function == "not" && args.len() == 1 => args[0],
            _ => return Err("false target branch has no exact original-predicate not wrapper"),
        }
    } else {
        filter_node
    };
    let message_feed = match recipe.message_input {
        Some(input) if edges.iter().any(|(_, to)| *to == input) => {
            Some(unique_feed(&edges, input)?)
        }
        _ => None,
    };
    if let Some(feed) = message_feed
        && !bounded_message_feed(feed, builder, &mut BTreeMap::new(), &mut BTreeSet::new())
    {
        return Err("message physical dependency graph exceeds the bounded scalar shape");
    }
    Ok(Plan {
        owner: owner.clone(),
        collection: collection.clone(),
        predicate,
        throw_true,
        message_feed,
    })
}

fn local_options(options: &mapping::FormatOptions) -> bool {
    let mut ordinary = options.clone();
    ordinary.xml_document = false;
    ordinary.mfd_decimal_input_names.clear();
    ordinary == mapping::FormatOptions::default()
}

fn local_hint(hint: Option<&str>) -> bool {
    hint.is_none_or(|value| {
        !value.trim().is_empty()
            && !value.contains("://")
            && !value.contains('*')
            && !value.contains('?')
    })
}

fn bounded_schema(root: &SchemaNode) -> bool {
    let mut pending = vec![(root, 0)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if count > MAX_NODES
            || depth > MAX_DEPTH
            || node.recursive_ref.is_some()
            || node.name == "element()"
            || node.name == "attribute()"
            || node.xml_wildcard_namespace.is_some()
            || !node.xml_name_alternatives.is_empty()
            || !node.xml_repeating_choices.is_empty()
            || !node.xml_repeating_sequences.is_empty()
            || node.xml_type_alternatives
            || node.json_any
            || node.container_nullable
        {
            return false;
        }
        match &node.kind {
            SchemaKind::Scalar { .. } => {}
            SchemaKind::ScalarUnion { .. } => return false,
            SchemaKind::Group {
                children,
                alternatives,
                dynamic,
                ..
            } => {
                if children.len() > MAX_NODES.saturating_sub(count + pending.len()) {
                    return false;
                }
                if !alternatives.is_empty()
                    || dynamic.is_some()
                    || children
                        .iter()
                        .map(|child| &child.name)
                        .collect::<BTreeSet<_>>()
                        .len()
                        != children.len()
                {
                    return false;
                }
                pending.extend(children.iter().map(|child| (child, depth + 1)));
            }
        }
    }
    true
}

fn bounded_scopes(root: &Scope) -> bool {
    let mut pending = vec![(root, 0)];
    let mut count = 0;
    while let Some((scope, depth)) = pending.pop() {
        count += 1;
        if count > MAX_NODES || depth > MAX_DEPTH || !scope.dynamic_children.is_empty() {
            return false;
        }
        if scope.children.len() > MAX_NODES.saturating_sub(count + pending.len()) {
            return false;
        }
        pending.extend(scope.children.iter().map(|child| (child, depth + 1)));
    }
    true
}

fn bounded_expressions(graph: &Graph) -> bool {
    if graph.nodes.len() > MAX_NODES {
        return false;
    }
    let mut dependencies = 0usize;
    for node in graph.nodes.values() {
        let count = match node {
            Node::Const { .. } | Node::SourceField { .. } | Node::Position { .. } => 0,
            Node::Call { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. } => 1,
            _ => return false,
        };
        dependencies = match dependencies.checked_add(count) {
            Some(count) if count <= MAX_NODES * 4 => count,
            _ => return false,
        };
    }
    // The additional attachment phase uses anchored scalar materialization;
    // accept only ordinary scalar dependencies and cap its recursion depth.
    let mut depths = BTreeMap::new();
    let mut active = BTreeSet::new();
    for id in graph.nodes.keys() {
        if expression_depth(*id, graph, &mut depths, &mut active).is_none() {
            return false;
        }
    }
    true
}

fn expression_depth(
    id: NodeId,
    graph: &Graph,
    depths: &mut BTreeMap<NodeId, usize>,
    active: &mut BTreeSet<NodeId>,
) -> Option<usize> {
    if let Some(depth) = depths.get(&id) {
        return Some(*depth);
    }
    if active.len() >= MAX_DEPTH || !active.insert(id) {
        return None;
    }
    let dependencies = match graph.nodes.get(&id)? {
        Node::Const { .. } | Node::SourceField { .. } | Node::Position { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        _ => return None,
    };
    let mut depth = 1;
    for dependency in dependencies {
        depth = depth.max(expression_depth(dependency, graph, depths, active)?.checked_add(1)?);
    }
    active.remove(&id);
    if depth > MAX_DEPTH {
        return None;
    }
    depths.insert(id, depth);
    Some(depth)
}

/// One proof/memo is shared by P, the optional raw message and every static
/// binding beneath the owner. No other iteration or private scalar frame is
/// admitted by safe_scope_tree, so empty Position is this one current item.
fn owner_scalar_roots(
    root: &Scope,
    plan: &Plan,
    message: Option<NodeId>,
    graph: &Graph,
    source: &SchemaNode,
) -> bool {
    let Some(item) = schema_node_at(source, &plan.collection) else {
        return false;
    };
    let mut roots = vec![plan.predicate];
    roots.extend(message);
    let mut pending = vec![scope_at(root, &plan.owner)];
    while let Some(scope) = pending.pop() {
        if scope.bindings.len() > (MAX_NODES * 4).saturating_sub(roots.len()) {
            return false;
        }
        roots.extend(scope.bindings.iter().map(|binding| binding.node));
        pending.extend(&scope.children);
    }
    let mut depths = BTreeMap::new();
    let mut active = BTreeSet::new();
    let mut dependency_count = 0usize;
    roots.into_iter().all(|id| {
        projected_expression_depth(
            id,
            graph,
            item,
            &plan.collection,
            &mut depths,
            &mut active,
            &mut dependency_count,
        )
        .is_some()
    })
}

fn projected_expression_depth(
    id: NodeId,
    graph: &Graph,
    item: &SchemaNode,
    collection: &[String],
    depths: &mut BTreeMap<NodeId, usize>,
    active: &mut BTreeSet<NodeId>,
    dependency_count: &mut usize,
) -> Option<usize> {
    if let Some(depth) = depths.get(&id) {
        return Some(*depth);
    }
    if depths.len() + active.len() >= MAX_NODES || active.len() >= MAX_DEPTH || !active.insert(id) {
        return None;
    }
    let node = graph.nodes.get(&id)?;
    let dependencies = match node {
        Node::Const { .. } => Vec::new(),
        Node::SourceField { path, frame } => {
            let suffix = match frame {
                Some(frame) if frame == collection => path.as_slice(),
                None if path.starts_with(collection) => &path[collection.len()..],
                // Unframed relative fields and root/sibling broadcasts are not
                // proven by schema existence; do not guess their runtime frame.
                _ => return None,
            };
            if !owner_scalar_path(item, suffix) {
                return None;
            }
            Vec::new()
        }
        Node::Position {
            collection: position,
        } if position.is_empty() || position == collection => Vec::new(),
        Node::Call { .. } | Node::If { .. } | Node::ValueMap { .. } => node.dependencies(),
        _ => return None,
    };
    *dependency_count = dependency_count
        .checked_add(dependencies.len())
        .filter(|count| *count <= MAX_NODES * 4)?;
    let mut depth = 1;
    for dependency in dependencies {
        depth = depth.max(
            projected_expression_depth(
                dependency,
                graph,
                item,
                collection,
                depths,
                active,
                dependency_count,
            )?
            .checked_add(1)?,
        );
    }
    active.remove(&id);
    if depth > MAX_DEPTH {
        return None;
    }
    depths.insert(id, depth);
    Some(depth)
}

fn owner_scalar_path(mut item: &SchemaNode, path: &[String]) -> bool {
    if path.is_empty() || path.len() > MAX_DEPTH {
        return false;
    }
    for name in path {
        let Some(child) = item.child(name) else {
            return false;
        };
        if child.repeating {
            return false;
        }
        item = child;
    }
    matches!(item.kind, SchemaKind::Scalar { .. })
}

fn plain_collection(schema: &SchemaNode, path: &[String]) -> bool {
    if schema.repeating {
        return false;
    }
    let mut node = schema;
    for (index, name) in path.iter().enumerate() {
        let Some(next) = node.child(name) else {
            return false;
        };
        node = next;
        if !matches!(node.kind, SchemaKind::Group { .. })
            || node.repeating != (index + 1 == path.len())
        {
            return false;
        }
    }
    true
}

fn ordinary_controls(scope: &Scope) -> bool {
    matches!(scope.construction, ScopeConstruction::Constructed)
        && scope.iteration_output == IterationOutput::Repeated
        && !scope.has_grouping()
        && !scope.has_sort()
        && !scope.sort_descending
        && scope.windows.is_empty()
        && scope.post_group_filter.is_none()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn safe_scope_tree(scope: &Scope, owner: &[usize], depth: usize, inside: bool) -> bool {
    if depth > MAX_DEPTH || !ordinary_controls(scope) {
        return false;
    }
    if owner.is_empty() && !inside {
        if !matches!(scope.iteration, ScopeIteration::Source(_)) || scope.filter.is_none() {
            return false;
        }
        return scope
            .children
            .iter()
            .all(|child| safe_scope_tree(child, &[], depth + 1, true));
    }
    if !matches!(scope.iteration, ScopeIteration::None)
        || scope.filter.is_some()
        || (!inside && !scope.bindings.is_empty())
    {
        return false;
    }
    if inside {
        return scope
            .children
            .iter()
            .all(|child| safe_scope_tree(child, &[], depth + 1, true));
    }
    scope.children.iter().enumerate().all(|(index, child)| {
        if owner.first() == Some(&index) {
            safe_scope_tree(child, &owner[1..], depth + 1, false)
        } else {
            empty_scope_tree(child)
        }
    })
}

fn empty_scope_tree(scope: &Scope) -> bool {
    ordinary_controls(scope)
        && matches!(scope.iteration, ScopeIteration::None)
        && scope.filter.is_none()
        && scope.bindings.is_empty()
        && scope.children.iter().all(empty_scope_tree)
}

fn collect_owners(
    scope: &Scope,
    wanted: &[String],
    path: &mut Vec<String>,
    indices: &mut Vec<usize>,
    found: &mut Vec<Vec<usize>>,
) {
    if path == wanted {
        found.push(indices.clone());
    }
    for (index, child) in scope.children.iter().enumerate() {
        path.push(child.target_field.clone());
        indices.push(index);
        collect_owners(child, wanted, path, indices, found);
        indices.pop();
        path.pop();
    }
}

fn scope_at<'a>(mut scope: &'a Scope, path: &[usize]) -> &'a Scope {
    for index in path {
        scope = &scope.children[*index];
    }
    scope
}

fn scope_at_mut<'a>(mut scope: &'a mut Scope, path: &[usize]) -> &'a mut Scope {
    for index in path {
        scope = &mut scope.children[*index];
    }
    scope
}

fn unique_physical_ports(structure: roxmltree::Node<'_, '_>) -> bool {
    let mut ports = BTreeSet::new();
    for (count, node) in structure.descendants().enumerate() {
        if count >= MAX_NODES * 16 {
            return false;
        }
        let attributes: &[&str] = if node.has_tag_name("entry") {
            &["inpkey", "outkey"]
        } else if node.has_tag_name("datapoint") {
            &["key"]
        } else {
            &[]
        };
        for attribute in attributes {
            if let Some(value) = node.attribute(*attribute) {
                let Ok(key) = value.parse::<u32>() else {
                    return false;
                };
                if !ports.insert(key) || ports.len() > MAX_NODES {
                    return false;
                }
            }
        }
    }
    true
}

fn bounded_message_feed(
    key: u32,
    builder: &GraphBuilder<'_>,
    depths: &mut BTreeMap<u32, usize>,
    active: &mut BTreeSet<u32>,
) -> bool {
    physical_expression_depth(key, builder, depths, active).is_some()
}

fn physical_expression_depth(
    key: u32,
    builder: &GraphBuilder<'_>,
    depths: &mut BTreeMap<u32, usize>,
    active: &mut BTreeSet<u32>,
) -> Option<usize> {
    if let Some(depth) = depths.get(&key) {
        return Some(*depth);
    }
    if depths.len() + active.len() >= MAX_NODES || active.len() >= MAX_DEPTH || !active.insert(key)
    {
        return None;
    }
    let mut depth = 1;
    if !builder.sources[0].output_keys.contains(&key) {
        let index = *builder.fn_by_output.get(&key)?;
        let component = builder.fn_components.get(index)?;
        if (component.kind != 2 && component.kind != 5) || component.inputs.len() > MAX_DEPTH {
            return None;
        }
        for input in &component.inputs {
            if let Some(feed) = input
                .and_then(|input| builder.edge_from.get(&input))
                .copied()
            {
                depth = depth
                    .max(physical_expression_depth(feed, builder, depths, active)?.checked_add(1)?);
            }
        }
    }
    active.remove(&key);
    if depth > MAX_DEPTH {
        return None;
    }
    depths.insert(key, depth);
    Some(depth)
}

fn physical_edges(structure: roxmltree::Node<'_, '_>) -> Result<Vec<(u32, u32)>, &'static str> {
    let mut edges = Vec::new();
    if let Some(graph) = structure.children().find(|node| node.has_tag_name("graph")) {
        for vertex in graph
            .descendants()
            .filter(|node| node.has_tag_name("vertex"))
        {
            let from = vertex
                .attribute("vertexkey")
                .and_then(|value| value.parse().ok())
                .ok_or("invalid physical source vertex")?;
            for edge in vertex
                .descendants()
                .filter(|node| node.has_tag_name("edge"))
            {
                let to = edge
                    .attribute("vertexkey")
                    .and_then(|value| value.parse().ok())
                    .ok_or("invalid physical target vertex")?;
                edges.push((from, to));
                if edges.len() > MAX_NODES {
                    return Err("physical edge census exceeds item-ordered import budget");
                }
            }
        }
    }
    let connections = structure
        .children()
        .find(|node| node.has_tag_name("connections"))
        .or_else(|| {
            structure.parent().and_then(|parent| {
                parent
                    .children()
                    .find(|node| node.has_tag_name("connections"))
            })
        });
    if let Some(connections) = connections {
        for edge in connections
            .children()
            .filter(|node| node.has_tag_name("edge"))
        {
            let from = edge
                .attribute("from")
                .and_then(|value| value.parse().ok())
                .ok_or("invalid physical source edge")?;
            let to = edge
                .attribute("to")
                .and_then(|value| value.parse().ok())
                .ok_or("invalid physical target edge")?;
            edges.push((from, to));
            if edges.len() > MAX_NODES {
                return Err("physical edge census exceeds item-ordered import budget");
            }
        }
    }
    if edges.len() > MAX_NODES {
        return Err("physical edge census exceeds item-ordered import budget");
    }
    let mut sinks = BTreeSet::new();
    if edges.iter().any(|(_, to)| !sinks.insert(*to)) {
        return Err("physical target pin has more than one edge");
    }
    Ok(edges)
}

fn unique_feed(edges: &[(u32, u32)], sink: u32) -> Result<u32, &'static str> {
    let mut feeds = edges
        .iter()
        .filter(|(_, to)| *to == sink)
        .map(|(from, _)| *from);
    let first = feeds
        .next()
        .ok_or("required physical input is disconnected")?;
    if feeds.next().is_some() {
        return Err("physical input is ambiguous");
    }
    Ok(first)
}

fn consumers(edges: &[(u32, u32)], feed: u32) -> Vec<u32> {
    edges
        .iter()
        .filter(|(from, _)| *from == feed)
        .map(|(_, to)| *to)
        .collect()
}

fn warn(warnings: &mut Vec<String>, name: &str, reason: &str) {
    warnings.push(format!(
        "item-ordered exception `{name}` was not attached: {reason}"
    ));
}
