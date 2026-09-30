//! Restore a bounded joined SQLite SELECT instead of exporting its lowered
//! null-aware multiplication as an internal function component.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaKind, Value};
use mapping::{IterationOutput, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use crate::MfdError;

use super::TargetExport;
use super::schema::{
    DbLayout, KeyAlloc, RenderedSchemaComponent, SideFormat, db_layout, xml_escape,
};
use super::source::SourceExports;

const QUERY_NAME: &str = "SELECT_Statement";

struct Projection {
    name: String,
    expression: String,
    port: u32,
}

enum Threshold {
    Literal(i64),
    Host(NodeId),
}

/// This plan has no fallback path: any departure from the imported joined
/// query shape leaves the existing extension component in the export.
pub(super) struct NativeSelect {
    source_uid: u32,
    source_port: u32,
    computed_node: NodeId,
    computed_port: u32,
    procedure_input: u32,
    catalog_output: u32,
    datasource: String,
    sql: String,
    projections: Vec<Projection>,
    absorbed: BTreeSet<NodeId>,
    threshold_input: Option<(NodeId, u32)>,
}

impl NativeSelect {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
        mfd_path: &Path,
        keys: &mut KeyAlloc,
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
            || !matches!(db_layout(source.schema), Some(DbLayout::Table(_)))
            || target.format != SideFormat::Csv
            || target.dynamic_json.is_some()
            || target.mapped_scope_plans.get(&[], None).is_some()
            || !plain_csv_scope(target.root)
            || !source.schema.repeating
            || !safe_identifier(&source.schema.name)
        {
            return None;
        }
        let relation = sole_relation(source.schema)?;
        let (joined_table, relation_column) = relation.name.split_once('|')?;
        if !safe_identifier(joined_table) || !safe_identifier(relation_column) {
            return None;
        }
        let db_path = database_path(source.path?, mfd_path);
        if !db_path.is_file() {
            return None;
        }
        let columns = format_db::resolve_foreign_key_columns(
            &db_path,
            &source.schema.name,
            joined_table,
            relation_column,
        )
        .ok()?;
        if columns.parent_column != relation_column
            || !scalar_field(relation, &columns.child_column).is_some_and(is_integer)
            || format_db::resolve_foreign_key_relation(
                &db_path,
                &source.schema.name,
                &columns.parent_column,
                joined_table,
                &columns.child_column,
            )
            .ok()?
            .side
                != format_db::ForeignKeySide::Parent
        {
            return None;
        }

        let nodes = &project.graph.nodes;
        let filter = target.root.filter?;
        let (predicate, threshold, filter_nodes) =
            match_query_filter(nodes, filter, &relation.name, &columns.child_column)?;
        if !scalar_field(source.schema, &predicate).is_some_and(is_integer) {
            return None;
        }
        let mut without_filter = project.clone();
        without_filter.root.filter = None;
        without_filter.prune_unreachable_nodes();
        if filter_nodes
            .iter()
            .any(|id| without_filter.graph.nodes.contains_key(id))
        {
            return None;
        }

        let mut computed = None;
        let mut projections = Vec::new();
        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for binding in &target.root.bindings {
            if !safe_identifier(&binding.target_field) {
                return None;
            }
            let node = nodes.get(&binding.node)?;
            if let Node::Call { function, args } = node
                && function == "sqlite_multiply"
            {
                let [left, right] = args.as_slice() else {
                    return None;
                };
                let left = source_field_path(nodes, *left)?;
                let right = source_field_path(nodes, *right)?;
                let left_sql = qualified_column(source.schema, relation, joined_table, left)?;
                let right_sql = qualified_column(source.schema, relation, joined_table, right)?;
                if !left_sql.1 || !right_sql.1 || left_sql.0 == right_sql.0 {
                    return None;
                }
                if computed.replace(binding.node).is_some() {
                    return None;
                }
                if !names.insert(binding.target_field.to_ascii_lowercase()) {
                    return None;
                }
                projections.push(Projection {
                    name: binding.target_field.clone(),
                    expression: format!("({} * {})", left_sql.0, right_sql.0),
                    port: 0,
                });
                continue;
            }
            let field = if let Some(path) = source_field_path(nodes, binding.node) {
                path
            } else if let Node::Call { function, args } = node
                && function == "date_from_datetime"
            {
                let [input] = args.as_slice() else {
                    return None;
                };
                source_field_path(nodes, *input)?
            } else {
                return None;
            };
            if !paths.insert(field.to_vec()) {
                continue;
            }
            let (expression, _) = qualified_column(source.schema, relation, joined_table, field)?;
            let name = field.last()?.clone();
            if !names.insert(name.to_ascii_lowercase()) {
                return None;
            }
            projections.push(Projection {
                name,
                expression,
                port: source.ports.key_for_abs(field)?,
            });
        }
        let computed_node = computed?;
        if projections.len() < 2 {
            return None;
        }
        let mut without_computed = project.clone();
        without_computed.root.filter = None;
        without_computed
            .root
            .bindings
            .retain(|binding| binding.node != computed_node);
        without_computed.prune_unreachable_nodes();
        if without_computed.graph.nodes.contains_key(&computed_node) {
            return None;
        }
        let source_port = source.ports.key_for_abs(&[])?;
        let threshold_input = match threshold {
            Threshold::Literal(_) => None,
            Threshold::Host(node) => Some((node, keys.next())),
        };
        let threshold_sql = match threshold {
            Threshold::Literal(value) => value.to_string(),
            Threshold::Host(_) => ":HostThreshold".to_string(),
        };
        let sql = format!(
            "SELECT {} FROM {} INNER JOIN {} ON {}.{} = {}.{} WHERE {}.{} > {threshold_sql}",
            projections
                .iter()
                .map(|projection| format!("{} AS {}", projection.expression, projection.name))
                .collect::<Vec<_>>()
                .join(", "),
            source.schema.name,
            joined_table,
            source.schema.name,
            columns.parent_column,
            joined_table,
            columns.child_column,
            source.schema.name,
            predicate,
        );
        let computed_port = keys.next();
        let procedure_input = keys.next();
        let catalog_output = keys.next();
        projections
            .iter_mut()
            .find(|projection| projection.port == 0)?
            .port = computed_port;
        let mut absorbed = filter_nodes;
        absorbed.insert(computed_node);
        Some(Self {
            source_uid: source.component_uid,
            source_port,
            computed_node,
            computed_port,
            procedure_input,
            catalog_output,
            datasource: super::schema::db_datasource_name(source.path),
            sql,
            projections,
            absorbed,
            threshold_input,
        })
    }

    pub(super) fn absorbed_nodes(&self) -> &BTreeSet<NodeId> {
        &self.absorbed
    }

    pub(super) fn seed_alias(&self, node_out_key: &mut BTreeMap<NodeId, u32>) {
        node_out_key.insert(self.computed_node, self.computed_port);
    }

    pub(super) fn scope_without_filter(&self, root: &Scope) -> Scope {
        let mut root = root.clone();
        root.filter = None;
        root
    }

    pub(super) fn datasource(&self) -> &str {
        &self.datasource
    }

    pub(super) fn owns_source(&self, uid: u32) -> bool {
        self.source_uid == uid
    }

    pub(super) fn local_view(&self) -> String {
        let parameters = if self.threshold_input.is_some() {
            "\t\t\t\t\t\t<Parameters><Parameter name=\"HostThreshold\" type=\"integer\"/></Parameters>\n"
        } else {
            ""
        };
        format!(
            "\t\t\t\t\t<LocalViewStorage><LocalViewElement SQL=\"{}\">\n\
             \t\t\t\t\t\t<PathElement Name=\"main\" Kind=\"Database\"/>\n\
             \t\t\t\t\t\t<PathElement Name=\"{QUERY_NAME}\" Kind=\"Select Statement\"/>\n\
             {parameters}\
             \t\t\t\t\t</LocalViewElement></LocalViewStorage>\n",
            xml_escape(&self.sql)
        )
    }

    pub(super) fn render_source(&self) -> RenderedSchemaComponent {
        let parameter_entry = self.threshold_input.map_or_else(String::new, |(_, port)| {
            format!("<entry name=\"{QUERY_NAME}\"><entry name=\"HostThreshold\" type=\"attribute\" inpkey=\"{port}\"/></entry>")
        });
        let mut entries = String::new();
        for projection in &self.projections {
            let _ = writeln!(
                entries,
                "\t\t\t\t\t\t\t\t\t<entry name=\"{}\" type=\"attribute\" outkey=\"{}\"/>",
                xml_escape(&projection.name),
                projection.port,
            );
        }
        let xml = format!(
            "\t\t\t\t<component name=\"{QUERY_NAME}\" library=\"db\" uid=\"{}\" kind=\"28\">\n\
             \t\t\t\t\t<view rbx=\"300\" rby=\"240\"/>\n\
             \t\t\t\t\t<data>\n\
             \t\t\t\t\t\t<root><entry name=\"procedure\" inpkey=\"{}\"/>{parameter_entry}</root>\n\
             \t\t\t\t\t\t<root><entry name=\"{QUERY_NAME}\" outkey=\"{}\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t<entry name=\"{QUERY_NAME}\" expanded=\"1\">\n\
             {entries}\
             \t\t\t\t\t\t\t</entry></entry></root>\n\
             \t\t\t\t\t</data>\n\
             \t\t\t\t</component>\n",
            self.source_uid, self.procedure_input, self.source_port
        );
        RenderedSchemaComponent {
            xml,
            siblings: Vec::new(),
        }
    }

    pub(super) fn render_catalog(
        &self,
        uid: &mut u32,
        components: &mut String,
        edges: &mut Vec<(u32, u32)>,
        node_out_key: &BTreeMap<NodeId, u32>,
    ) -> Result<(), MfdError> {
        let threshold_edge = if let Some((node, input)) = self.threshold_input {
            let output = node_out_key.get(&node).copied().ok_or_else(|| {
                MfdError::Unsupported(
                    "native database query host threshold has no rendered output port".into(),
                )
            })?;
            Some((output, input))
        } else {
            None
        };
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"database\" library=\"db\" uid=\"{uid}\" kind=\"15\">\n\
             \t\t\t\t\t<data><root><entry name=\"document\"><entry name=\"{QUERY_NAME}\" type=\"routine\" outkey=\"{}\"/></entry></root>\n\
             \t\t\t\t\t\t<database ref=\"{}\"><data><selections><selection>\n\
             \t\t\t\t\t\t\t<PathElement Name=\"main\" Kind=\"Database\"/>\n\
             \t\t\t\t\t\t\t<PathElement Name=\"{QUERY_NAME}\" Kind=\"Select Statement\"/>\n\
             \t\t\t\t\t\t</selection></selections></data></database>\n\
             \t\t\t\t\t</data>\n\
             \t\t\t\t</component>\n",
            self.catalog_output,
            xml_escape(&self.datasource)
        );
        edges.push((self.catalog_output, self.procedure_input));
        if let Some(edge) = threshold_edge {
            edges.push(edge);
        }
        Ok(())
    }
}

