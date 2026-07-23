use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FormatOptions, Node, NodeId};

use super::GraphBuilder;
use super::function::{FnComponent, is_input, parse_constant};
use super::schema::{ComponentFormat, SchemaComponent, entry_key_sets, parse_u32};
use super::source::SourcePath;

mod catalog;
mod correlated;
mod joined;
mod sql;

pub(super) use catalog::read_embedded_catalog;
use sql::{Parser, valid_identifier};

#[derive(Clone)]
pub(super) struct DbQuery {
    name: String,
    collection: Vec<String>,
    predicates: Vec<QueryPredicate>,
    order: Option<QueryOrder>,
    cardinality: QueryCardinality,
    required_paths: Vec<Vec<String>>,
    computed_ports: BTreeMap<u32, DbComputedExpression>,
}

#[derive(Clone)]
enum DbComputedExpression {
    Multiply {
        left: Vec<String>,
        right: Vec<String>,
        ty: ScalarType,
    },
}

#[cfg(test)]
pub(super) fn at_most_one_query_for_test(collection: Vec<String>) -> DbQuery {
    DbQuery {
        name: "query".to_string(),
        collection,
        predicates: Vec::new(),
        order: None,
        cardinality: QueryCardinality::AtMostOne,
        required_paths: Vec::new(),
        computed_ports: BTreeMap::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryCardinality {
    Many,
    AtMostOne,
}

#[derive(Clone)]
struct QueryPredicate {
    column: String,
    operator: QueryOperator,
    operand: QueryOperand,
}

#[derive(Clone, Copy)]
enum QueryOperator {
    Equal,
    Like,
    Greater,
}

#[derive(Clone)]
enum QueryOperand {
    Parameter {
        name: String,
        input_key: u32,
        ty: ScalarType,
    },
    Literal(Value),
    Correlated,
}

#[derive(Clone)]
struct QueryOrder {
    column: String,
    descending: bool,
}

struct ParsedQuery {
    table: String,
    projection: QueryProjection,
    predicates: Vec<ParsedPredicate>,
    order: Option<QueryOrder>,
    cardinality: QueryCardinality,
}

enum QueryProjection {
    All,
    Columns(Vec<String>),
}

struct ParsedPredicate {
    column: String,
    operator: QueryOperator,
    operand: ParsedOperand,
}

enum ParsedOperand {
    Parameter(String),
    Literal(Value),
}

pub(super) enum DbControlError {
    Query(String),
    Where { name: String, reason: String },
}

impl DbControlError {
    pub(super) fn warning(&self, target_path: &[String]) -> String {
        match self {
            Self::Query(reason) => format!(
                "database query source feeding `{}` is unsupported: {reason}; iteration skipped",
                target_path.join("/")
            ),
            Self::Where { name, reason } => format!(
                "database where/order component `{name}` is unsupported: {reason}; iteration into `{}` skipped",
                target_path.join("/")
            ),
        }
    }
}

pub(super) fn is_routine_catalog(
    component: &roxmltree::Node<'_, '_>,
    siblings: &roxmltree::Node<'_, '_>,
) -> bool {
    if component.attribute("kind") != Some("15") {
        return false;
    }
    let routines = component
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("routine"))
        .collect::<Vec<_>>();
    let queries = siblings
        .children()
        .filter(|sibling| {
            sibling.has_tag_name("component")
                && sibling.attribute("library") == Some("db")
                && sibling.attribute("kind") == Some("28")
        })
        .filter_map(|query| query.attribute("name"))
        .collect::<Vec<_>>();
    let matched = |routine: &roxmltree::Node<'_, '_>| {
        routine.attribute("name").is_some_and(|routine| {
            queries
                .iter()
                .any(|query| same_query_identity(routine, query))
        })
    };
    if routines.is_empty() || routines.iter().any(|routine| !matched(routine)) {
        return false;
    }
    component
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("table"))
        .all(|table| {
            routines.iter().any(|routine| {
                matched(routine) && routine.ancestors().any(|ancestor| ancestor == table)
            })
        })
}

pub(super) fn same_query_identity(left: &str, right: &str) -> bool {
    let Some((left_base, left_parameter)) = query_identity(left) else {
        return false;
    };
    let Some((right_base, right_parameter)) = query_identity(right) else {
        return false;
    };
    left_base == right_base
        && (left_parameter == right_parameter
            || left_parameter.is_none()
            || right_parameter.is_none())
}

fn query_identity(name: &str) -> Option<(&str, Option<&str>)> {
    match name.split_once('|') {
        None if !name.is_empty() => Some((name, None)),
        Some((base, parameter))
            if !base.is_empty() && !parameter.is_empty() && !parameter.contains('|') =>
        {
            Some((base, Some(parameter)))
        }
        _ => None,
    }
}

