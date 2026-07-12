use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FormatOptions, Node, NodeId};

use super::GraphBuilder;
use super::function::{FnComponent, is_input, parse_constant};
use super::schema::{ComponentFormat, SchemaComponent, entry_key_sets, parse_u32};
use super::source::SourcePath;

#[derive(Clone)]
pub(super) struct DbQuery {
    name: String,
    predicates: Vec<QueryPredicate>,
    order: Option<QueryOrder>,
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
}

#[derive(Clone)]
enum QueryOperand {
    Parameter {
        name: String,
        input_key: u32,
        ty: ScalarType,
    },
    Literal(Value),
}

#[derive(Clone)]
struct QueryOrder {
    column: String,
    descending: bool,
}

struct ParsedQuery {
    table: String,
    columns: Vec<String>,
    predicates: Vec<ParsedPredicate>,
    order: Option<QueryOrder>,
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
    if component.attribute("kind") != Some("15")
        || component
            .descendants()
            .any(|node| node.has_tag_name("entry") && node.attribute("type") == Some("table"))
    {
        return false;
    }
    let routines = component
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("routine"))
        .filter_map(|entry| entry.attribute("name"))
        .collect::<BTreeSet<_>>();
    !routines.is_empty()
        && siblings.children().any(|sibling| {
            sibling.has_tag_name("component")
                && sibling.attribute("library") == Some("db")
                && sibling.attribute("kind") == Some("28")
                && sibling
                    .attribute("name")
                    .and_then(|name| name.split('|').next())
                    .is_some_and(|name| routines.contains(name))
        })
}

pub(super) fn read_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default();
    if name.contains('|') {
        return Err(
            "correlated query parameters are not supported; the query was not flattened"
                .to_string(),
        );
    }
    let connection = query_connection(mapping, name)?;
    if !connection
        .attribute("database_kind")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
        || !connection
            .attribute("import_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("SQLite"))
    {
        return Err("only SQLite query datasources are supported".to_string());
    }
    if has_query_relation(&connection, name) {
        return Err(
            "query participates in datasource relation metadata; correlated queries are not supported"
                .to_string(),
        );
    }
    let local_view = connection
        .descendants()
        .filter(|node| {
            node.has_tag_name("LocalViewElement")
                && node.descendants().any(|path| {
                    path.has_tag_name("PathElement")
                        && path.attribute("Kind") == Some("Select Statement")
                        && path.attribute("Name") == Some(name)
                })
        })
        .collect::<Vec<_>>();
    let [local_view] = local_view.as_slice() else {
        return Err(format!(
            "expected exactly one datasource query definition named `{name}`"
        ));
    };
    let sql = local_view
        .attribute("SQL")
        .ok_or_else(|| "query definition has no SQL text".to_string())?;
    let parsed = Parser::new(sql)?.parse()?;
    ensure_unique_names("SQL projection", &parsed.columns)?;

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
    let schema = query_schema(
        &physical,
        &parsed.columns,
        &predicates,
        parsed.order.as_ref(),
    )?;
    let ports = read_output_ports(component, &parsed.columns, &schema)?;
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
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
        db_query: Some(DbQuery {
            name: name.to_string(),
            predicates,
            order: parsed.order,
        }),
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
    pub(super) fn binding_node(&mut self, key: u32, target_path: &[String]) -> Option<NodeId> {
        if let Some((source, component)) = self
            .sources
            .iter()
            .enumerate()
            .find(|(_, component)| component.ports.contains_key(&key))
            && component.db_query.is_some()
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
    ) -> Result<(Option<NodeId>, Option<NodeId>, bool), DbControlError> {
        let (query_filter, query_sort, query_descending) = self
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
        ))
    }

    pub(super) fn apply_db_query(
        &mut self,
        source_path: Option<&SourcePath>,
        existing_filter: Option<NodeId>,
    ) -> Result<(Option<NodeId>, Option<NodeId>, bool), String> {
        let Some(source_path) = source_path else {
            return Ok((existing_filter, None, false));
        };
        let Some(query) = self
            .sources
            .get(source_path.source)
            .and_then(|source| source.db_query.clone())
        else {
            return Ok((existing_filter, None, false));
        };
        let mut filter = existing_filter;
        for predicate in query.predicates {
            let node = self.query_predicate_node(source_path, predicate)?;
            filter = Some(match filter {
                Some(existing) => self.alloc(Node::Call {
                    function: "and".to_string(),
                    args: vec![existing, node],
                }),
                None => node,
            });
        }
        let (sort, descending) = match query.order {
            Some(order) => {
                let (node, ty) = self.db_column_node(source_path, &[order.column])?;
                if ty == ScalarType::String {
                    return Err(
                        "text ORDER BY collation cannot be established from SQLite schema metadata"
                            .to_string(),
                    );
                }
                (Some(node), order.descending)
            }
            None => (None, false),
        };
        Ok((filter, sort, descending))
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
                if ty != column_type {
                    return Err(format!(
                        "query `{}` parameter `:{name}` type {ty:?} does not match column type {column_type:?}",
                        self.sources[source_path.source]
                            .db_query
                            .as_ref()
                            .map_or("unknown", |query| query.name.as_str())
                    ));
                }
                let value = self.static_query_parameter(input_key, 0)?;
                coerce_value(value, ty)?
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
            let transparent_input = component
                .inputs
                .first()
                .copied()
                .flatten()
                .ok_or_else(|| "query parameter input has no upstream pin".to_string())?;
            return self.static_query_parameter(transparent_input, depth + 1);
        }
        Err(format!(
            "query parameter uses dynamic function `{}`; only literal constants are supported",
            component.name
        ))
    }
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
        (Value::Null, _) => Err("query parameters cannot be null".to_string()),
        (value, expected) => Err(format!(
            "query operand has type {}, expected {expected:?}",
            value.type_name()
        )),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    String(String),
    Number(String),
    Parameter(String),
    Comma,
    Equal,
    Semicolon,
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(sql: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: tokenize(sql)?,
            position: 0,
        })
    }

    fn parse(mut self) -> Result<ParsedQuery, String> {
        self.keyword("SELECT")?;
        let mut columns = vec![self.identifier()?];
        while self.take(&Token::Comma) {
            columns.push(self.identifier()?);
        }
        ensure_unique_names("SQL projection", &columns)?;
        self.keyword("FROM")?;
        let table = self.identifier()?;
        let mut predicates = Vec::new();
        if self.take_keyword("WHERE") {
            loop {
                let column = self.identifier()?;
                let operator = if self.take(&Token::Equal) {
                    QueryOperator::Equal
                } else if self.take_keyword("LIKE") {
                    QueryOperator::Like
                } else {
                    return Err("query predicates must use `=` or `LIKE`".to_string());
                };
                let operand = match self.next() {
                    Some(Token::Parameter(name)) => ParsedOperand::Parameter(name),
                    Some(Token::String(value)) => ParsedOperand::Literal(Value::String(value)),
                    Some(Token::Number(value)) => ParsedOperand::Literal(parse_number(&value)?),
                    _ => {
                        return Err(
                            "query predicate operands must be named parameters or literals"
                                .to_string(),
                        );
                    }
                };
                predicates.push(ParsedPredicate {
                    column,
                    operator,
                    operand,
                });
                if !self.take_keyword("AND") {
                    break;
                }
            }
        }
        let order = if self.take_keyword("ORDER") {
            self.keyword("BY")?;
            let column = self.identifier()?;
            let descending = if self.take_keyword("DESC") {
                true
            } else {
                self.take_keyword("ASC");
                false
            };
            Some(QueryOrder { column, descending })
        } else {
            None
        };
        self.take(&Token::Semicolon);
        if self.position != self.tokens.len() {
            return Err(
                "only a single-table SELECT with conjunction predicates is supported".to_string(),
            );
        }
        Ok(ParsedQuery {
            table,
            columns,
            predicates,
            order,
        })
    }

    fn identifier(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Token::Word(word)) if valid_identifier(&word) => Ok(word),
            _ => Err("expected a simple SQL identifier".to_string()),
        }
    }

    fn keyword(&mut self, expected: &str) -> Result<(), String> {
        self.take_keyword(expected)
            .then_some(())
            .ok_or_else(|| format!("expected SQL keyword `{expected}`"))
    }

    fn take_keyword(&mut self, expected: &str) -> bool {
        let matches = self.tokens.get(self.position).is_some_and(
            |token| matches!(token, Token::Word(word) if word.eq_ignore_ascii_case(expected)),
        );
        if matches {
            self.position += 1;
        }
        matches
    }

    fn take(&mut self, expected: &Token) -> bool {
        if self.tokens.get(self.position) == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += usize::from(token.is_some());
        token
    }
}

