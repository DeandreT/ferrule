use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params};

use super::{DbFormatError, quote, read, read_value};

struct TablePlan<'a> {
    schema: &'a SchemaNode,
    physical_table: String,
    scalar_columns: Vec<(&'a str, ScalarType)>,
    relations: Vec<Relation<'a>>,
}

struct Relation<'a> {
    child: Box<TablePlan<'a>>,
    parent_key: String,
    child_key: String,
}

struct SelectedColumn {
    name: String,
}

#[derive(Clone)]
struct ForeignKey {
    from: String,
    table: String,
    to: String,
}

pub(super) fn read_instance(
    db_path: &Path,
    schema: &SchemaNode,
) -> Result<Instance, DbFormatError> {
    if schema.repeating && is_flat_table(schema) {
        return read(db_path, schema).map(Instance::Repeated);
    }

    let conn = Connection::open(db_path)?;
    if schema.repeating {
        let physical_table = physical_table_name(&schema.name)?;
        let plan = build_table_plan(&conn, schema, physical_table)?;
        let rows = read_table(&conn, &plan, None)?;
        return Ok(Instance::Repeated(rows));
    }

    let SchemaKind::Group { children } = &schema.kind else {
        return Err(invalid_schema(schema, "database root must be a group"));
    };
    if children.is_empty() {
        return Err(invalid_schema(
            schema,
            "composite database root must contain at least one table",
        ));
    }

    let mut fields = Vec::with_capacity(children.len());
    for table in children {
        if !table.repeating || !matches!(table.kind, SchemaKind::Group { .. }) {
            return Err(invalid_schema(
                table,
                "composite database children must be repeating table groups",
            ));
        }
        if table.name.contains('|') {
            return Err(invalid_schema(
                table,
                "top-level table names cannot contain a relationship join column",
            ));
        }
        let plan = build_table_plan(&conn, table, &table.name)?;
        let rows = read_table(&conn, &plan, None)?;
        fields.push((table.name.clone(), Instance::Repeated(rows)));
    }
    Ok(Instance::Group(fields))
}

fn is_flat_table(schema: &SchemaNode) -> bool {
    matches!(
        &schema.kind,
        SchemaKind::Group { children }
            if children
                .iter()
                .all(|child| matches!(child.kind, SchemaKind::Scalar { .. }) && !child.repeating)
    )
}

fn build_table_plan<'a>(
    conn: &Connection,
    schema: &'a SchemaNode,
    physical_table: &str,
) -> Result<TablePlan<'a>, DbFormatError> {
    if !schema.repeating {
        return Err(invalid_schema(schema, "table groups must be repeating"));
    }
    let SchemaKind::Group { children } = &schema.kind else {
        return Err(invalid_schema(schema, "table must be a group"));
    };

    let scalar_columns = columns_of_relational(schema)?;
    let relations = children
        .iter()
        .filter(|child| matches!(child.kind, SchemaKind::Group { .. }))
        .map(|child| resolve_relation(conn, physical_table, child))
        .collect::<Result<Vec<_>, _>>()?;
    if scalar_columns.is_empty() && relations.is_empty() {
        return Err(invalid_schema(
            schema,
            "table must select at least one scalar column or relationship",
        ));
    }
    Ok(TablePlan {
        schema,
        physical_table: physical_table.to_string(),
        scalar_columns,
        relations,
    })
}

