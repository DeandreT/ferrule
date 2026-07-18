use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use super::{
    DbQuery, ParsedOperand, ParsedQuery, Parser, QueryCardinality, QueryOperand, QueryOperator,
    QueryOrder, QueryPredicate, QueryProjection, ensure_unique_names, query_schema,
    read_parameter_keys, read_parameter_types, same_query_identity,
};
use crate::import::schema::{ComponentFormat, SchemaComponent, entry_key_sets, parse_u32};

struct Relation {
    child_query: String,
    parameter: String,
    parent_name: String,
    parent_is_query: bool,
    parent_column: String,
}

struct QueryPlan {
    name: String,
    table: String,
    selected: Vec<String>,
    predicates: Vec<QueryPredicate>,
    order: Option<QueryOrder>,
    correlated_type: Option<ScalarType>,
    correlated_column: Option<String>,
}

struct ResolvedQuery {
    table: String,
    columns: Vec<String>,
    predicates: Vec<super::ParsedPredicate>,
    order: Option<QueryOrder>,
}

pub(super) struct CatalogRoutine<'a, 'input> {
    pub(super) parent: roxmltree::Node<'a, 'input>,
    pub(super) routine: roxmltree::Node<'a, 'input>,
}

enum PortShape<'a, 'input> {
    Query,
    Catalog(CatalogRoutine<'a, 'input>),
}

pub(super) fn read_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    connection: &roxmltree::Node<'_, '_>,
    component_query: &str,
) -> Result<SchemaComponent, String> {
    read_correlated_component(
        component,
        mapping,
        mfd_path,
        connection,
        component_query,
        PortShape::Query,
    )
}

pub(super) fn read_catalog_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    connection: &roxmltree::Node<'_, '_>,
    component_query: &str,
    routine: CatalogRoutine<'_, '_>,
) -> Result<SchemaComponent, String> {
    read_correlated_component(
        component,
        mapping,
        mfd_path,
        connection,
        component_query,
        PortShape::Catalog(routine),
    )
}

fn read_correlated_component(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    connection: &roxmltree::Node<'_, '_>,
    component_query: &str,
    port_shape: PortShape<'_, '_>,
) -> Result<SchemaComponent, String> {
    let relation = read_relation(connection, component_query)?;
    let parameter_keys = read_parameter_keys(component)?;
    let child = read_query_plan(
        connection,
        &relation.child_query,
        &parameter_keys,
        Some(&relation.parameter),
    )?;
    let parent = relation
        .parent_is_query
        .then(|| read_query_plan(connection, &relation.parent_name, &parameter_keys, None))
        .transpose()?;

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

    let parent_table = parent
        .as_ref()
        .map_or(relation.parent_name.as_str(), |plan| plan.table.as_str());
    let parent_physical = format_db::introspect(&db_path, parent_table)
        .map_err(|error| format!("could not introspect parent table `{parent_table}` ({error})"))?;
    let mut parent_schema = match &parent {
        Some(plan) => query_schema(
            &parent_physical,
            &plan.selected,
            &plan.predicates,
            plan.order.as_ref(),
        )?,
        None => parent_physical.clone(),
    };
    ensure_schema_column(
        &mut parent_schema,
        &parent_physical,
        &relation.parent_column,
    )?;

    let child_physical = format_db::introspect(&db_path, &child.table).map_err(|error| {
        format!(
            "could not introspect child table `{}` ({error})",
            child.table
        )
    })?;
    let mut child_schema = query_schema(
        &child_physical,
        &child.selected,
        &child.predicates,
        child.order.as_ref(),
    )?;
    let child_column = child
        .correlated_column
        .as_deref()
        .ok_or_else(|| "correlated query predicate was not resolved".to_string())?;
    let child_type = scalar_column_type(&child_schema, child_column)?;
    let parent_type = scalar_column_type(&parent_schema, &relation.parent_column)?;
    if child.correlated_type != Some(child_type) || child_type != parent_type {
        return Err(format!(
            "correlation types do not match: parameter {:?}, child {child_type:?}, parent {parent_type:?}",
            child.correlated_type
        ));
    }
    let canonical_child_table = child_schema.name.clone();
    let canonical_child_column = canonical_column(&child_schema, child_column)?;
    let canonical_parent_table = parent_schema.name.clone();
    let canonical_parent_column = canonical_column(&parent_schema, &relation.parent_column)?;
    let foreign_key = format_db::resolve_foreign_key_relation(
        &db_path,
        &canonical_parent_table,
        &canonical_parent_column,
        &canonical_child_table,
        &canonical_child_column,
    )
    .map_err(|error| format!("query relation does not match SQLite foreign keys ({error})"))?;
    let relation_name = format!("{canonical_child_table}|{}", foreign_key.join_column);
    child_schema.name = relation_name.clone();
    append_child(&mut parent_schema, child_schema)?;

    let parent_path = vec![canonical_parent_table.clone()];
    let mut child_path = parent_path.clone();
    child_path.push(relation_name);
    let ports = match port_shape {
        PortShape::Query => read_correlated_ports(
            component,
            mapping,
            &canonical_parent_table,
            &relation.child_query,
            &parent_schema,
            &parent_path,
            &child_path,
            parent.as_ref(),
            &child,
        )?,
        PortShape::Catalog(routine) => read_catalog_ports(
            &routine,
            &parent_schema,
            &parent_path,
            &child_path,
            parent.as_ref(),
            &child,
        )?,
    };
    let schema = SchemaNode::group("database", vec![parent_schema]);
    format_db::validate_relational_schema(&db_path, &schema)
        .map_err(|error| format!("query relation does not match SQLite foreign keys ({error})"))?;
    let (input_keys, output_keys) = component_key_sets(component);

    let mut queries = Vec::new();
    if let Some(parent) = parent {
        queries.push(DbQuery {
            name: parent.name,
            collection: parent_path,
            predicates: parent.predicates,
            order: parent.order,
            cardinality: QueryCardinality::Many,
        });
    }
    queries.push(DbQuery {
        name: child.name,
        collection: child_path,
        predicates: child.predicates,
        order: child.order,
        cardinality: QueryCardinality::Many,
    });
    Ok(SchemaComponent {
        name: component.attribute("name").unwrap_or_default().to_string(),
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
        db_queries: queries,
        dynamic_json: None,
    })
}