fn plain_csv_scope(scope: &Scope) -> bool {
    scope.target_field.is_empty()
        && matches!(scope.iteration, ScopeIteration::Source(ref path) if path.is_empty())
        && scope.iteration_output == IterationOutput::Repeated
        && scope.construction == ScopeConstruction::Constructed
        && scope.filter.is_some()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && !scope.bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn sole_relation(schema: &ir::SchemaNode) -> Option<&ir::SchemaNode> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    let mut relations = children
        .iter()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }));
    let relation = relations.next()?;
    if relations.next().is_some() || !relation.repeating || relation.database_relation.is_some() {
        return None;
    }
    Some(relation)
}

fn scalar_field<'a>(schema: &'a ir::SchemaNode, name: &str) -> Option<&'a ir::SchemaNode> {
    let field = schema.child(name)?;
    (!field.repeating && matches!(field.kind, SchemaKind::Scalar { .. })).then_some(field)
}

fn is_integer(field: &ir::SchemaNode) -> bool {
    matches!(
        field.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    )
}

fn qualified_column(
    source: &ir::SchemaNode,
    relation: &ir::SchemaNode,
    joined_table: &str,
    path: &[String],
) -> Option<(String, bool)> {
    let (table, field) = match path {
        [field] => (source, field),
        [parent, field] if *parent == relation.name => (relation, field),
        _ => return None,
    };
    let scalar = scalar_field(table, field)?;
    if !safe_identifier(field) {
        return None;
    }
    let numeric = matches!(
        scalar.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int | ScalarType::Float
        }
    );
    let table_name = if std::ptr::eq(table, source) {
        &source.name
    } else {
        joined_table
    };
    Some((format!("{table_name}.{field}"), numeric))
}

