use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use super::sql::{ColumnRef, JoinedProjectionExpr, JoinedQuery};
use super::{
    DbComputedExpression, DbQuery, ParsedOperand, Parser, QueryCardinality, QueryOperand,
    QueryOperator, QueryPredicate, read_parameter_keys, read_parameter_types,
};
use crate::import::schema::{ComponentFormat, SchemaComponent, parse_u32};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableSide {
    Primary,
    Joined,
}

#[derive(Clone)]
struct ResolvedColumn {
    side: TableSide,
    name: String,
    ty: ScalarType,
}

pub(super) fn read_component(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    connection: &roxmltree::Node<'_, '_>,
    query_name: &str,
) -> Result<SchemaComponent, String> {
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
    let parsed = Parser::new(sql)?.parse_joined()?;

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
    let primary = format_db::introspect(&db_path, &parsed.primary_table).map_err(|error| {
        format!(
            "could not introspect joined-query table `{}` ({error})",
            parsed.primary_table
        )
    })?;
    let joined = format_db::introspect(&db_path, &parsed.joined_table).map_err(|error| {
        format!(
            "could not introspect joined-query table `{}` ({error})",
            parsed.joined_table
        )
    })?;

    let (primary_join, joined_join) = resolve_join_endpoints(&parsed, &primary, &joined)?;
    let relation = format_db::resolve_foreign_key_relation(
        &db_path,
        &primary.name,
        &primary_join,
        &joined.name,
        &joined_join,
    )
    .map_err(|error| format!("joined query does not match one SQLite foreign key ({error})"))?;
    if relation.side != format_db::ForeignKeySide::Parent {
        return Err(
            "joined query must follow a many-to-one foreign key owned by its FROM table"
                .to_string(),
        );
    }
    let relation_name = format!("{}|{}", joined.name, relation.join_column);

    let mut resolved = BTreeMap::new();
    let mut primary_fields = BTreeSet::new();
    let mut joined_fields = BTreeSet::from([joined_join.clone()]);
    for projection in &parsed.projections {
        let expression = match &projection.expr {
            JoinedProjectionExpr::Column(column) => {
                let column = resolve_column(column, &primary, &joined)?;
                note_field(&column, &mut primary_fields, &mut joined_fields);
                ResolvedProjection::Column(column)
            }
            JoinedProjectionExpr::Multiply(left, right) => {
                let left = resolve_column(left, &primary, &joined)?;
                let right = resolve_column(right, &primary, &joined)?;
                if !is_numeric(left.ty) || !is_numeric(right.ty) {
                    return Err(format!(
                        "computed query output `{}` multiplies a non-numeric column",
                        projection.output
                    ));
                }
                note_field(&left, &mut primary_fields, &mut joined_fields);
                note_field(&right, &mut primary_fields, &mut joined_fields);
                ResolvedProjection::Multiply(left, right)
            }
        };
        resolved.insert(projection.output.to_ascii_lowercase(), expression);
    }

    let predicate_column = resolve_column(&parsed.predicate_column, &primary, &joined)?;
    if predicate_column.side != TableSide::Primary {
        return Err("joined query predicate must reference its FROM table".to_string());
    }
    primary_fields.insert(predicate_column.name.clone());

    let declared_parameters = read_parameter_types(local_view)?;
    let parameter_keys = read_parameter_keys(component)?;
    let predicate_operand =
        match parsed.predicate_operand {
            ParsedOperand::Literal(value) => QueryOperand::Literal(value),
            ParsedOperand::Parameter(name) => QueryOperand::Parameter {
                ty: declared_parameters.get(&name).copied().ok_or_else(|| {
                    format!("SQL parameter `:{name}` has no matching declaration")
                })?,
                input_key: parameter_keys.get(&name).copied().ok_or_else(|| {
                    format!("SQL parameter `:{name}` has no matching query input")
                })?,
                name,
            },
        };

    let output_ports = read_output_ports(component, &resolved, &relation_name)?;
    let joined_children = select_fields(&joined, &joined_fields)?;
    let mut primary_children = select_fields(&primary, &primary_fields)?;
    primary_children.push(SchemaNode::group(relation_name.clone(), joined_children).repeating());
    let schema = SchemaNode::group(primary.name.clone(), primary_children).repeating();

    format_db::validate_relational_schema(&db_path, &schema).map_err(|error| {
        format!("joined query could not be represented by the relational reader ({error})")
    })?;

    let input_keys = component
        .descendants()
        .filter(|entry| entry.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
        .collect();
    let output_keys = component
        .descendants()
        .filter(|entry| entry.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .collect();
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
        ports: output_ports.paths,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: vec![DbQuery {
            name: query_name.to_string(),
            collection: Vec::new(),
            predicates: vec![QueryPredicate {
                column: predicate_column.name,
                operator: QueryOperator::Greater,
                operand: predicate_operand,
            }],
            order: None,
            cardinality: QueryCardinality::Many,
            required_paths: vec![vec![relation_name, joined_join]],
            computed_ports: output_ports.computed,
        }],
        dynamic_json: None,
    })
}

enum ResolvedProjection {
    Column(ResolvedColumn),
    Multiply(ResolvedColumn, ResolvedColumn),
}

struct OutputPorts {
    paths: BTreeMap<u32, Vec<String>>,
    computed: BTreeMap<u32, DbComputedExpression>,
}

