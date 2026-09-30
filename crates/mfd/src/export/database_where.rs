//! A narrowly reversible SQLite where control for a direct XML row mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, Value};
use mapping::{IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use super::TargetExport;
use super::schema::{DbLayout, KeyAlloc, SideFormat, db_layout, xml_escape};
use super::source::SourceExports;
use crate::MfdError;

pub(super) struct NativeWhere {
    source_port: u32,
    target_port: u32,
    parameter: NodeId,
    column: String,
    descending: bool,
    absorbed_nodes: BTreeSet<NodeId>,
}

impl NativeWhere {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
    ) -> Option<Self> {
        if sources.len() != 1 {
            return None;
        }
        let source = sources.iter().next()?;
        let [target] = targets else { return None };
        if source.format != SideFormat::Db
            || source.dynamic_path_node.is_some()
            || !matches!(db_layout(source.schema), Some(DbLayout::Table(_)))
            || target.format != SideFormat::Xml
            || target.dynamic_json.is_some()
            || !project.failure_rules.is_empty()
            || !plain_root(target.root)
        {
            return None;
        }
        let [row] = target.root.children.as_slice() else {
            return None;
        };
        if !plain_row(row)
            || !matches!(row.iteration, ScopeIteration::Source(ref path) if path.is_empty())
            || target.mapped_scope_plans.get(&[], None).is_some()
            || target
                .mapped_scope_plans
                .get(std::slice::from_ref(&row.target_field), None)
                .is_some()
        {
            return None;
        }
        let target_row = target.schema.child(&row.target_field)?;
        if !target_row.repeating || !matches!(target_row.kind, SchemaKind::Group { .. }) {
            return None;
        }
        let source_port = sources.key_for_abs(&[])?;
        let target_port = target
            .ports
            .key_for_abs(std::slice::from_ref(&row.target_field))?;
        let filter = row.filter?;
        let sort = row.sort_by?;
        if !row.sort_then_by.is_empty()
            || project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Position { .. }))
        {
            return None;
        }
        let Node::If {
            condition,
            then,
            else_,
        } = project.graph.nodes.get(&filter)?
        else {
            return None;
        };
        let Node::Call {
            function: comparison,
            args: comparison_args,
        } = project.graph.nodes.get(then)?
        else {
            return None;
        };
        if comparison != "sql_like" {
            return None;
        }
        let [value, parameter] = comparison_args.as_slice() else {
            return None;
        };
        if sort != *value {
            return None;
        }
        let Node::SourceField { path, frame: None } = project.graph.nodes.get(value)? else {
            return None;
        };
        let [column] = path.as_slice() else {
            return None;
        };
        if !safe_identifier(column)
            || !source.schema.child(column).is_some_and(|field| {
                !field.repeating
                    && matches!(
                        field.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::String
                        }
                    )
            })
        {
            return None;
        }
        let Node::Call {
            function: both,
            args: both_args,
        } = project.graph.nodes.get(condition)?
        else {
            return None;
        };
        if both != "and" {
            return None;
        }
        let [value_exists, parameter_exists] = both_args.as_slice() else {
            return None;
        };
        if !exists_of(&project.graph.nodes, *value_exists, *value)
            || !exists_of(&project.graph.nodes, *parameter_exists, *parameter)
            || !matches!(
                project.graph.nodes.get(else_),
                Some(Node::Const {
                    value: Value::Bool(false)
                })
            )
        {
            return None;
        }

        // Retain the connected expression on the native parameter pin, including
        // a named optional host input. Its literal default keeps this inverse
        // bounded to the same prefix family as ordinary constant patterns.
        if !prefix_pattern(&project.graph.nodes, *parameter) {
            return None;
        }
        if !row
            .bindings
            .iter()
            .all(|binding| native_projection(&project.graph.nodes, binding.node))
        {
            return None;
        }

        let absorbed_nodes = [
            filter,
            *condition,
            *then,
            *else_,
            *value_exists,
            *parameter_exists,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if absorbed_nodes.contains(value) || absorbed_nodes.contains(parameter) {
            return None;
        }
        let mut without_filter = project.clone();
        without_filter.root.children[0].filter = None;
        without_filter.prune_unreachable_nodes();
        if absorbed_nodes
            .iter()
            .any(|id| without_filter.graph.nodes.contains_key(id))
        {
            return None;
        }
        Some(Self {
            source_port,
            target_port,
            parameter: *parameter,
            column: column.clone(),
            descending: row.sort_descending,
            absorbed_nodes,
        })
    }

    pub(super) fn absorbed_nodes(&self) -> &BTreeSet<NodeId> {
        &self.absorbed_nodes
    }

    pub(super) fn scope_without_controls(&self, root: &Scope) -> Scope {
        let mut root = root.clone();
        root.children[0].filter = None;
        root.children[0].sort_by = None;
        root
    }

    pub(super) fn connect(
        &self,
        node_out_key: &BTreeMap<NodeId, u32>,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        components: &mut String,
        edges: &mut Vec<(u32, u32)>,
    ) -> Result<(), MfdError> {
        let position = edges
            .iter()
            .position(|edge| *edge == (self.source_port, self.target_port))
            .ok_or_else(|| {
                MfdError::Unsupported("native database where lost its direct row connection".into())
            })?;
        if edges[position + 1..].contains(&(self.source_port, self.target_port)) {
            return Err(MfdError::Unsupported(
                "native database where has more than one direct row connection".into(),
            ));
        }
        let parameter_port = node_out_key.get(&self.parameter).copied().ok_or_else(|| {
            MfdError::Unsupported(
                "native database where parameter expression was not exported".into(),
            )
        })?;
        let collection_input = keys.next();
        let parameter_input = keys.next();
        let output = keys.next();
        *uid += 1;
        let order = if self.descending { "DESC" } else { "ASC" };
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"{}\" library=\"db\" uid=\"{uid}\" kind=\"21\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{collection_input}\"/><datapoint pos=\"1\" key=\"{parameter_input}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
             \t\t\t\t\t<data><where condition=\"{} LIKE :sqlparam\" order=\"{} {order}\"><parameters><parameter name=\"sqlparam\" type=\"string\"/></parameters></where></data>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n",
            xml_escape(&self.column),
            xml_escape(&self.column),
            xml_escape(&self.column),
        );
        edges[position] = (self.source_port, collection_input);
        edges.extend([
            (parameter_port, parameter_input),
            (output, self.target_port),
        ]);
        Ok(())
    }
}