fn source_field_path(nodes: &BTreeMap<NodeId, Node>, id: NodeId) -> Option<&[String]> {
    let Node::SourceField { path, frame: None } = nodes.get(&id)? else {
        return None;
    };
    Some(path)
}

fn match_query_filter(
    nodes: &BTreeMap<NodeId, Node>,
    filter: NodeId,
    relation: &str,
    child_key: &str,
) -> Option<(String, Threshold, BTreeSet<NodeId>)> {
    let [relation_exists, predicate_if] = call_args(nodes, filter, "and", 2)? else {
        return None;
    };
    let [relation_field] = call_args(nodes, *relation_exists, "exists", 1)? else {
        return None;
    };
    if source_field_path(nodes, *relation_field)? != [relation, child_key] {
        return None;
    }
    let Node::If {
        condition,
        then,
        else_,
    } = nodes.get(predicate_if)?
    else {
        return None;
    };
    let [predicate_exists, constant_exists] = call_args(nodes, *condition, "and", 2)? else {
        return None;
    };
    let [predicate, constant] = call_args(nodes, *then, "greater_than", 2)? else {
        return None;
    };
    let [predicate_check] = call_args(nodes, *predicate_exists, "exists", 1)? else {
        return None;
    };
    let [constant_check] = call_args(nodes, *constant_exists, "exists", 1)? else {
        return None;
    };
    if predicate != predicate_check
        || constant != constant_check
        || !matches!(
            nodes.get(else_),
            Some(Node::Const {
                value: Value::Bool(false)
            })
        )
    {
        return None;
    }
    let predicate = source_field_path(nodes, *predicate)?;
    let [predicate] = predicate else {
        return None;
    };
    let threshold = match nodes.get(constant)? {
        Node::Const {
            value: Value::Int(value),
        } if *value >= 0 => Threshold::Literal(*value),
        Node::RuntimeParameterDefault {
            ty: ScalarType::Int,
            default,
            ..
        } if matches!(nodes.get(default), Some(Node::Const { value: Value::Int(value) }) if *value >= 0) => {
            Threshold::Host(*constant)
        }
        Node::RuntimeParameter {
            ty: ScalarType::Int,
            ..
        } => Threshold::Host(*constant),
        _ => return None,
    };
    if !safe_identifier(predicate) {
        return None;
    }
    let mut absorbed = BTreeSet::from([
        filter,
        *relation_exists,
        *predicate_if,
        *condition,
        *then,
        *else_,
        *predicate_exists,
        *constant_exists,
        *constant,
    ]);
    if matches!(threshold, Threshold::Host(_)) {
        absorbed.remove(constant);
    }
    Some((predicate.clone(), threshold, absorbed))
}

fn call_args<'a>(
    nodes: &'a BTreeMap<NodeId, Node>,
    id: NodeId,
    name: &str,
    count: usize,
) -> Option<&'a [NodeId]> {
    let Node::Call { function, args } = nodes.get(&id)? else {
        return None;
    };
    (function == name && args.len() == count).then_some(args)
}

fn safe_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn database_path(stored: &str, mfd_path: &Path) -> PathBuf {
    let stored = Path::new(stored);
    if stored.is_absolute() {
        stored.to_path_buf()
    } else {
        mfd_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(stored)
    }
}