pub(super) fn read_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default();
    let query_name = name.split('|').next().unwrap_or_default();
    let connection = query_connection(mapping, query_name)?;
    if !connection
        .attribute("database_kind")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
        || !connection
            .attribute("import_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
    {
        return Err("only SQLite query datasources are supported".to_string());
    }
    if has_query_relation(&connection, query_name) {
        return correlated::read_component(component, mapping, mfd_path, &connection, query_name);
    }
    if name.contains('|') {
        return Err(
            "query name encodes a correlation but no relation metadata matches it".to_string(),
        );
    }
    read_uncorrelated_component(component, mfd_path, &connection, query_name, false).or_else(
        |ordinary_reason| {
            joined::read_component(component, mfd_path, &connection, query_name).map_err(
                |joined_reason| {
                    format!(
                        "{ordinary_reason}; relational query lowering also failed: {joined_reason}"
                    )
                },
            )
        },
    )
}

pub(super) fn read_inline_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    query_name: &str,
) -> Result<SchemaComponent, String> {
    let connection = query_connection(mapping, query_name)?;
    if !connection
        .attribute("database_kind")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
        || !connection
            .attribute("import_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
    {
        return Err("only SQLite query datasources are supported".to_string());
    }
    if has_query_relation(&connection, query_name) {
        return Err("standalone inline queries cannot participate in a relation".to_string());
    }
    read_uncorrelated_component(component, mfd_path, &connection, query_name, true)
}

fn read_uncorrelated_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    connection: &roxmltree::Node<'_, '_>,
    query_name: &str,
    inline: bool,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default();
    let local_view = connection
        .descendants()
        .filter(|node| {
            node.has_tag_name("LocalViewElement")
                && node.descendants().any(|path| {
                    path.has_tag_name("PathElement")
                        && path.attribute("Kind") == Some("Select Statement")
                        && path.attribute("Name") == Some(query_name)
                })
        })
        .collect::<Vec<_>>();
    let [local_view] = local_view.as_slice() else {
        return Err(format!(
            "expected exactly one datasource query definition named `{query_name}`"
        ));
    };
    let sql = local_view
        .attribute("SQL")
        .ok_or_else(|| "query definition has no SQL text".to_string())?;
    let parsed = Parser::new(sql)?.parse()?;

    let declared_parameters = read_parameter_types(local_view)?;
    let parameter_keys = read_parameter_keys(component)?;
    let predicates = parsed
        .predicates
        .into_iter()
        .map(|predicate| {
            let operand = match predicate.operand {
                ParsedOperand::Literal(value) => QueryOperand::Literal(value),
                ParsedOperand::Parameter(name) => {
                    let ty = declared_parameters.get(&name).copied().ok_or_else(|| {
                        format!("SQL parameter `:{name}` has no matching declaration")
                    })?;
                    let input_key = parameter_keys.get(&name).copied().ok_or_else(|| {
                        format!(
                            "parameter `:{name}` is supplied by a relation or parent row; correlated queries are not supported"
                        )
                    })?;
                    QueryOperand::Parameter {
                        name,
                        input_key,
                        ty,
                    }
                }
            };
            Ok(QueryPredicate {
                column: predicate.column,
                operator: predicate.operator,
                operand,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let connection_string = connection
        .attribute("ConnectionString")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "datasource has no SQLite connection path".to_string())?;
    let db_path = mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(connection_string);
    if !db_path.exists() {
        return Err(format!(
            "SQLite database `{connection_string}` was not found next to the design"
        ));
    }
    let physical = format_db::introspect(&db_path, &parsed.table)
        .map_err(|error| format!("could not introspect table `{}` ({error})", parsed.table))?;
    let columns = projection_columns(&parsed.projection, &physical)?;
    let schema = query_schema(&physical, &columns, &predicates, parsed.order.as_ref())?;
    let ports = if inline {
        read_inline_output_ports(component, &columns, &schema)?
    } else {
        read_output_ports(component, &columns, &schema)?
    };
    let data_root = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
        .ok_or_else(|| "query component has no entry tree".to_string())?;
    let (input_keys, output_keys) = entry_key_sets(&data_root);

    Ok(SchemaComponent {
        name: name.to_string(),
        format: ComponentFormat::Db,
        schema,
        input_instance: Some(connection_string.to_string()),
        output_instance: None,
        options: FormatOptions::default(),
        is_source: true,
        is_default_output: false,
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: vec![DbQuery {
            name: query_name.to_string(),
            collection: Vec::new(),
            predicates,
            order: parsed.order,
            cardinality: parsed.cardinality,
            required_paths: Vec::new(),
            computed_ports: BTreeMap::new(),
        }],
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

fn has_query_relation(connection: &roxmltree::Node<'_, '_>, query_name: &str) -> bool {
    connection.descendants().any(|relation| {
        relation.has_tag_name("LocalRelationElement")
            && relation.descendants().any(|path| {
                path.has_tag_name("PathElement") && path.attribute("Name") == Some(query_name)
            })
    })
}

fn query_connection<'a, 'input>(
    mapping: &'a roxmltree::Node<'a, 'input>,
    query_name: &str,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let refs = mapping
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("15"))
        .filter(|component| {
            component.descendants().any(|path| {
                path.has_tag_name("PathElement")
                    && path.attribute("Kind") == Some("Select Statement")
                    && path.attribute("Name") == Some(query_name)
            })
        })
        .filter_map(|component| {
            component
                .descendants()
                .find(|node| node.has_tag_name("database"))
                .and_then(|database| database.attribute("ref"))
        })
        .collect::<BTreeSet<_>>();
    if refs.len() != 1 {
        return Err(format!(
            "query `{query_name}` does not select one unambiguous datasource"
        ));
    }
    let connection_ref = refs
        .iter()
        .next()
        .copied()
        .ok_or_else(|| format!("query `{query_name}` has no datasource"))?;
    let connections = mapping
        .descendants()
        .filter(|node| {
            node.has_tag_name("database_connection")
                && node.attribute("name") == Some(connection_ref)
        })
        .collect::<Vec<_>>();
    let [connection] = connections.as_slice() else {
        return Err(format!(
            "datasource `{connection_ref}` must resolve to exactly one connection"
        ));
    };
    Ok(*connection)
}

fn read_parameter_types(
    local_view: &roxmltree::Node<'_, '_>,
) -> Result<BTreeMap<String, ScalarType>, String> {
    let mut parameters = BTreeMap::new();
    for parameter in local_view
        .descendants()
        .filter(|node| node.has_tag_name("Parameter"))
    {
        let name = parameter
            .attribute("name")
            .filter(|name| valid_identifier(name))
            .ok_or_else(|| "query parameter has an invalid or missing name".to_string())?;
        let ty = scalar_type(parameter.attribute("type").unwrap_or_default())?;
        if parameters.insert(name.to_string(), ty).is_some() {
            return Err(format!("query declares parameter `:{name}` more than once"));
        }
    }
    Ok(parameters)
}

fn read_parameter_keys(
    component: &roxmltree::Node<'_, '_>,
) -> Result<BTreeMap<String, u32>, String> {
    let mut parameters = BTreeMap::new();
    for entry in component.descendants().filter(|node| {
        node.has_tag_name("entry")
            && node.attribute("type") == Some("attribute")
            && node.attribute("inpkey").is_some()
    }) {
        let name = entry.attribute("name").unwrap_or_default();
        let key = parse_u32(entry.attribute("inpkey"))
            .ok_or_else(|| format!("query parameter `{name}` has an invalid input key"))?;
        if parameters.insert(name.to_string(), key).is_some() {
            return Err(format!(
                "query exposes parameter `:{name}` through more than one input"
            ));
        }
    }
    Ok(parameters)
}

fn scalar_type(name: &str) -> Result<ScalarType, String> {
    match name.to_ascii_lowercase().as_str() {
        "string" | "text" => Ok(ScalarType::String),
        "integer" | "int" | "long" => Ok(ScalarType::Int),
        "decimal" | "double" | "float" | "real" => Ok(ScalarType::Float),
        "boolean" | "bool" => Ok(ScalarType::Bool),
        _ => Err(format!("query parameter type `{name}` is not supported")),
    }
}

fn query_schema(
    physical: &SchemaNode,
    selected: &[String],
    predicates: &[QueryPredicate],
    order: Option<&QueryOrder>,
) -> Result<SchemaNode, String> {
    let SchemaKind::Group { children, .. } = &physical.kind else {
        return Err("introspected query table is not a group".to_string());
    };
    let mut needed = selected.to_vec();
    needed.extend(predicates.iter().map(|predicate| predicate.column.clone()));
    needed.extend(order.map(|order| order.column.clone()));
    let mut seen = BTreeSet::new();
    let projected = needed
        .into_iter()
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .map(|name| {
            let mut matches = children
                .iter()
                .filter(|column| column.name.eq_ignore_ascii_case(&name));
            let column = matches
                .next()
                .ok_or_else(|| format!("table `{}` has no column `{name}`", physical.name))?;
            if matches.next().is_some() {
                return Err(format!("column `{name}` is ambiguous ignoring ASCII case"));
            }
            Ok(column.clone())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SchemaNode::group(physical.name.clone(), projected).repeating())
}

fn projection_columns(
    projection: &QueryProjection,
    physical: &SchemaNode,
) -> Result<Vec<String>, String> {
    match projection {
        QueryProjection::Columns(columns) => {
            ensure_unique_names("SQL projection", columns)?;
            Ok(columns.clone())
        }
        QueryProjection::All => match &physical.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .map(|column| match column.kind {
                    SchemaKind::Scalar { .. } if !column.repeating => Ok(column.name.clone()),
                    _ => Err(format!(
                        "table `{}` contains a non-scalar column that cannot be expanded from `*`",
                        physical.name
                    )),
                })
                .collect(),
            SchemaKind::Scalar { .. } => Err("introspected query table is not a group".to_string()),
        },
    }
}

fn read_output_ports(
    component: &roxmltree::Node<'_, '_>,
    selected: &[String],
    schema: &SchemaNode,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let mut ports = BTreeMap::new();
    let collections = component
        .descendants()
        .filter(|node| {
            node.has_tag_name("entry")
                && node.attribute("outkey").is_some()
                && node.attribute("type") != Some("attribute")
        })
        .collect::<Vec<_>>();
    let [collection] = collections.as_slice() else {
        return Err("query result must expose exactly one collection output".to_string());
    };
    let collection_key = parse_u32(collection.attribute("outkey"))
        .ok_or_else(|| "query result has an invalid collection output key".to_string())?;
    ports.insert(collection_key, Vec::new());
    let mut output_names = BTreeSet::new();
    let all_leaves = component
        .descendants()
        .filter(is_output_leaf)
        .collect::<Vec<_>>();
    let owned_leaves = collection
        .descendants()
        .filter(is_output_leaf)
        .collect::<Vec<_>>();
    if owned_leaves.len() != all_leaves.len() {
        return Err(
            "query result contains outputs outside its collection; nested or correlated results are not supported"
                .to_string(),
        );
    }
    for entry in owned_leaves {
        let name = entry.attribute("name").unwrap_or_default();
        let key = parse_u32(entry.attribute("outkey"))
            .ok_or_else(|| format!("query output `{name}` has an invalid key"))?;
        if !output_names.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "query result exposes output `{name}` more than once ignoring ASCII case"
            ));
        }
        let canonical = match &schema.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .find(|child| child.name.eq_ignore_ascii_case(name)),
            SchemaKind::Scalar { .. } => None,
        }
        .map(|child| child.name.clone())
        .ok_or_else(|| format!("query output `{name}` is not in the typed schema"))?;
        if ports.insert(key, vec![canonical]).is_some() {
            return Err(format!("query output key `{key}` is used more than once"));
        }
    }
    let selected_names = selected
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if output_names != selected_names {
        return Err("SQL projection does not match the query component outputs".to_string());
    }
    Ok(ports)
}

fn read_inline_output_ports(
    component: &roxmltree::Node<'_, '_>,
    selected: &[String],
    schema: &SchemaNode,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let active = component
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type") == Some("routine")
                && entry.attribute("outkey").is_some()
        })
        .collect::<Vec<_>>();
    let [routine] = active.as_slice() else {
        return Err("inline query must expose exactly one collection output".to_string());
    };
    if component.descendants().any(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("outkey").is_some()
            && entry != *routine
            && !entry.ancestors().any(|ancestor| ancestor == *routine)
    }) {
        return Err("inline query contains outputs outside its result collection".to_string());
    }
    let tables = routine
        .children()
        .filter(|entry| entry.has_tag_name("entry") && entry.attribute("type") == Some("table"))
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return Err("inline query result must contain exactly one table entry".to_string());
    };
    let columns = table
        .children()
        .filter(|entry| entry.has_tag_name("entry"))
        .collect::<Vec<_>>();
    if columns.iter().any(|entry| {
        entry.children().any(|child| child.has_tag_name("entry"))
            || entry
                .attribute("type")
                .is_some_and(|kind| kind != "attribute")
    }) {
        return Err("inline query result contains unsupported nested entries".to_string());
    }
    let mut ports = BTreeMap::new();
    let collection_key = parse_u32(routine.attribute("outkey"))
        .ok_or_else(|| "inline query collection has an invalid output key".to_string())?;
    ports.insert(collection_key, Vec::new());
    let mut output_names = BTreeSet::new();
    for entry in columns {
        let name = entry.attribute("name").unwrap_or_default();
        let key = parse_u32(entry.attribute("outkey"))
            .ok_or_else(|| format!("inline query output `{name}` has an invalid key"))?;
        if !output_names.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "inline query result exposes output `{name}` more than once ignoring ASCII case"
            ));
        }
        let canonical = match &schema.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .find(|child| child.name.eq_ignore_ascii_case(name)),
            SchemaKind::Scalar { .. } => None,
        }
        .map(|child| child.name.clone())
        .ok_or_else(|| format!("inline query output `{name}` is not in the typed schema"))?;
        if ports.insert(key, vec![canonical]).is_some() {
            return Err(format!(
                "inline query output key `{key}` is used more than once"
            ));
        }
    }
    let selected_names = selected
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if output_names != selected_names {
        return Err("SQL projection does not match the inline query outputs".to_string());
    }
    Ok(ports)
}

