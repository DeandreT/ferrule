use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use mapping::{IterationOutput, Scope, ScopeConstruction, ScopeIteration};

use crate::MfdError;

use super::schema::{KeyAlloc, PortTree, SideFormat};

#[derive(Default)]
pub(super) struct TargetBranches {
    by_root: BTreeMap<Vec<String>, BranchSet>,
}

struct BranchSet {
    extra_ports: Vec<BTreeMap<Vec<String>, u32>>,
    iterating: Vec<bool>,
}

impl TargetBranches {
    pub(super) fn build(
        schema: &SchemaNode,
        scope: &Scope,
        keys: &mut KeyAlloc,
        explicit_text: &BTreeSet<Vec<String>>,
    ) -> Self {
        let mut branches = Self::default();
        collect_branches(
            schema,
            scope,
            &mut Vec::new(),
            keys,
            explicit_text,
            &mut branches,
        );
        branches
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
}

pub(super) fn validate(
    root: &Scope,
    target: &SchemaNode,
    format: SideFormat,
) -> Result<(), MfdError> {
    validate_scope(root, target, format, &mut Vec::new(), false)
}

fn validate_scope(
    scope: &Scope,
    target: &SchemaNode,
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
        validate_container(scope, path)?;
        match format {
            SideFormat::Xml | SideFormat::Xbrl | SideFormat::Db
                if !path.is_empty() && (format != SideFormat::Db || path.len() == 1) =>
            {
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
                    "only repeating XML/database groups and flat CSV roots are supported",
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
            validate_segment(segment, target, format, path)?;
        }
        return Ok(());
    }

    for child in &scope.children {
        path.push(child.target_field.clone());
        validate_scope(child, target, format, path, inside_concatenation)?;
        path.pop();
    }
    Ok(())
}

fn validate_container(scope: &Scope, path: &[String]) -> Result<(), MfdError> {
    if scope.construction != ScopeConstruction::Constructed
        || scope.iteration_output != IterationOutput::Repeated
        || scope.filter.is_some()
        || scope.group_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some()
        || scope.take.is_some()
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
    format: SideFormat,
    path: &mut Vec<String>,
) -> Result<(), MfdError> {
    if !scope.target_field.is_empty()
        || scope.construction != ScopeConstruction::Constructed
        || scope.iteration_output != IterationOutput::Repeated
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
            "segments require ordinary repeated construction without joins or dynamic fields",
        ));
    }
    if format == SideFormat::Csv
        && !scope.iterates()
        && (scope.filter.is_some()
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
            || !scope.children.is_empty())
    {
        return Err(unsupported(
            path,
            "CSV singleton segments must be flat and uncontrolled",
        ));
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        validate_scope(child, target, format, path, true)?;
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
            },
        );
        return;
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        collect_branches(schema, child, path, keys, explicit_text, branches);
        path.pop();
    }
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