fn read_relation(
    connection: &roxmltree::Node<'_, '_>,
    component_query: &str,
) -> Result<Relation, String> {
    let relations = connection
        .descendants()
        .filter(|node| node.has_tag_name("LocalRelationElement"))
        .filter_map(|node| parse_relation(&node))
        .filter(|relation| {
            relation.child_query == component_query
                || relation.parent_is_query && relation.parent_name == component_query
        })
        .collect::<Vec<_>>();
    let [relation] = relations.as_slice() else {
        return Err(format!(
            "query `{component_query}` must participate in exactly one parent correlation"
        ));
    };
    Ok(Relation {
        child_query: relation.child_query.clone(),
        parameter: relation.parameter.clone(),
        parent_name: relation.parent_name.clone(),
        parent_is_query: relation.parent_is_query,
        parent_column: relation.parent_column.clone(),
    })
}

fn parse_relation(node: &roxmltree::Node<'_, '_>) -> Option<Relation> {
    let child_query = table_path(node, "SourceTable", "Select Statement")?;
    let parameter = single_column(node, "SourceColumns", "Parameter")?;
    let destination = node
        .children()
        .find(|node| node.has_tag_name("DestinationTable"))?;
    let parent_path = destination
        .children()
        .rfind(|node| node.has_tag_name("PathElement"))?;
    let parent_name = parent_path.attribute("Name")?.to_string();
    let parent_is_query = parent_path.attribute("Kind") == Some("Select Statement");
    if !parent_is_query && parent_path.attribute("Kind") != Some("Table") {
        return None;
    }
    Some(Relation {
        child_query,
        parameter,
        parent_name,
        parent_is_query,
        parent_column: single_column(node, "DestinationColumns", "Column")?,
    })
}

fn table_path(node: &roxmltree::Node<'_, '_>, tag: &str, kind: &str) -> Option<String> {
    node.children()
        .find(|node| node.has_tag_name(tag))?
        .children()
        .rfind(|node| node.has_tag_name("PathElement"))
        .filter(|path| path.attribute("Kind") == Some(kind))?
        .attribute("Name")
        .map(str::to_string)
}

fn single_column(node: &roxmltree::Node<'_, '_>, tag: &str, kind: &str) -> Option<String> {
    let columns = node
        .children()
        .find(|node| node.has_tag_name(tag))?
        .children()
        .filter(|node| node.has_tag_name("Column") && node.attribute("kind") == Some(kind))
        .filter_map(|column| column.attribute("name"))
        .collect::<Vec<_>>();
    let [column] = columns.as_slice() else {
        return None;
    };
    Some((*column).to_string())
}

