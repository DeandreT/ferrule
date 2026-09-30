//! Rebuild a guarded SQLite title predicate as a native database where control.
//!
//! A few imported database select statements become a relational table plus a
//! guarded `sql_like` graph predicate. Keeping that predicate as a Ferrule
//! function makes an otherwise native design unusable in the reference editor.
//! This inverse is intentionally limited to the root or one related Person
//! collection and its plain XML employee projection.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, Value};
use mapping::{IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use super::TargetExport;
use super::schema::{DbLayout, KeyAlloc, SideFormat, db_layout};
use super::source::SourceExports;
use crate::MfdError;

pub(super) struct NativeQueryWhere {
    target_port: u32,
    parameter: NodeId,
    residual_filter: Option<NodeId>,
    absorbed: BTreeSet<NodeId>,
}

impl NativeQueryWhere {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
    ) -> Option<Self> {
        if sources.len() != 1
            || !project.failure_rules.is_empty()
            || !project.user_functions.is_empty()
            || project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Position { .. }))
        {
            return None;
        }
        let source = sources.iter().next()?;
        let [target] = targets else { return None };
        if source.format != SideFormat::Db
            || source.dynamic_path_node.is_some()
            || target.format != SideFormat::Xml
            || target.dynamic_json.is_some()
            || target.schema.name != "Company"
            || !plain_scope(target.root)
            || !matches!(target.root.iteration, ScopeIteration::None)
            || target.mapped_scope_plans.get(&[], None).is_some()
            || target
                .mapped_scope_plans
                .get(&["Employees".into()], None)
                .is_some()
            || target
                .mapped_scope_plans
                .get(&["Employees".into(), "Employee".into()], None)
                .is_some()
        {
            return None;
        }
        let [wrapper] = target.root.children.as_slice() else {
            return None;
        };
        let [row] = wrapper.children.as_slice() else {
            return None;
        };
        if wrapper.target_field != "Employees"
            || !plain_scope(wrapper)
            || !matches!(wrapper.iteration, ScopeIteration::None)
            || row.target_field != "Employee"
            || !plain_row(row)
        {
            return None;
        }
        let target_row = target.schema.child("Employees")?.child("Employee")?;
        if !target_row.repeating || !matches!(target_row.kind, SchemaKind::Group { .. }) {
            return None;
        }
        let collection = match &row.iteration {
            ScopeIteration::Source(path) if path.is_empty() => {
                if source.schema.name != "Person"
                    || !source.schema.repeating
                    || !matches!(db_layout(source.schema), Some(DbLayout::Table(_)))
                {
                    return None;
                }
                Vec::new()
            }
            ScopeIteration::Source(path)
                if path.len() == 2 && path[0] == "Department" && path[1] == "Person|ForeignKey" =>
            {
                if source.schema.name != "database"
                    || !matches!(db_layout(source.schema), Some(DbLayout::Database(_)))
                    || !source.schema.child("Department")?.repeating
                    || !source
                        .schema
                        .child("Department")?
                        .child("Person|ForeignKey")?
                        .repeating
                {
                    return None;
                }
                path.clone()
            }
            _ => return None,
        };
        let collection_schema = collection
            .iter()
            .try_fold(source.schema, |schema, segment| schema.child(segment))?;
        source.ports.key_for_abs(&collection)?;
        let title = collection_schema.child("Title")?;
        if title.repeating
            || !matches!(
                title.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
        {
            return None;
        }
        let target_port = target
            .ports
            .key_for_abs(&["Employees".into(), "Employee".into()])?;
        let filter = row.filter?;
        let (residual_filter, like_filter, mut absorbed) =
            if let Some(Node::Call { function, args }) = project.graph.nodes.get(&filter) {
                if function == "and" {
                    let [residual, like] = args.as_slice() else {
                        return None;
                    };
                    guarded_equal_one(project, *residual, &collection)?;
                    (Some(*residual), *like, BTreeSet::from([filter]))
                } else {
                    (None, filter, BTreeSet::new())
                }
            } else {
                (None, filter, BTreeSet::new())
            };
        let (parameter, like_nodes) = guarded_like(project, like_filter, &collection)?;
        absorbed.extend(like_nodes);

        // If a filter node is used by a binding or another root, replacing it
        // with a database control would leave an independently live expression.
        let mut without_like = project.clone();
        without_like.root.children[0].children[0].filter = residual_filter;
        without_like.prune_unreachable_nodes();
        if absorbed
            .iter()
            .any(|node| without_like.graph.nodes.contains_key(node))
        {
            return None;
        }
        Some(Self {
            target_port,
            parameter,
            residual_filter,
            absorbed,
        })
    }

    pub(super) fn absorbed_nodes(&self) -> &BTreeSet<NodeId> {
        &self.absorbed
    }

    pub(super) fn scope_with_residual(&self, root: &Scope) -> Scope {
        let mut root = root.clone();
        root.children[0].children[0].filter = self.residual_filter;
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
        let mut incoming = edges
            .iter()
            .enumerate()
            .filter(|(_, (_, to))| *to == self.target_port);
        let (position, &(collection, _)) = incoming.next().ok_or_else(|| {
            MfdError::Unsupported("native database title where lost its row connection".into())
        })?;
        if incoming.next().is_some() {
            return Err(MfdError::Unsupported(
                "native database title where has multiple row connections".into(),
            ));
        }
        let parameter = node_out_key.get(&self.parameter).copied().ok_or_else(|| {
            MfdError::Unsupported("native database title where lost its pattern value".into())
        })?;
        let collection_input = keys.next();
        let parameter_input = keys.next();
        let output = keys.next();
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"Title where\" library=\"db\" uid=\"{uid}\" kind=\"21\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{collection_input}\"/><datapoint pos=\"1\" key=\"{parameter_input}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/></targets>\n\
             \t\t\t\t\t<data><where condition=\"Title LIKE :sqlparam\"><parameters><parameter name=\"sqlparam\" type=\"string\"/></parameters></where></data>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n",
        );
        edges[position] = (collection, collection_input);
        edges.extend([(parameter, parameter_input), (output, self.target_port)]);
        Ok(())
    }
}

