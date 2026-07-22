use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, Value, XML_TYPE_FIELD};
use mapping::{Graph, IterationOutput, Node, Scope, ScopeConstruction, ScopeIteration};

use crate::MfdError;

use super::schema::{KeyAlloc, PortTree, SideFormat};

#[derive(Default)]
pub(super) struct TargetBranches {
    by_root: BTreeMap<Vec<String>, BranchSet>,
    binding_clones: BTreeMap<Vec<String>, Vec<u32>>,
}

struct BranchSet {
    extra_ports: Vec<BTreeMap<Vec<String>, u32>>,
    iterating: Vec<bool>,
    conditions: Vec<Option<String>>,
    binding_clones: Vec<BTreeMap<Vec<String>, Vec<u32>>>,
}

impl TargetBranches {
    pub(super) fn build(
        schema: &SchemaNode,
        scope: &Scope,
        graph: &Graph,
        keys: &mut KeyAlloc,
        explicit_text: &BTreeSet<Vec<String>>,
    ) -> Self {
        let mut branches = Self::default();
        collect_branches(
            schema,
            scope,
            graph,
            &mut Vec::new(),
            keys,
            explicit_text,
            &mut branches,
        );
        collect_binding_clones(
            schema,
            scope,
            &mut Vec::new(),
            keys,
            &mut branches.binding_clones,
        );
        branches
    }

    pub(super) fn condition(&self, root: &[String], index: usize) -> Option<&str> {
        self.by_root.get(root)?.conditions.get(index)?.as_deref()
    }

    pub(super) fn count(&self, root: &[String]) -> Option<usize> {
        self.by_root.get(root).map(|set| set.extra_ports.len() + 1)
    }