fn tokenize(sql: &str) -> Result<Vec<Token>, String> {
    let chars = sql.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            character if character.is_ascii_whitespace() => index += 1,
            ',' => {
                tokens.push(Token::Comma);
                index += 1;
            }
            '=' => {
                tokens.push(Token::Equal);
                index += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                index += 1;
            }
            '"' => {
                let (value, next) = quoted(&chars, index + 1, '"')?;
                tokens.push(Token::Word(value));
                index = next;
            }
            '\'' => {
                let (value, next) = quoted(&chars, index + 1, '\'')?;
                tokens.push(Token::String(value));
                index = next;
            }
            ':' => {
                let (value, next) = bare(&chars, index + 1);
                if !valid_identifier(&value) {
                    return Err("query contains an invalid named parameter".to_string());
                }
                tokens.push(Token::Parameter(value));
                index = next;
            }
            character if character.is_ascii_digit() || matches!(character, '+' | '-') => {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_digit()
                        || matches!(chars[index], '.' | 'e' | 'E' | '+' | '-'))
                {
                    index += 1;
                }
                tokens.push(Token::Number(chars[start..index].iter().collect()));
            }
            character if character == '_' || character.is_ascii_alphabetic() => {
                let (value, next) = bare(&chars, index);
                tokens.push(Token::Word(value));
                index = next;
            }
            character => {
                return Err(format!(
                    "unsupported SQL token `{character}`; joins, expressions, and comments are not accepted"
                ));
            }
        }
    }
    Ok(tokens)
}

fn quoted(chars: &[char], mut index: usize, quote: char) -> Result<(String, usize), String> {
    let mut value = String::new();
    while index < chars.len() {
        if chars[index] == quote {
            if chars.get(index + 1) == Some(&quote) {
                value.push(quote);
                index += 2;
            } else {
                return Ok((value, index + 1));
            }
        } else {
            value.push(chars[index]);
            index += 1;
        }
    }
    Err("unterminated quoted SQL value".to_string())
}

fn bare(chars: &[char], mut index: usize) -> (String, usize) {
    let start = index;
    while index < chars.len() && (chars[index] == '_' || chars[index].is_ascii_alphanumeric()) {
        index += 1;
    }
    (chars[start..index].iter().collect(), index)
}

fn valid_identifier(identifier: &str) -> bool {
    let mut bytes = identifier.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn parse_number(number: &str) -> Result<Value, String> {
    number
        .parse::<i64>()
        .map(Value::Int)
        .or_else(|_| number.parse::<f64>().map(Value::Float))
        .map_err(|_| format!("invalid numeric SQL literal `{number}`"))
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
        assert_eq!(parsed.columns, ["First", "Title"]);
        assert_eq!(parsed.predicates.len(), 2);
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
    }
}