fn plain_scope(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.bindings.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn plain_row(row: &Scope) -> bool {
    row.construction == ScopeConstruction::Constructed
        && row.iteration_output == IterationOutput::Repeated
        && row.filter.is_some()
        && row.post_group_filter.is_none()
        && !row.has_grouping()
        && !row.has_sort()
        && row.windows.is_empty()
        && row.children.is_empty()
        && row.dynamic_bindings.is_empty()
        && row.dynamic_children.is_empty()
        && !row.merge_dynamic_fields
}

fn guarded_like(
    project: &Project,
    filter: NodeId,
    collection: &[String],
) -> Option<(NodeId, BTreeSet<NodeId>)> {
    let nodes = &project.graph.nodes;
    let Node::If {
        condition,
        then,
        else_,
    } = nodes.get(&filter)?
    else {
        return None;
    };
    let Node::Call { function, args } = nodes.get(then)? else {
        return None;
    };
    if function != "sql_like" {
        return None;
    }
    let [title, parameter] = args.as_slice() else {
        return None;
    };
    if !title_field(nodes.get(title)?, collection) {
        return None;
    }
    let supported_pattern = match nodes.get(parameter)? {
        Node::Const {
            value: Value::String(pattern),
        } => substring_pattern(pattern),
        Node::RuntimeParameterDefault {
            ty: ScalarType::String,
            default,
            ..
        } => {
            matches!(nodes.get(default), Some(Node::Const { value: Value::String(pattern) }) if substring_pattern(pattern))
        }
        Node::RuntimeParameter {
            ty: ScalarType::String,
            ..
        } => true,
        _ => false,
    };
    if !supported_pattern {
        return None;
    }
    let Node::Call { function, args } = nodes.get(condition)? else {
        return None;
    };
    if function != "and" {
        return None;
    }
    let [title_exists, parameter_exists] = args.as_slice() else {
        return None;
    };
    if !exists_of(nodes, *title_exists, *title)
        || !exists_of(nodes, *parameter_exists, *parameter)
        || !matches!(
            nodes.get(else_),
            Some(Node::Const {
                value: Value::Bool(false)
            })
        )
    {
        return None;
    }
    Some((
        *parameter,
        [
            filter,
            *condition,
            *then,
            *else_,
            *title_exists,
            *parameter_exists,
        ]
        .into_iter()
        .collect(),
    ))
}

fn guarded_equal_one(project: &Project, filter: NodeId, collection: &[String]) -> Option<()> {
    let nodes = &project.graph.nodes;
    let Node::If {
        condition,
        then,
        else_,
    } = nodes.get(&filter)?
    else {
        return None;
    };
    let Node::Call { function, args } = nodes.get(then)? else {
        return None;
    };
    if function != "equal" {
        return None;
    }
    let [foreign_key, one] = args.as_slice() else {
        return None;
    };
    let Node::SourceField { path, frame } = nodes.get(foreign_key)? else {
        return None;
    };
    if path.len() != 1 || path[0] != "ForeignKey" {
        return None;
    }
    let (foreign_key_schema, expected_frame) = if collection.is_empty() {
        (&project.source, None)
    } else {
        (project.source.child("Department")?, Some(&collection[..1]))
    };
    if frame.as_deref() != expected_frame
        || !foreign_key_schema.child("ForeignKey").is_some_and(|field| {
            !field.repeating
                && matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::Int
                    }
                )
        })
        || !one_operand(nodes, *one)
    {
        return None;
    }
    let Node::Call { function, args } = nodes.get(condition)? else {
        return None;
    };
    if function != "and" {
        return None;
    }
    let [foreign_key_exists, one_exists] = args.as_slice() else {
        return None;
    };
    (exists_of(nodes, *foreign_key_exists, *foreign_key)
        && exists_of(nodes, *one_exists, *one)
        && matches!(
            nodes.get(else_),
            Some(Node::Const {
                value: Value::Bool(false)
            })
        ))
    .then_some(())
}

fn one_operand(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> bool {
    let node = match nodes.get(&id) {
        Some(Node::RuntimeParameterDefault {
            ty: ScalarType::Int,
            default,
            ..
        }) => nodes.get(default),
        node => node,
    };
    matches!(
        node,
        Some(Node::Const {
            value: Value::Int(1)
        })
    ) || matches!(node, Some(Node::Const { value: Value::Float(value) }) if *value == 1.0)
}

fn title_field(node: &Node, collection: &[String]) -> bool {
    matches!(node, Node::SourceField { path, frame }
        if path.len() == 1 && path[0] == "Title"
            && if collection.is_empty() { frame.is_none() } else { frame.as_deref() == Some(collection) })
}

fn exists_of(nodes: &BTreeMap<NodeId, Node>, id: NodeId, value: NodeId) -> bool {
    matches!(nodes.get(&id), Some(Node::Call { function, args }) if function == "exists" && args.as_slice() == [value])
}

fn substring_pattern(value: &str) -> bool {
    let Some(word) = value
        .strip_prefix('%')
        .and_then(|value| value.strip_suffix('%'))
    else {
        return false;
    };
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphabetic())
}