    pub(super) fn iterates(&self, root: &[String], index: usize) -> bool {
        self.by_root
            .get(root)
            .and_then(|set| set.iterating.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn key_for(
        &self,
        base: &PortTree,
        root: &[String],
        index: usize,
        path: &[String],
    ) -> Option<u32> {
        if index == 0 {
            return base.key_for_abs(path);
        }
        self.by_root
            .get(root)?
            .extra_ports
            .get(index - 1)?
            .get(path)
            .copied()
    }

    pub(super) fn binding_key(
        &self,
        base: &PortTree,
        branch: Option<(&[String], usize)>,
        path: &[String],
        occurrence: usize,
    ) -> Option<u32> {
        if occurrence == 0 {
            return branch
                .and_then(|(root, index)| self.key_for(base, root, index, path))
                .or_else(|| base.key_for_abs(path));
        }
        self.binding_clone_keys(branch, path)
            .get(occurrence - 1)
            .copied()
    }

    pub(super) fn binding_clone_keys(
        &self,
        branch: Option<(&[String], usize)>,
        path: &[String],
    ) -> &[u32] {
        let clones = match branch {
            Some((root, index)) => self
                .by_root
                .get(root)
                .and_then(|set| set.binding_clones.get(index))
                .and_then(|clones| clones.get(path)),
            None => self.binding_clones.get(path),
        };
        clones.map(Vec::as_slice).unwrap_or_default()
    }
}

pub(super) fn validate(
    root: &Scope,
    target: &SchemaNode,
    graph: &Graph,
    format: SideFormat,
) -> Result<(), MfdError> {
    validate_scope(root, target, graph, format, &mut Vec::new(), false)
}

fn validate_scope(
    scope: &Scope,
    target: &SchemaNode,
    graph: &Graph,
    format: SideFormat,
    path: &mut Vec<String>,
    inside_concatenation: bool,
) -> Result<(), MfdError> {
    if let Some(segments) = scope.concatenated() {
        if inside_concatenation {
            return Err(unsupported(
                path,
                "nested concatenation is not representable",
            ));
        }
        let output = scope.iteration_output;
        validate_container(scope, path, output)?;
        match format {
            SideFormat::Xml | SideFormat::Xbrl if !path.is_empty() => {
                let node = schema_node_at(target, path)
                    .ok_or_else(|| unsupported(path, "target schema path is missing"))?;
                let compatible = matches!(node.kind, SchemaKind::Group { .. })
                    && (node.repeating && output == IterationOutput::Repeated
                        || !node.repeating && output == IterationOutput::MappedSequence);
                if !compatible {
                    return Err(unsupported(
                        path,
                        "XML concatenation requires a repeating group or a mapped sequence into a non-repeating group",
                    ));
                }
            }
            SideFormat::Edi if !path.is_empty() => {
                let node = schema_node_at(target, path)
                    .ok_or_else(|| unsupported(path, "target schema path is missing"))?;
                if !node.repeating
                    || output != IterationOutput::Repeated
                    || !matches!(node.kind, SchemaKind::Group { .. })
                {
                    return Err(unsupported(
                        path,
                        "EDI concatenation requires a repeating target group",
                    ));
                }
            }
            SideFormat::Db if !path.is_empty() && (format != SideFormat::Db || path.len() == 1) => {
                let node = schema_node_at(target, path)
                    .ok_or_else(|| unsupported(path, "target schema path is missing"))?;
                if !node.repeating || !matches!(node.kind, SchemaKind::Group { .. }) {
                    return Err(unsupported(
                        path,
                        "XML/database concatenation requires a repeating target group",
                    ));
                }
            }
            SideFormat::Csv if path.is_empty() => {
                if segments.iter().filter(|segment| segment.iterates()).count() != 1 {
                    return Err(unsupported(
                        path,
                        "CSV concatenation requires exactly one repeated row segment",
                    ));
                }
            }
            _ => {
                return Err(unsupported(
                    path,
                    "only repeating XML/EDI/database groups and flat CSV roots are supported",
                ));
            }
        }
        let target_node = schema_node_at(target, path)
            .ok_or_else(|| unsupported(path, "target schema path is missing"))?;
        let cloned_ports = schema_node_count(target_node).saturating_mul(segments.len() - 1);
        if segments.len() > 256 || cloned_ports > 65_536 {
            return Err(unsupported(
                path,
                "the cloned target entry tree exceeds the bounded export limit",
            ));
        }
        for segment in segments.iter() {
            validate_segment(segment, target, graph, format, path, output)?;
        }
        return Ok(());
    }

    for child in &scope.children {
        path.push(child.target_field.clone());
        validate_scope(child, target, graph, format, path, inside_concatenation)?;
        path.pop();
    }
    Ok(())
}

fn validate_container(
    scope: &Scope,
    path: &[String],
    output: IterationOutput,
) -> Result<(), MfdError> {
    if scope.construction != ScopeConstruction::Constructed
        || scope.iteration_output != output
        || scope.filter.is_some()
        || scope.post_group_filter.is_some()
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_adjacent_by.is_some()
        || scope.group_ending_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some()
        || !scope.windows.is_empty()
        || !scope.bindings.is_empty()
        || !scope.children.is_empty()
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
    {
        return Err(unsupported(
            path,
            "the concatenation container must contain only its ordered segments",
        ));
    }
    Ok(())
}

fn validate_segment(
    scope: &Scope,
    target: &SchemaNode,
    graph: &Graph,
    format: SideFormat,
    path: &mut Vec<String>,
    output: IterationOutput,
) -> Result<(), MfdError> {
    if !scope.target_field.is_empty()
        || !matches!(
            scope.construction,
            ScopeConstruction::Constructed | ScopeConstruction::CopyCurrentSource
        )
        || scope.iteration_output != output
        || matches!(
            scope.iteration,
            ScopeIteration::InnerJoin { .. } | ScopeIteration::Concatenate(_)
        )
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
    {
        return Err(unsupported(
            path,
            "segments require ordinary or exact current-source construction with matching output cardinality and without joins or dynamic fields",
        ));
    }
    if format == SideFormat::Csv
        && !scope.iterates()
        && (scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_adjacent_by.is_some()
            || scope.group_ending_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || !scope.windows.is_empty()
            || !scope.children.is_empty())
    {
        return Err(unsupported(
            path,
            "CSV singleton segments must be flat and uncontrolled",
        ));
    }
    if output == IterationOutput::MappedSequence {
        let target_node = schema_node_at(target, path)
            .ok_or_else(|| unsupported(path, "target schema path is missing"))?;
        if exact_type_condition(scope, graph, target_node).is_none() {
            return Err(unsupported(
                path,
                "mapped-sequence segments require an exact xsi:type alternative filter and matching type binding",
            ));
        }
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        validate_scope(child, target, graph, format, path, true)?;
        if contains_non_repeated_output(child) {
            return Err(unsupported(
                path,
                "first-item or mapped-sequence output inside a segment is not supported",
            ));
        }
        path.pop();
    }
    Ok(())
}

fn contains_non_repeated_output(scope: &Scope) -> bool {
    scope.iteration_output != IterationOutput::Repeated
        || scope.children.iter().any(contains_non_repeated_output)
}

fn unsupported(path: &[String], reason: &str) -> MfdError {
    let path = if path.is_empty() {
        "<root>".to_string()
    } else {
        path.join("/")
    };
    MfdError::Unsupported(format!(
        "concatenated target scope `{path}` cannot be exported losslessly: {reason}"
    ))
}

fn collect_branches(
    schema: &SchemaNode,
    scope: &Scope,
    graph: &Graph,
    path: &mut Vec<String>,
    keys: &mut KeyAlloc,
    explicit_text: &BTreeSet<Vec<String>>,
    branches: &mut TargetBranches,
) {
    if let Some(segments) = scope.concatenated()
        && let Some(node) = schema_node_at(schema, path)
    {
        let extra_ports = segments
            .iter()
            .skip(1)
            .map(|_| {
                let mut ports = BTreeMap::new();
                allocate_subtree(node, path, None, keys, explicit_text, &mut ports);
                ports
            })
            .collect();
        branches.by_root.insert(
            path.clone(),
            BranchSet {
                extra_ports,
                iterating: segments.iter().map(Scope::iterates).collect(),
                conditions: segments
                    .iter()
                    .map(|segment| exact_type_condition(segment, graph, node))
                    .collect(),
                binding_clones: segments
                    .iter()
                    .map(|segment| {
                        let mut clones = BTreeMap::new();
                        collect_binding_clones(schema, segment, path, keys, &mut clones);
                        clones
                    })
                    .collect(),
            },
        );
        return;
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        collect_branches(schema, child, graph, path, keys, explicit_text, branches);
        path.pop();
    }
}

fn collect_binding_clones(
    schema: &SchemaNode,
    scope: &Scope,
    path: &mut Vec<String>,
    keys: &mut KeyAlloc,
    clones: &mut BTreeMap<Vec<String>, Vec<u32>>,
) {
    if scope.concatenated().is_some() {
        return;
    }
    let pushed = !scope.target_field.is_empty();
    if pushed {
        path.push(scope.target_field.clone());
    }
    let mut occurrences = BTreeMap::<&str, usize>::new();
    for binding in &scope.bindings {
        let occurrence = occurrences.entry(&binding.target_field).or_default();
        if *occurrence > 0 {
            path.push(binding.target_field.clone());
            if schema_node_at(schema, path).is_some_and(|node| {
                node.repeating && !node.attribute && matches!(node.kind, SchemaKind::Scalar { .. })
            }) {
                clones.entry(path.clone()).or_default().push(keys.next());
            }
            path.pop();
        }
        *occurrence += 1;
    }
    for child in &scope.children {
        collect_binding_clones(schema, child, path, keys, clones);
    }
    if pushed {
        path.pop();
    }
}

pub(super) fn exact_type_condition(
    scope: &Scope,
    graph: &Graph,
    target: &SchemaNode,
) -> Option<String> {
    let (condition, _, _) = source_type_condition(scope, graph)?;
    if !target
        .alternatives()
        .iter()
        .any(|alternative| alternative.name == condition)
    {
        return None;
    }
    scope
        .bindings
        .iter()
        .any(|binding| {
            binding.target_field == XML_TYPE_FIELD
                && matches!(
                    graph.nodes.get(&binding.node),
                    Some(Node::Const {
                        value: Value::String(value)
                    }) if value == &condition
                )
        })
        .then_some(condition)
}

pub(super) fn exact_type_marker(scope: &Scope, graph: &Graph, target: &SchemaNode) -> Option<u32> {
    exact_type_condition(scope, graph, target)?;
    source_type_condition(scope, graph).map(|(_, marker, _)| marker)
}

pub(super) fn source_type_condition(
    scope: &Scope,
    graph: &Graph,
) -> Option<(String, u32, Vec<String>)> {
    let filter = scope.filter?;
    let Node::Call { function, args } = graph.nodes.get(&filter)? else {
        return None;
    };
    let [first, second] = args.as_slice() else {
        return None;
    };
    if function != "equal" {
        return None;
    }
    type_condition_operands(graph, *first, *second)
        .or_else(|| type_condition_operands(graph, *second, *first))
}

fn type_condition_operands(
    graph: &Graph,
    marker: u32,
    expected: u32,
) -> Option<(String, u32, Vec<String>)> {
    let Node::SourceField { path, frame } = graph.nodes.get(&marker)? else {
        return None;
    };
    if path.last().is_none_or(|field| field != XML_TYPE_FIELD) {
        return None;
    }
    let Node::Const {
        value: Value::String(expected),
    } = graph.nodes.get(&expected)?
    else {
        return None;
    };
    let mut group = frame.clone().unwrap_or_default();
    group.extend(path[..path.len() - 1].iter().cloned());
    Some((expected.clone(), marker, group))
}

fn allocate_subtree(
    node: &SchemaNode,
    path: &mut Vec<String>,
    shared_key: Option<u32>,
    keys: &mut KeyAlloc,
    explicit_text: &BTreeSet<Vec<String>>,
    ports: &mut BTreeMap<Vec<String>, u32>,
) {
    let key = shared_key.unwrap_or_else(|| keys.next());
    ports.insert(path.clone(), key);
    if let SchemaKind::Group { children, .. } = &node.kind {
        for child in children {
            path.push(child.name.clone());
            let shared_key = (child.text && !explicit_text.contains(path)).then_some(key);
            allocate_subtree(child, path, shared_key, keys, explicit_text, ports);
            path.pop();
        }
    }
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

fn schema_node_count(schema: &SchemaNode) -> usize {
    1 + match &schema.kind {
        SchemaKind::Group { children, .. } => children.iter().map(schema_node_count).sum(),
        SchemaKind::Scalar { .. } => 0,
    }
}