fn is_output_leaf(node: &roxmltree::Node<'_, '_>) -> bool {
    node.has_tag_name("entry")
        && node.attribute("type") == Some("attribute")
        && node.attribute("outkey").is_some()
}

fn ensure_unique_names(label: &str, names: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "{label} contains `{name}` more than once ignoring ASCII case"
            ));
        }
    }
    Ok(())
}

impl GraphBuilder<'_> {
    pub(super) fn db_computed_projection_node(&mut self, key: u32) -> Option<NodeId> {
        if let Some(node) = self.source_node_function_nodes.get(&key) {
            return Some(*node);
        }
        let (source, expression) =
            self.sources
                .iter()
                .enumerate()
                .find_map(|(source, component)| {
                    component.db_queries.iter().find_map(|query| {
                        query
                            .computed_ports
                            .get(&key)
                            .cloned()
                            .map(|expression| (source, expression))
                    })
                })?;
        let DbComputedExpression::Multiply { left, right, ty } = expression;
        let left = self.source_value_path(source, left);
        let right = self.source_value_path(source, right);
        let left = self.source_field_at(&left)?;
        let right = self.source_field_at(&right)?;
        let raw = self.alloc(Node::Call {
            function: "sqlite_multiply".to_string(),
            args: vec![left, right],
        });
        let node = self.apply_source_node_functions(key, ty, raw);
        self.source_node_function_nodes.insert(key, node);
        Some(node)
    }

    pub(super) fn binding_node_at_anchor(
        &mut self,
        key: u32,
        target_path: &[String],
        active_anchor: &[String],
    ) -> Option<NodeId> {
        let unscoped_query = self.sources.iter().enumerate().any(|(source, component)| {
            db_query_owns_output(component, key)
                && !component.db_queries.is_empty()
                && !self.query_scope_sources.contains(&source)
        });
        if !unscoped_query && let Some(source_path) = self.source_abs_path(key) {
            let source_path = self.source_value_path(source_path.source, source_path.path);
            let ty = self
                .schema_node(&source_path)
                .and_then(|node| match &node.kind {
                    SchemaKind::Scalar { ty } => Some(*ty),
                    SchemaKind::Group { .. } => None,
                });
            let input = self.source_field_at_anchor(&source_path, active_anchor)?;
            return Some(match ty {
                Some(ty) if self.has_source_node_functions(key) => {
                    self.apply_source_node_functions(key, ty, input)
                }
                Some(_) | None => input,
            });
        }
        if !unscoped_query && let Some(node) = self.scalar_node_at_anchor(key, active_anchor) {
            return Some(node);
        }
        self.binding_node(key, target_path)
    }

    pub(super) fn binding_node(&mut self, key: u32, target_path: &[String]) -> Option<NodeId> {
        if let Some((source, component)) = self
            .sources
            .iter()
            .enumerate()
            .find(|(_, component)| db_query_owns_output(component, key))
            && !component.db_queries.is_empty()
            && !self.query_scope_sources.contains(&source)
        {
            if self.warned_unscoped_queries.insert(source) {
                self.warnings.push(format!(
                    "database query `{}` is used only through scalar outputs; its predicates and parameters require a collection iteration, so those bindings were skipped",
                    component.name
                ));
            }
            return None;
        }
        let node = self.value_node(key);
        if node.is_none() {
            self.warnings.push(format!(
                "binding for `{}` comes from an unsupported feed; skipped",
                target_path.join("/")
            ));
        }
        node
    }

    pub(super) fn apply_db_controls(
        &mut self,
        db_where: Option<usize>,
        source_path: Option<&SourcePath>,
        existing_filter: Option<NodeId>,
    ) -> Result<(Option<NodeId>, Option<NodeId>, bool, bool), DbControlError> {
        let (query_filter, query_sort, query_descending, query_at_most_one) = self
            .apply_db_query(source_path, existing_filter)
            .map_err(DbControlError::Query)?;
        let (filter, where_sort, where_descending) = self
            .apply_db_where(db_where, source_path, query_filter)
            .map_err(|reason| DbControlError::Where {
                name: db_where
                    .and_then(|index| self.fn_components.get(index))
                    .map_or_else(|| "unknown".to_string(), |component| component.name.clone()),
                reason,
            })?;
        if query_sort.is_some() && where_sort.is_some() {
            return Err(DbControlError::Query(
                "query ORDER is combined with a database where/order control".to_string(),
            ));
        }
        Ok((
            filter,
            query_sort.or(where_sort),
            if query_sort.is_some() {
                query_descending
            } else {
                where_descending
            },
            query_at_most_one,
        ))
    }

    pub(super) fn apply_db_query(
        &mut self,
        source_path: Option<&SourcePath>,
        existing_filter: Option<NodeId>,
    ) -> Result<(Option<NodeId>, Option<NodeId>, bool, bool), String> {
        let Some(source_path) = source_path else {
            return Ok((existing_filter, None, false, false));
        };
        let Some(source) = self.sources.get(source_path.source) else {
            return Ok((existing_filter, None, false, false));
        };
        let mut filter = existing_filter;
        let queries = source
            .db_queries
            .iter()
            .filter(|query| source_path.path.starts_with(&query.collection))
            .cloned()
            .collect::<Vec<_>>();
        let mut sort = None;
        let mut descending = false;
        let mut at_most_one = false;
        for query in queries {
            // A flattened multi-hop iteration retains every parent frame. Parent
            // predicates therefore evaluate identically for each child, and one
            // parent sort remains a stable ordering by that parent value.
            let mut collection = source_path.clone();
            collection.path = query.collection.clone();
            at_most_one |= query.collection == source_path.path
                && query.cardinality == QueryCardinality::AtMostOne;
            for path in query.required_paths {
                let mut required = collection.clone();
                required.path.extend(path);
                let required = self.source_value_path(required.source, required.path);
                let field = self
                    .source_field_at(&required)
                    .ok_or_else(|| "joined query required field is unavailable".to_string())?;
                let exists = self.alloc(Node::Call {
                    function: "exists".to_string(),
                    args: vec![field],
                });
                filter = Some(match filter {
                    Some(existing) => self.alloc(Node::Call {
                        function: "and".to_string(),
                        args: vec![existing, exists],
                    }),
                    None => exists,
                });
            }
            for predicate in query.predicates {
                if matches!(predicate.operand, QueryOperand::Correlated) {
                    continue;
                }
                let node = self.query_predicate_node(&collection, predicate)?;
                filter = Some(match filter {
                    Some(existing) => self.alloc(Node::Call {
                        function: "and".to_string(),
                        args: vec![existing, node],
                    }),
                    None => node,
                });
            }
            let query_sort = match query.order {
                Some(order) => {
                    let (node, ty) = self.db_column_node(&collection, &[order.column])?;
                    if ty == ScalarType::String {
                        return Err(
                        "text ORDER BY collation cannot be established from SQLite schema metadata"
                            .to_string(),
                    );
                    }
                    Some((node, order.descending))
                }
                None => None,
            };
            if let Some((node, direction)) = query_sort {
                if sort.replace(node).is_some() {
                    return Err("multiple query ORDER clauses apply to one iteration".to_string());
                }
                descending = direction;
            }
        }
        Ok((filter, sort, descending, at_most_one))
    }

    pub(super) fn db_query_is_at_most_one(&self, source_path: &SourcePath) -> bool {
        self.sources
            .get(source_path.source)
            .is_some_and(|source| source_query_is_at_most_one(source, &source_path.path))
    }

    fn query_predicate_node(
        &mut self,
        source_path: &SourcePath,
        predicate: QueryPredicate,
    ) -> Result<NodeId, String> {
        let (column, column_type) = self.db_column_node(source_path, &[predicate.column])?;
        let value = match predicate.operand {
            QueryOperand::Literal(value) => coerce_value(value, column_type)?,
            QueryOperand::Parameter {
                name,
                input_key,
                ty,
            } => {
                let value = self.static_query_parameter(input_key, 0)?;
                let query_name = self.sources[source_path.source]
                    .db_queries
                    .iter()
                    .find(|query| query.collection == source_path.path)
                    .map_or("unknown", |query| query.name.as_str());
                coerce_value(value, column_type).map_err(|reason| {
                    format!(
                        "query `{query_name}` parameter `:{name}` declared as {ty:?} cannot be converted: {reason}"
                    )
                })?
            }
            QueryOperand::Correlated => {
                return Err(
                    "correlated predicate was not absorbed by the relational source".to_string(),
                );
            }
        };
        if matches!(predicate.operator, QueryOperator::Like) && column_type != ScalarType::String {
            return Err("LIKE requires a string column and operand".to_string());
        }
        if matches!(predicate.operator, QueryOperator::Equal) && column_type == ScalarType::String {
            return Err(
                "text equality collation cannot be established from SQLite schema metadata"
                    .to_string(),
            );
        }
        let operand = self.alloc(Node::Const { value });
        let comparison = self.alloc(Node::Call {
            function: match predicate.operator {
                QueryOperator::Equal => "equal",
                QueryOperator::Like => "sql_like",
                QueryOperator::Greater => "greater_than",
            }
            .to_string(),
            args: vec![column, operand],
        });
        let column_exists = self.alloc(Node::Call {
            function: "exists".to_string(),
            args: vec![column],
        });
        let operand_exists = self.alloc(Node::Call {
            function: "exists".to_string(),
            args: vec![operand],
        });
        let both_exist = self.alloc(Node::Call {
            function: "and".to_string(),
            args: vec![column_exists, operand_exists],
        });
        let false_value = self.alloc(Node::Const {
            value: Value::Bool(false),
        });
        Ok(self.alloc(Node::If {
            condition: both_exist,
            then: comparison,
            else_: false_value,
        }))
    }

    fn static_query_parameter(&self, input_key: u32, depth: usize) -> Result<Value, String> {
        if depth >= 12 {
            return Err("query parameter feed contains a cycle".to_string());
        }
        let feed = self
            .edge_from
            .get(&input_key)
            .copied()
            .ok_or_else(|| "query parameter input is not connected".to_string())?;
        let index = self
            .fn_by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| "query parameter is not a compile-time constant".to_string())?;
        let component: &FnComponent = &self.fn_components[index];
        if component.name == "constant" {
            let (value, datatype) = component
                .constant
                .as_ref()
                .ok_or_else(|| "constant query parameter has no value".to_string())?;
            return Ok(parse_constant(value, datatype));
        }
        if is_input(component) {
            let transparent_input = component.inputs.first().copied().flatten();
            return match transparent_input {
                Some(input) if self.edge_from.contains_key(&input) => {
                    self.static_query_parameter(input, depth + 1)
                }
                _ => component.input_preview.clone().ok_or_else(|| {
                    "query parameter input has neither an upstream value nor an enabled preview value"
                        .to_string()
                }),
            };
        }
        Err(format!(
            "query parameter uses dynamic function `{}`; only literal constants are supported",
            component.name
        ))
    }
}