fn read_table(
    conn: &Connection,
    plan: &TablePlan<'_>,
    constraint: Option<(&str, &SqlValue)>,
) -> Result<Vec<Instance>, DbFormatError> {
    let SchemaKind::Group { children } = &plan.schema.kind else {
        return Err(invalid_schema(plan.schema, "table must be a group"));
    };
    let selected = selected_columns(&plan.scalar_columns, &plan.relations);
    let column_list = selected
        .iter()
        .map(|column| quote(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let where_clause = constraint
        .map(|(column, _)| format!(" WHERE {} = ?1", quote(column)))
        .unwrap_or_default();
    let order = if supports_rowid(conn, &plan.physical_table) {
        " ORDER BY rowid"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {column_list} FROM {}{where_clause}{order}",
        quote(&plan.physical_table)
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = match constraint {
        Some((_, value)) => stmt.query(params![value])?,
        None => stmt.query([])?,
    };
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut fields = Vec::with_capacity(children.len());
        let mut relations = plan.relations.iter();
        for child in children {
            match &child.kind {
                SchemaKind::Scalar { ty } => {
                    let index = selected
                        .iter()
                        .position(|column| column.name.eq_ignore_ascii_case(&child.name))
                        .ok_or_else(|| {
                            invalid_schema(plan.schema, "scalar column was not selected")
                        })?;
                    fields.push((
                        child.name.clone(),
                        Instance::Scalar(read_value(&child.name, row.get_ref(index)?, *ty)?),
                    ));
                }
                SchemaKind::Group { .. } => {
                    let relation = relations.next().ok_or_else(|| {
                        invalid_schema(plan.schema, "relationship plan was not resolved")
                    })?;
                    let parent_index = selected
                        .iter()
                        .position(|column| column.name.eq_ignore_ascii_case(&relation.parent_key))
                        .ok_or_else(|| {
                            invalid_schema(plan.schema, "relationship key was not selected")
                        })?;
                    let parent_value: SqlValue = row.get(parent_index)?;
                    let related = read_table(
                        conn,
                        &relation.child,
                        Some((&relation.child_key, &parent_value)),
                    )?;
                    fields.push((child.name.clone(), Instance::Repeated(related)));
                }
            }
        }
        if relations.next().is_some() {
            return Err(invalid_schema(
                plan.schema,
                "relationship plan does not match the schema",
            ));
        }
        out.push(Instance::Group(fields));
    }
    Ok(out)
}

fn columns_of_relational(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, DbFormatError> {
    let SchemaKind::Group { children } = &schema.kind else {
        return Err(invalid_schema(schema, "table must be a group"));
    };
    children
        .iter()
        .filter_map(|child| match &child.kind {
            SchemaKind::Scalar { ty } if !child.repeating => Some(Ok((child.name.as_str(), *ty))),
            SchemaKind::Scalar { .. } => Some(Err(invalid_schema(
                child,
                "database scalar columns cannot repeat",
            ))),
            SchemaKind::Group { .. } if child.repeating => None,
            SchemaKind::Group { .. } => Some(Err(invalid_schema(
                child,
                "relationship groups must be repeating",
            ))),
        })
        .collect()
}

fn selected_columns(
    scalars: &[(&str, ScalarType)],
    relations: &[Relation<'_>],
) -> Vec<SelectedColumn> {
    let mut selected = scalars
        .iter()
        .map(|(name, _)| SelectedColumn {
            name: (*name).to_string(),
        })
        .collect::<Vec<_>>();
    for relation in relations {
        if !selected
            .iter()
            .any(|column| column.name.eq_ignore_ascii_case(&relation.parent_key))
        {
            selected.push(SelectedColumn {
                name: relation.parent_key.clone(),
            });
        }
    }
    selected
}

fn resolve_relation<'a>(
    conn: &Connection,
    parent_table: &str,
    schema: &'a SchemaNode,
) -> Result<Relation<'a>, DbFormatError> {
    let (child_table, join_column) = schema.name.split_once('|').ok_or_else(|| {
        invalid_schema(
            schema,
            "nested relation names must use `PhysicalTable|JoinColumn`",
        )
    })?;
    if child_table.is_empty() || join_column.is_empty() || join_column.contains('|') {
        return Err(invalid_schema(
            schema,
            "nested relation names must contain one non-empty table and join column",
        ));
    }

    let child_foreign_keys = foreign_keys(conn, child_table)?;
    let parent_foreign_keys = foreign_keys(conn, parent_table)?;
    let mut candidates = Vec::new();
    for key in child_foreign_keys {
        if key.from.eq_ignore_ascii_case(join_column)
            && key.table.eq_ignore_ascii_case(parent_table)
        {
            candidates.push((key.to, key.from));
        }
    }
    for key in parent_foreign_keys {
        if key.from.eq_ignore_ascii_case(join_column) && key.table.eq_ignore_ascii_case(child_table)
        {
            candidates.push((key.from, key.to));
        }
    }

    let (parent_key, child_key) = match candidates.as_slice() {
        [] => {
            return Err(DbFormatError::MissingForeignKeyRelation {
                parent_table: parent_table.to_string(),
                child_table: child_table.to_string(),
                join_column: join_column.to_string(),
            });
        }
        [candidate] => candidate.clone(),
        _ => {
            return Err(DbFormatError::AmbiguousForeignKeyRelation {
                parent_table: parent_table.to_string(),
                child_table: child_table.to_string(),
                join_column: join_column.to_string(),
            });
        }
    };
    let child = build_table_plan(conn, schema, child_table)?;
    Ok(Relation {
        child: Box::new(child),
        parent_key,
        child_key,
    })
}

fn foreign_keys(conn: &Connection, table: &str) -> Result<Vec<ForeignKey>, DbFormatError> {
    let mut stmt = conn.prepare(&format!("PRAGMA foreign_key_list({})", quote(table)))?;
    let foreign_keys = stmt
        .query_map([], |row| {
            Ok(ForeignKey {
                table: row.get("table")?,
                from: row.get("from")?,
                to: row.get("to")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(foreign_keys)
}

fn supports_rowid(conn: &Connection, table: &str) -> bool {
    conn.prepare(&format!("SELECT rowid FROM {} LIMIT 0", quote(table)))
        .is_ok()
}

fn physical_table_name(name: &str) -> Result<&str, DbFormatError> {
    match name.split_once('|') {
        None if !name.is_empty() => Ok(name),
        _ => Err(DbFormatError::InvalidRelationalSchema {
            node: name.to_string(),
            reason: "top-level table names cannot contain a relationship join column",
        }),
    }
}

fn invalid_schema(node: &SchemaNode, reason: &'static str) -> DbFormatError {
    DbFormatError::InvalidRelationalSchema {
        node: node.name.clone(),
        reason,
    }
}