fn read_query_plan(
    connection: &roxmltree::Node<'_, '_>,
    name: &str,
    parameter_keys: &BTreeMap<String, u32>,
    correlated: Option<&str>,
) -> Result<QueryPlan, String> {
    let views = connection
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
    let [view] = views.as_slice() else {
        return Err(format!(
            "expected exactly one query definition named `{name}`"
        ));
    };
    let parsed = Parser::new(
        view.attribute("SQL")
            .ok_or_else(|| format!("query `{name}` has no SQL text"))?,
    )?
    .parse()?;
    let ParsedQuery {
        table,
        projection,
        predicates,
        order,
        cardinality,
    } = parsed;
    let columns = match projection {
        QueryProjection::Columns(columns) => columns,
        QueryProjection::All => {
            return Err("`SELECT *` is supported only for standalone queries".to_string());
        }
    };
    ensure_unique_names("SQL projection", &columns)?;
    if cardinality != QueryCardinality::Many {
        return Err("SQL LIMIT is supported only for standalone queries".to_string());
    }
    let declarations = read_parameter_types(view)?;
    build_plan(
        name,
        ResolvedQuery {
            table,
            columns,
            predicates,
            order,
        },
        &declarations,
        parameter_keys,
        correlated,
    )
}

fn build_plan(
    name: &str,
    query: ResolvedQuery,
    declarations: &BTreeMap<String, ScalarType>,
    parameter_keys: &BTreeMap<String, u32>,
    correlated: Option<&str>,
) -> Result<QueryPlan, String> {
    let mut correlated_type = None;
    let mut correlated_column = None;
    let predicates = query
        .predicates
        .into_iter()
        .map(|predicate| {
            let operand = match predicate.operand {
                ParsedOperand::Literal(value) => QueryOperand::Literal(value),
                ParsedOperand::Parameter(parameter) if correlated == Some(parameter.as_str()) => {
                    if !matches!(predicate.operator, QueryOperator::Equal)
                        || correlated_column
                            .replace(predicate.column.clone())
                            .is_some()
                    {
                        return Err("correlation must be one equality predicate".to_string());
                    }
                    correlated_type = declarations.get(&parameter).copied();
                    QueryOperand::Correlated
                }
                ParsedOperand::Parameter(parameter) => {
                    let ty = declarations.get(&parameter).copied().ok_or_else(|| {
                        format!("SQL parameter `:{parameter}` has no declaration")
                    })?;
                    let input_key = parameter_keys.get(&parameter).copied().ok_or_else(|| {
                        format!("parameter `:{parameter}` is not a connected static input")
                    })?;
                    QueryOperand::Parameter {
                        name: parameter,
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
    if correlated.is_some() && correlated_column.is_none() {
        return Err("relation parameter is not used by one equality predicate".to_string());
    }
    Ok(QueryPlan {
        name: name.to_string(),
        table: query.table,
        selected: query.columns,
        predicates,
        order: query.order,
        correlated_type,
        correlated_column,
    })
}

fn ensure_schema_column(
    schema: &mut SchemaNode,
    physical: &SchemaNode,
    column: &str,
) -> Result<(), String> {
    if matches!(&schema.kind, SchemaKind::Group { children, .. }
        if children.iter().any(|child| child.name.eq_ignore_ascii_case(column)))
    {
        return Ok(());
    }
    let physical_column = match &physical.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(column)),
        SchemaKind::Scalar { .. } => None,
    }
    .cloned()
    .ok_or_else(|| {
        format!(
            "table `{}` has no relation column `{column}`",
            physical.name
        )
    })?;
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return Err("query table schema is not a group".to_string());
    };
    children.push(physical_column);
    Ok(())
}

fn scalar_column_type(schema: &SchemaNode, column: &str) -> Result<ScalarType, String> {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(column))
            .and_then(|child| match child.kind {
                SchemaKind::Scalar { ty } if !child.repeating => Some(ty),
                _ => None,
            }),
        SchemaKind::Scalar { .. } => None,
    }
    .ok_or_else(|| format!("column `{column}` is not scalar"))
}

fn append_child(parent: &mut SchemaNode, child: SchemaNode) -> Result<(), String> {
    let SchemaKind::Group { children, .. } = &mut parent.kind else {
        return Err("parent query table is not a group".to_string());
    };
    children.push(child);
    Ok(())
}