fn resolve_join_endpoints(
    query: &JoinedQuery,
    primary: &SchemaNode,
    joined: &SchemaNode,
) -> Result<(String, String), String> {
    let left = resolve_column(&query.join_left, primary, joined)?;
    let right = resolve_column(&query.join_right, primary, joined)?;
    match (left.side, right.side) {
        (TableSide::Primary, TableSide::Joined) => Ok((left.name, right.name)),
        (TableSide::Joined, TableSide::Primary) => Ok((right.name, left.name)),
        _ => Err("joined query equality must connect its two declared tables".to_string()),
    }
}

fn resolve_column(
    column: &ColumnRef,
    primary: &SchemaNode,
    joined: &SchemaNode,
) -> Result<ResolvedColumn, String> {
    let requested_side = column
        .table
        .as_deref()
        .map(|table| {
            if table.eq_ignore_ascii_case(&primary.name) {
                Ok(TableSide::Primary)
            } else if table.eq_ignore_ascii_case(&joined.name) {
                Ok(TableSide::Joined)
            } else {
                Err(format!("joined query references unknown table `{table}`"))
            }
        })
        .transpose()?;
    let mut matches = [(TableSide::Primary, primary), (TableSide::Joined, joined)]
        .into_iter()
        .filter(|(side, _)| requested_side.is_none_or(|wanted| wanted == *side))
        .filter_map(|(side, schema)| {
            scalar_child(schema, &column.column).map(|child| (side, child))
        });
    let Some((side, child)) = matches.next() else {
        return Err(format!(
            "joined query column `{}` is not in the selected table",
            column.column
        ));
    };
    if matches.next().is_some() {
        return Err(format!(
            "unqualified joined query column `{}` is ambiguous",
            column.column
        ));
    }
    let SchemaKind::Scalar { ty } = &child.kind else {
        return Err(format!(
            "joined query column `{}` is not scalar",
            child.name
        ));
    };
    Ok(ResolvedColumn {
        side,
        name: child.name.clone(),
        ty: *ty,
    })
}

fn scalar_child<'a>(schema: &'a SchemaNode, name: &str) -> Option<&'a SchemaNode> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find(|child| child.name.eq_ignore_ascii_case(name))
}

fn is_numeric(ty: ScalarType) -> bool {
    matches!(ty, ScalarType::Int | ScalarType::Float)
}

fn note_field(
    column: &ResolvedColumn,
    primary: &mut BTreeSet<String>,
    joined: &mut BTreeSet<String>,
) {
    match column.side {
        TableSide::Primary => primary.insert(column.name.clone()),
        TableSide::Joined => joined.insert(column.name.clone()),
    };
}

fn select_fields(
    schema: &SchemaNode,
    selected: &BTreeSet<String>,
) -> Result<Vec<SchemaNode>, String> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(format!("database table `{}` is not a group", schema.name));
    };
    Ok(children
        .iter()
        .filter(|child| selected.contains(&child.name))
        .cloned()
        .collect())
}

fn read_output_ports(
    component: &roxmltree::Node<'_, '_>,
    projections: &BTreeMap<String, ResolvedProjection>,
    relation_name: &str,
) -> Result<OutputPorts, String> {
    let collections = component
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.attribute("outkey").is_some()
                && entry.attribute("type") != Some("attribute")
        })
        .collect::<Vec<_>>();
    let [collection] = collections.as_slice() else {
        return Err("joined query result must expose exactly one collection output".to_string());
    };
    let collection_key = parse_u32(collection.attribute("outkey"))
        .ok_or_else(|| "joined query collection has an invalid output key".to_string())?;
    let mut ports = BTreeMap::from([(collection_key, Vec::new())]);
    let mut computed = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for entry in collection.descendants().filter(|entry| {
        entry.has_tag_name("entry")
            && entry.attribute("type") == Some("attribute")
            && entry.attribute("outkey").is_some()
    }) {
        let name = entry.attribute("name").unwrap_or_default();
        let key = parse_u32(entry.attribute("outkey"))
            .ok_or_else(|| format!("joined query output `{name}` has an invalid key"))?;
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "joined query exposes output `{name}` more than once"
            ));
        }
        let projection = projections
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| format!("joined query output `{name}` is absent from SQL projection"))?;
        match projection {
            ResolvedProjection::Column(column) => {
                ports.insert(key, column_path(column, relation_name));
            }
            ResolvedProjection::Multiply(left, right) => {
                computed.insert(
                    key,
                    DbComputedExpression::Multiply {
                        left: column_path(left, relation_name),
                        right: column_path(right, relation_name),
                        ty: if left.ty == ScalarType::Float || right.ty == ScalarType::Float {
                            ScalarType::Float
                        } else {
                            ScalarType::Int
                        },
                    },
                );
            }
        }
    }
    if seen != projections.keys().cloned().collect() {
        return Err("SQL projection does not match the joined query outputs".to_string());
    }
    Ok(OutputPorts {
        paths: ports,
        computed,
    })
}

fn column_path(column: &ResolvedColumn, relation_name: &str) -> Vec<String> {
    match column.side {
        TableSide::Primary => vec![column.name.clone()],
        TableSide::Joined => vec![relation_name.to_string(), column.name.clone()],
    }
}