pub(super) fn source_query_is_at_most_one(source: &SchemaComponent, path: &[String]) -> bool {
    source
        .db_queries
        .iter()
        .any(|query| query.collection == path && query.cardinality == QueryCardinality::AtMostOne)
}

fn db_query_owns_output(component: &SchemaComponent, key: u32) -> bool {
    component.ports.contains_key(&key)
        || component
            .db_queries
            .iter()
            .any(|query| query.computed_ports.contains_key(&key))
}

fn coerce_value(value: Value, ty: ScalarType) -> Result<Value, String> {
    match (value, ty) {
        (Value::String(value), ScalarType::String) => Ok(Value::String(value)),
        (Value::Bool(value), ScalarType::Bool) => Ok(Value::Bool(value)),
        (Value::Int(value), ScalarType::Int) => Ok(Value::Int(value)),
        (Value::Float(value), ScalarType::Int)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Ok(Value::Int(value as i64))
        }
        (Value::Int(value), ScalarType::Float)
            if (-9_007_199_254_740_992..=9_007_199_254_740_992).contains(&value) =>
        {
            Ok(Value::Float(value as f64))
        }
        (Value::Float(value), ScalarType::Float) if value.is_finite() => Ok(Value::Float(value)),
        (Value::String(value), ScalarType::Int) => value
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| "query operand is not an integer".to_string()),
        (Value::String(value), ScalarType::Float) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float)
            .ok_or_else(|| "query operand is not a finite number".to_string()),
        (Value::Null | Value::JsonNull(_), _) => Err("query parameters cannot be null".to_string()),
        (value, expected) => Err(format!(
            "query operand has type {}, expected {expected:?}",
            value.type_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conservative_parameterized_select() {
        let parsed = Parser::new(
            r#"SELECT "First", "Title" FROM "Person" WHERE "ForeignKey" = :DepartmentID AND "Title" LIKE '%Manager%'"#,
        )
        .and_then(Parser::parse)
        .unwrap();
        assert_eq!(parsed.table, "Person");
        assert!(matches!(
            parsed.projection,
            QueryProjection::Columns(columns) if columns == ["First", "Title"]
        ));
        assert_eq!(parsed.predicates.len(), 2);
    }

    #[test]
    fn parses_all_columns_and_exact_limit_one() {
        let parsed = Parser::new("SELECT * FROM Articles ORDER BY Price DESC LIMIT 1")
            .and_then(Parser::parse)
            .unwrap();
        assert!(matches!(parsed.projection, QueryProjection::All));
        assert_eq!(parsed.cardinality, QueryCardinality::AtMostOne);
    }

    #[test]
    fn rejects_other_limits_offsets_and_dynamic_limits() {
        for sql in [
            "SELECT * FROM Articles LIMIT 2",
            "SELECT * FROM Articles LIMIT :count",
            "SELECT * FROM Articles LIMIT 1 OFFSET 0",
        ] {
            assert!(Parser::new(sql).and_then(Parser::parse).is_err(), "{sql}");
        }
    }

    #[test]
    fn rejects_joins_and_disjunctions() {
        for sql in [
            "SELECT Name FROM Person JOIN Department ON Person.Id = Department.Id",
            "SELECT Name FROM Person WHERE Id = 1 OR Id = 2",
            "SELECT Name, name FROM Person",
        ] {
            assert!(Parser::new(sql).and_then(Parser::parse).is_err(), "{sql}");
        }
    }

    #[test]
    fn parses_bounded_joined_projection_and_predicate() {
        let parsed = Parser::new(
            "SELECT (Units * Cost) AS Total, Purchase.Id, Item.Label FROM Purchase INNER JOIN Item ON Purchase.ItemId = Item.Id WHERE Purchase.Units > :Minimum",
        )
        .and_then(Parser::parse_joined)
        .unwrap();
        assert_eq!(parsed.primary_table, "Purchase");
        assert_eq!(parsed.joined_table, "Item");
        assert_eq!(parsed.projections.len(), 3);
        assert!(matches!(
            parsed.projections[0].expr,
            sql::JoinedProjectionExpr::Multiply(_, _)
        ));
        assert!(matches!(
            parsed.predicate_operand,
            ParsedOperand::Parameter(ref name) if name == "Minimum"
        ));
    }

    #[test]
    fn rejects_duplicate_keys_and_result_leaves_outside_the_collection() {
        let schema = SchemaNode::group(
            "Person",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Title", ScalarType::String),
            ],
        )
        .repeating();
        for xml in [
            r#"<component><entry name="Rows" outkey="1"><entry name="Name" type="attribute" outkey="2"/><entry name="Title" type="attribute" outkey="2"/></entry></component>"#,
            r#"<component><entry name="Rows" outkey="1"><entry name="Name" type="attribute" outkey="2"/><entry name="name" type="attribute" outkey="3"/></entry></component>"#,
            r#"<component><entry name="Name" type="attribute" outkey="2"/><entry name="Rows" outkey="1"><entry name="Title" type="attribute" outkey="3"/></entry></component>"#,
        ] {
            let document = roxmltree::Document::parse(xml).unwrap();
            assert!(
                read_output_ports(
                    &document.root_element(),
                    &["Name".to_string(), "Title".to_string()],
                    &schema,
                )
                .is_err(),
                "{xml}"
            );
        }
    }

    #[test]
    fn relation_metadata_marks_a_query_as_correlated() {
        let document = roxmltree::Document::parse(
            r#"<database_connection><LocalRelationElement><SourceColumns><PathElement Name="ManagersByDepartment" Kind="Select Statement"/></SourceColumns></LocalRelationElement></database_connection>"#,
        )
        .unwrap();
        assert!(has_query_relation(
            &document.root_element(),
            "ManagersByDepartment"
        ));
    }

    #[test]
    fn routine_catalog_suppression_requires_a_matching_query_component() {
        let without_query = roxmltree::Document::parse(
            r#"<children><component library="db" kind="15"><entry name="Q" type="routine"/></component></children>"#,
        )
        .unwrap();
        let catalog = without_query
            .root_element()
            .children()
            .find(|node| node.has_tag_name("component"))
            .unwrap();
        assert!(!is_routine_catalog(&catalog, &without_query.root_element()));

        let with_query = roxmltree::Document::parse(
            r#"<children><component library="db" kind="15"><entry name="Q" type="routine"/></component><component name="Q" library="db" kind="28"/></children>"#,
        )
        .unwrap();
        let catalog = with_query
            .root_element()
            .children()
            .find(|node| node.attribute("kind") == Some("15"))
            .unwrap();
        assert!(is_routine_catalog(&catalog, &with_query.root_element()));

        let mixed = roxmltree::Document::parse(
            r#"<children><component library="db" kind="15"><entry name="TableA" type="table"><entry name="Q" type="routine"/></entry><entry name="Unrelated" type="table"/></component><component name="Q|Param" library="db" kind="28"/></children>"#,
        )
        .unwrap();
        let catalog = mixed
            .root_element()
            .children()
            .find(|node| node.attribute("kind") == Some("15"))
            .unwrap();
        assert!(!is_routine_catalog(&catalog, &mixed.root_element()));
    }

    #[test]
    fn query_identity_matches_only_typed_base_and_parameter_forms() {
        assert!(same_query_identity("Q", "Q|Parameter"));
        assert!(same_query_identity("Q|Parameter", "Q"));
        assert!(same_query_identity("Q|Parameter", "Q|Parameter"));
        assert!(!same_query_identity("Q|First", "Q|Second"));
        assert!(!same_query_identity("QExtra", "Q"));
        assert!(!same_query_identity("Q|", "Q"));
        assert!(!same_query_identity("Q|P|Extra", "Q|P"));
    }
}