fn read_catalog_ports(
    routine: &CatalogRoutine<'_, '_>,
    parent_schema: &SchemaNode,
    parent_path: &[String],
    child_path: &[String],
    parent_plan: Option<&QueryPlan>,
    child_plan: &QueryPlan,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    if parent_plan.is_some() {
        return Err("inline query catalogs require a physical parent table".to_string());
    }
    let child_tables = routine
        .routine
        .children()
        .filter(|entry| entry.has_tag_name("entry") && entry.attribute("type") == Some("table"))
        .collect::<Vec<_>>();
    let [child_table] = child_tables.as_slice() else {
        return Err("inline query result must contain exactly one table entry".to_string());
    };
    if routine.routine.descendants().any(|entry| {
        entry != routine.routine
            && entry.has_tag_name("entry")
            && entry.attribute("type").is_some_and(|kind| kind != "table")
    }) {
        return Err("inline query result contains unsupported nested entry kinds".to_string());
    }

    let mut ports = BTreeMap::new();
    insert_port(
        &mut ports,
        parse_u32(routine.routine.attribute("outkey"))
            .ok_or_else(|| "inline query collection has an invalid output key".to_string())?,
        child_path.to_vec(),
    )?;
    collect_inline_columns(&routine.parent, parent_schema, parent_path, &[], &mut ports)?;
    collect_inline_columns(
        child_table,
        parent_schema
            .child(child_path.last().map(String::as_str).unwrap_or_default())
            .ok_or_else(|| "inline query child schema is missing".to_string())?,
        child_path,
        &child_plan.selected,
        &mut ports,
    )?;
    Ok(ports)
}

fn collect_inline_columns(
    entry: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    prefix: &[String],
    selected: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    let mut exposed = BTreeSet::new();
    for column in entry.children().filter(|child| {
        child.has_tag_name("entry")
            && child.attribute("type").is_none()
            && child.attribute("outkey").is_some()
    }) {
        let name = column.attribute("name").unwrap_or_default();
        if !selected.is_empty()
            && !selected
                .iter()
                .any(|selected| selected.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "inline query output `{name}` is not selected by its SQL query"
            ));
        }
        if !exposed.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "inline query output `{name}` is exposed more than once ignoring ASCII case"
            ));
        }
        let mut path = prefix.to_vec();
        path.push(canonical_column(schema, name)?);
        insert_port(
            ports,
            parse_u32(column.attribute("outkey"))
                .ok_or_else(|| format!("database output `{name}` has an invalid key"))?,
            path,
        )?;
    }
    if !selected.is_empty()
        && exposed
            != selected
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect()
    {
        return Err("SQL projection does not match the inline query outputs".to_string());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn read_correlated_ports(
    component: &roxmltree::Node<'_, '_>,
    mapping: &roxmltree::Node<'_, '_>,
    parent_table: &str,
    child_query: &str,
    parent_schema: &SchemaNode,
    parent_path: &[String],
    child_path: &[String],
    parent_plan: Option<&QueryPlan>,
    child_plan: &QueryPlan,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let mut ports = BTreeMap::new();
    collect_parent_catalog_ports(
        mapping,
        component,
        parent_table,
        parent_path,
        parent_schema,
        &mut ports,
    )?;
    let collections = component
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("outkey").is_some()
                && entry.attribute("type") != Some("attribute")
        })
        .collect::<Vec<_>>();
    let [collection] = collections.as_slice() else {
        return Err("correlated query must expose exactly one child collection".to_string());
    };
    let collection_name = collection.attribute("name").unwrap_or_default();
    if !same_query_identity(collection_name, child_query) {
        return Err(format!(
            "result collection `{collection_name}` does not match child query `{child_query}`"
        ));
    }
    insert_port(
        &mut ports,
        parse_u32(collection.attribute("outkey"))
            .ok_or_else(|| "child collection has an invalid output key".to_string())?,
        child_path.to_vec(),
    )?;
    let parent_selected = parent_plan
        .map(|plan| plan.selected.as_slice())
        .unwrap_or(&[]);
    for leaf in component.descendants().filter(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("type") == Some("attribute")
            && entry.attribute("outkey").is_some()
    }) {
        let name = leaf.attribute("name").unwrap_or_default();
        let in_child = leaf.ancestors().any(|ancestor| ancestor == *collection);
        let (schema, selected, prefix) = if in_child {
            let child = parent_schema
                .child(child_path.last().map(String::as_str).unwrap_or_default())
                .ok_or_else(|| "child relation schema is missing".to_string())?;
            (child, child_plan.selected.as_slice(), child_path)
        } else {
            (parent_schema, parent_selected, parent_path)
        };
        if !selected
            .iter()
            .any(|column| column.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "query output `{name}` is not selected by its SQL query"
            ));
        }
        let canonical = canonical_column(schema, name)?;
        let mut path = prefix.to_vec();
        path.push(canonical);
        insert_port(
            &mut ports,
            parse_u32(leaf.attribute("outkey"))
                .ok_or_else(|| format!("query output `{name}` has an invalid key"))?,
            path,
        )?;
    }
    Ok(ports)
}