fn plain_root(root: &Scope) -> bool {
    matches!(root.iteration, ScopeIteration::None)
        && root.construction == ScopeConstruction::Constructed
        && root.filter.is_none()
        && root.post_group_filter.is_none()
        && !root.has_grouping()
        && !root.has_sort()
        && root.windows.is_empty()
        && root.bindings.is_empty()
        && root.dynamic_bindings.is_empty()
        && root.dynamic_children.is_empty()
        && !root.merge_dynamic_fields
}

fn plain_row(row: &Scope) -> bool {
    row.construction == ScopeConstruction::Constructed
        && row.iteration_output == IterationOutput::Repeated
        && row.post_group_filter.is_none()
        && !row.has_grouping()
        && row.windows.is_empty()
        && row.children.is_empty()
        && row.dynamic_bindings.is_empty()
        && row.dynamic_children.is_empty()
        && !row.merge_dynamic_fields
}

fn exists_of(nodes: &BTreeMap<NodeId, Node>, id: NodeId, value: NodeId) -> bool {
    matches!(nodes.get(&id), Some(Node::Call { function, args }) if function == "exists" && args.as_slice() == [value])
}

fn literal_string(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> Option<String> {
    match nodes.get(&id)? {
        Node::Const {
            value: Value::String(value),
        } => Some(value.clone()),
        Node::Call { function, args } if function == "string" => {
            let [part] = args.as_slice() else { return None };
            literal_string(nodes, *part)
        }
        Node::Call { function, args } if function == "concat" => {
            args.iter().try_fold(String::new(), |mut value, id| {
                value.push_str(&literal_string(nodes, *id)?);
                Some(value)
            })
        }
        _ => None,
    }
}

fn prefix_pattern(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> bool {
    if let Some(pattern) = literal_string(nodes, id) {
        return pattern.strip_suffix('%').is_some_and(ascii_prefix);
    }
    let Some(Node::Call { function, args }) = nodes.get(&id) else {
        return false;
    };
    let [prefix, suffix] = args.as_slice() else {
        return false;
    };
    function == "concat"
        && literal_string(nodes, *suffix).as_deref() == Some("%")
        && optional_prefix(nodes, *prefix)
}

fn optional_prefix(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> bool {
    match nodes.get(&id) {
        Some(Node::Call { function, args }) if function == "string" => {
            matches!(args.as_slice(), [input] if optional_prefix(nodes, *input))
        }
        Some(Node::RuntimeParameterDefault {
            ty: ScalarType::String,
            default,
            ..
        }) => literal_string(nodes, *default).is_some_and(|value| ascii_prefix(&value)),
        _ => false,
    }
}

fn ascii_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn native_projection(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> bool {
    match nodes.get(&id) {
        Some(Node::SourceField { frame: None, .. } | Node::Const { .. }) => true,
        Some(Node::Call { function, args }) if matches!(function.as_str(), "concat" | "string") => {
            args.iter().all(|input| native_projection(nodes, *input))
        }
        _ => false,
    }
}

fn safe_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