fn collect_parent_catalog_ports(
    mapping: &roxmltree::Node<'_, '_>,
    query_component: &roxmltree::Node<'_, '_>,
    parent_table: &str,
    parent_path: &[String],
    parent_schema: &SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    for catalog in mapping.descendants().filter(|node| {
        node.has_tag_name("component")
            && node.attribute("library") == Some("db")
            && node.attribute("kind") == Some("15")
    }) {
        let owns_query = catalog.descendants().any(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type") == Some("routine")
                && query_component.attribute("name").is_some_and(|query| {
                    entry
                        .attribute("name")
                        .is_some_and(|routine| same_query_identity(query, routine))
                })
        });
        if !owns_query {
            continue;
        }
        let Some(table) = catalog.descendants().find(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type") == Some("table")
                && entry
                    .attribute("name")
                    .is_some_and(|name| name.eq_ignore_ascii_case(parent_table))
        }) else {
            continue;
        };
        for leaf in table.children().filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("type").is_none()
                && entry.attribute("outkey").is_some()
        }) {
            let name = leaf.attribute("name").unwrap_or_default();
            let mut path = parent_path.to_vec();
            path.push(canonical_column(parent_schema, name)?);
            insert_port(
                ports,
                parse_u32(leaf.attribute("outkey"))
                    .ok_or_else(|| format!("database column `{name}` has an invalid key"))?,
                path,
            )?;
        }
    }
    Ok(())
}

fn canonical_column(schema: &SchemaNode, name: &str) -> Result<String, String> {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .find(|child| child.name.eq_ignore_ascii_case(name))
            .map(|child| child.name.clone()),
        SchemaKind::Scalar { .. } => None,
    }
    .ok_or_else(|| format!("typed table has no output column `{name}`"))
}

fn insert_port(
    ports: &mut BTreeMap<u32, Vec<String>>,
    key: u32,
    path: Vec<String>,
) -> Result<(), String> {
    if ports.insert(key, path).is_some() {
        return Err(format!(
            "database output key `{key}` is used more than once"
        ));
    }
    Ok(())
}

fn component_key_sets(component: &roxmltree::Node<'_, '_>) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for root in component
        .children()
        .find(|node| node.has_tag_name("data"))
        .into_iter()
        .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
    {
        let (root_inputs, root_outputs) = entry_key_sets(&root);
        inputs.extend(root_inputs);
        outputs.extend(root_outputs);
    }
    (inputs, outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_plan(name: &str, selected: &[&str]) -> QueryPlan {
        QueryPlan {
            name: name.to_string(),
            table: name.to_string(),
            selected: selected.iter().map(|name| (*name).to_string()).collect(),
            predicates: Vec::new(),
            order: None,
            correlated_type: None,
            correlated_column: None,
        }
    }

    fn parent_schema() -> SchemaNode {
        SchemaNode::group(
            "Department",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Person|ForeignKey",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("ForeignKey", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating()
    }

    #[test]
    fn correlated_ports_expose_only_projected_columns() {
        let component = roxmltree::Document::parse(
            r#"<component name="People|DepartmentId"><entry name="People" outkey="1"><entry name="First" type="attribute" outkey="2"/><entry name="ForeignKey" type="attribute" outkey="3"/></entry></component>"#,
        )
        .unwrap();
        let mapping = roxmltree::Document::parse("<mapping/>").unwrap();
        let error = read_correlated_ports(
            &component.root_element(),
            &mapping.root_element(),
            "Department",
            "People",
            &parent_schema(),
            &["Department".to_string()],
            &["Department".to_string(), "Person|ForeignKey".to_string()],
            None,
            &query_plan("Person", &["First"]),
        )
        .unwrap_err();
        assert!(error.contains("`ForeignKey` is not selected"), "{error}");
    }

    #[test]
    fn correlated_collection_name_requires_a_typed_exact_match() {
        let component = roxmltree::Document::parse(
            r#"<component name="People"><entry name="PeopleExtra" outkey="1"><entry name="First" type="attribute" outkey="2"/></entry></component>"#,
        )
        .unwrap();
        let mapping = roxmltree::Document::parse("<mapping/>").unwrap();
        let error = read_correlated_ports(
            &component.root_element(),
            &mapping.root_element(),
            "Department",
            "People",
            &parent_schema(),
            &["Department".to_string()],
            &["Department".to_string(), "Person|ForeignKey".to_string()],
            None,
            &query_plan("Person", &["First"]),
        )
        .unwrap_err();
        assert!(error.contains("does not match child query"), "{error}");
    }
}
