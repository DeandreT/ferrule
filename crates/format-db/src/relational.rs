use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, ValueGeneration};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Transaction, params, params_from_iter};

use super::{
    DbFormatError, ForeignKeyColumns, ForeignKeyRelation, ForeignKeySide, quote, read, read_value,
};

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
    foreign_key_side: ForeignKeySide,
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
    if is_flat_table(schema) {
        return read(db_path, schema).map(Instance::Repeated);
    }

    let conn = Connection::open(db_path)?;
    if schema.repeating {
        let physical_table = physical_table_name(&schema.name)?;
        let plan = build_table_plan(&conn, schema, physical_table)?;
        let rows = read_table(&conn, &plan, None)?;
        return Ok(Instance::Repeated(rows));
    }

    let SchemaKind::Group { children, .. } = &schema.kind else {
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

pub(super) fn validate_schema(db_path: &Path, schema: &SchemaNode) -> Result<(), DbFormatError> {
    let conn = Connection::open(db_path)?;
    if schema.repeating {
        let physical_table = physical_table_name(&schema.name)?;
        build_table_plan(&conn, schema, physical_table)?;
        return Ok(());
    }

    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(invalid_schema(schema, "database root must be a group"));
    };
    if children.is_empty() {
        return Err(invalid_schema(
            schema,
            "composite database root must contain at least one table",
        ));
    }
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
        build_table_plan(&conn, table, &table.name)?;
    }
    Ok(())
}

pub(super) fn resolve_foreign_key_relation(
    db_path: &Path,
    parent_table: &str,
    parent_column: &str,
    child_table: &str,
    child_column: &str,
) -> Result<ForeignKeyRelation, DbFormatError> {
    let conn = Connection::open(db_path)?;
    let mut matches = Vec::new();
    matches.extend(
        foreign_keys(&conn, child_table)?
            .into_iter()
            .filter(|key| {
                key.from.eq_ignore_ascii_case(child_column)
                    && key.table.eq_ignore_ascii_case(parent_table)
                    && key.to.eq_ignore_ascii_case(parent_column)
            })
            .map(|key| ForeignKeyRelation {
                side: ForeignKeySide::Child,
                join_column: key.from,
            }),
    );
    matches.extend(
        foreign_keys(&conn, parent_table)?
            .into_iter()
            .filter(|key| {
                key.from.eq_ignore_ascii_case(parent_column)
                    && key.table.eq_ignore_ascii_case(child_table)
                    && key.to.eq_ignore_ascii_case(child_column)
            })
            .map(|key| ForeignKeyRelation {
                side: ForeignKeySide::Parent,
                join_column: key.from,
            }),
    );
    match matches.as_slice() {
        [relation] => Ok(relation.clone()),
        [] => Err(DbFormatError::MissingForeignKeyEndpoints {
            parent_table: parent_table.to_string(),
            parent_column: parent_column.to_string(),
            child_table: child_table.to_string(),
            child_column: child_column.to_string(),
        }),
        _ => Err(DbFormatError::AmbiguousForeignKeyEndpoints {
            parent_table: parent_table.to_string(),
            parent_column: parent_column.to_string(),
            child_table: child_table.to_string(),
            child_column: child_column.to_string(),
        }),
    }
}

pub(super) fn resolve_foreign_key_columns(
    db_path: &Path,
    parent_table: &str,
    child_table: &str,
    join_column: &str,
) -> Result<ForeignKeyColumns, DbFormatError> {
    let conn = Connection::open(db_path)?;
    relation_columns(&conn, parent_table, child_table, join_column)
}

pub(super) fn is_flat_table(schema: &SchemaNode) -> bool {
    matches!(
        &schema.kind,
        SchemaKind::Group { children, .. }
            if children
                .iter()
                .all(|child| matches!(child.kind, SchemaKind::Scalar { .. }) && !child.repeating)
    )
}

pub(super) fn write_instance(
    db_path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), DbFormatError> {
    let mut conn = Connection::open(db_path)?;
    let plans = write_plans(&conn, schema)?;
    for plan in &plans {
        validate_write_plan(&conn, plan)?;
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    let tx = conn.transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    let mut deleted = BTreeSet::new();
    for plan in &plans {
        delete_tables(&tx, plan, &mut deleted)?;
    }
    let mut generated = BTreeMap::new();
    match instance {
        Instance::Repeated(rows) if schema.repeating && plans.len() == 1 => {
            for (index, row) in rows.iter().enumerate() {
                insert_row(&tx, &plans[0], row, None, index, &mut generated)?;
            }
        }
        Instance::Group(fields) if !schema.repeating => {
            for (plan, table) in plans.iter().zip(schema_children(schema)?) {
                let rows = fields
                    .iter()
                    .find(|(name, _)| name == &table.name)
                    .map(|(_, value)| value)
                    .and_then(Instance::as_repeated)
                    .ok_or_else(|| invalid_schema(table, "database table value must repeat"))?;
                for (index, row) in rows.iter().enumerate() {
                    insert_row(&tx, plan, row, None, index, &mut generated)?;
                }
            }
        }
        other => {
            return Err(invalid_schema(
                schema,
                match other {
                    Instance::Repeated(_) => "composite database value must be a group",
                    _ if schema.repeating => "database table value must repeat",
                    _ => "composite database value must be a group",
                },
            ));
        }
    }
    tx.commit()?;
    Ok(())
}

fn write_plans<'a>(
    conn: &Connection,
    schema: &'a SchemaNode,
) -> Result<Vec<TablePlan<'a>>, DbFormatError> {
    if schema.repeating {
        let physical = physical_table_name(&schema.name)?;
        return Ok(vec![build_table_plan(conn, schema, physical)?]);
    }
    schema_children(schema)?
        .iter()
        .map(|table| build_table_plan(conn, table, &table.name))
        .collect()
}

fn schema_children(schema: &SchemaNode) -> Result<&[SchemaNode], DbFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(invalid_schema(schema, "database root must be a group"));
    };
    Ok(children)
}

fn validate_write_plan(conn: &Connection, plan: &TablePlan<'_>) -> Result<(), DbFormatError> {
    if !plan.relations.is_empty() && !supports_rowid(conn, &plan.physical_table) {
        return Err(invalid_schema(
            plan.schema,
            "relational writes require rowid tables when resolving relationship keys",
        ));
    }
    for relation in &plan.relations {
        if !supports_rowid(conn, &relation.child.physical_table) {
            return Err(invalid_schema(
                relation.child.schema,
                "relational writes require rowid tables when resolving relationship keys",
            ));
        }
        validate_write_plan(conn, &relation.child)?;
    }
    Ok(())
}

fn delete_tables(
    tx: &Transaction<'_>,
    plan: &TablePlan<'_>,
    deleted: &mut BTreeSet<String>,
) -> Result<(), DbFormatError> {
    for relation in &plan.relations {
        delete_tables(tx, &relation.child, deleted)?;
    }
    if !deleted.insert(plan.physical_table.to_ascii_lowercase()) {
        return Ok(());
    }
    tx.execute(&format!("DELETE FROM {}", quote(&plan.physical_table)), [])?;
    Ok(())
}

struct InsertedRow {
    rowid: i64,
}

fn insert_row(
    tx: &Transaction<'_>,
    plan: &TablePlan<'_>,
    instance: &Instance,
    inherited: Option<(&str, SqlValue)>,
    row_index: usize,
    generated: &mut BTreeMap<(String, String), i64>,
) -> Result<InsertedRow, DbFormatError> {
    let Instance::Group(fields) = instance else {
        return Err(DbFormatError::RowShape {
            row: row_index,
            got: super::instance_type_name(instance),
        });
    };
    validate_row_fields(plan, fields, row_index)?;

    let mut overrides = inherited
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect::<Vec<_>>();
    for relation in &plan.relations {
        if relation.foreign_key_side != ForeignKeySide::Parent {
            continue;
        }
        let children = relation_rows(fields, relation)?;
        let value = match children {
            [] => SqlValue::Null,
            [child] => {
                let inserted = insert_row(tx, &relation.child, child, None, 0, generated)?;
                inserted_key(tx, &relation.child, &relation.child_key, inserted.rowid)?
            }
            _ => {
                return Err(invalid_schema(
                    relation.child.schema,
                    "a parent-owned foreign key can reference at most one nested row",
                ));
            }
        };
        overrides.push((relation.parent_key.clone(), value));
    }

    let mut columns = Vec::new();
    let mut values = Vec::new();
    for (name, ty) in &plan.scalar_columns {
        if let Some((_, override_value)) = overrides
            .iter()
            .find(|(column, _)| name.eq_ignore_ascii_case(column))
        {
            columns.push((*name).to_string());
            values.push(override_value.clone());
            continue;
        }
        if generated_column(plan, name).is_some() {
            let value = fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value);
            if value.is_some_and(|value| !matches!(value, Instance::Scalar(ir::Value::Null))) {
                return Err(DbFormatError::GeneratedFieldSupplied {
                    row: row_index,
                    column: (*name).to_string(),
                });
            }
            let key = (
                plan.physical_table.to_ascii_lowercase(),
                name.to_ascii_lowercase(),
            );
            let next = generated
                .get(&key)
                .copied()
                .unwrap_or_default()
                .checked_add(1)
                .ok_or_else(|| DbFormatError::GeneratedValueOverflow {
                    column: (*name).to_string(),
                })?;
            generated.insert(key, next);
            columns.push((*name).to_string());
            values.push(SqlValue::Integer(next));
            continue;
        }
        let value = fields
            .iter()
            .find(|(field, _)| field == name)
            .and_then(|(_, value)| value.as_scalar())
            .ok_or_else(|| DbFormatError::MissingField {
                row: row_index,
                column: (*name).to_string(),
            })?;
        if *value != ir::Value::Null {
            columns.push((*name).to_string());
            values.push(super::to_sql_value(name, *ty, value)?);
        }
    }
    for (foreign_key, inherited) in overrides {
        if columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case(&foreign_key))
        {
            continue;
        }
        columns.push(foreign_key);
        values.push(inherited);
    }
    let sql = if columns.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", quote(&plan.physical_table))
    } else {
        let names = columns
            .iter()
            .map(|name| quote(name))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = (1..=columns.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "INSERT INTO {} ({names}) VALUES ({placeholders})",
            quote(&plan.physical_table)
        )
    };
    tx.execute(&sql, params_from_iter(values))?;
    let inserted = InsertedRow {
        rowid: tx.last_insert_rowid(),
    };

    for relation in &plan.relations {
        if relation.foreign_key_side != ForeignKeySide::Child {
            continue;
        }
        let parent_value = inserted_key(tx, plan, &relation.parent_key, inserted.rowid)?;
        let children = relation_rows(fields, relation)?;
        for (index, child) in children.iter().enumerate() {
            insert_row(
                tx,
                &relation.child,
                child,
                Some((&relation.child_key, parent_value.clone())),
                index,
                generated,
            )?;
        }
    }
    Ok(inserted)
}

fn validate_row_fields(
    plan: &TablePlan<'_>,
    fields: &[(String, Instance)],
    row_index: usize,
) -> Result<(), DbFormatError> {
    let SchemaKind::Group { children, .. } = &plan.schema.kind else {
        return Err(invalid_schema(plan.schema, "table must be a group"));
    };
    for (index, (name, _)) in fields.iter().enumerate() {
        if !children.iter().any(|child| child.name == *name) {
            return Err(DbFormatError::UnexpectedField {
                row: row_index,
                column: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(DbFormatError::DuplicateField {
                row: row_index,
                column: name.clone(),
            });
        }
    }
    Ok(())
}

fn relation_rows<'a>(
    fields: &'a [(String, Instance)],
    relation: &Relation<'_>,
) -> Result<&'a [Instance], DbFormatError> {
    fields
        .iter()
        .find(|(name, _)| name == &relation.child.schema.name)
        .map(|(_, value)| value)
        .and_then(Instance::as_repeated)
        .ok_or_else(|| {
            invalid_schema(
                relation.child.schema,
                "relationship value must be a repeated group",
            )
        })
}

fn inserted_key(
    tx: &Transaction<'_>,
    plan: &TablePlan<'_>,
    column: &str,
    rowid: i64,
) -> Result<SqlValue, DbFormatError> {
    tx.query_row(
        &format!(
            "SELECT {} FROM {} WHERE rowid = ?1",
            quote(column),
            quote(&plan.physical_table)
        ),
        [rowid],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn build_table_plan<'a>(
    conn: &Connection,
    schema: &'a SchemaNode,
    physical_table: &str,
) -> Result<TablePlan<'a>, DbFormatError> {
    if !schema.repeating {
        return Err(invalid_schema(schema, "table groups must be repeating"));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
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
    let SchemaKind::Group { children, .. } = &plan.schema.kind else {
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
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(invalid_schema(schema, "table must be a group"));
    };
    children
        .iter()
        .filter_map(|child| match &child.kind {
            SchemaKind::Scalar { ty } if !child.repeating => {
                if child.value_generation.is_some()
                    && !matches!(
                        (&child.kind, child.value_generation),
                        (
                            SchemaKind::Scalar {
                                ty: ScalarType::Int
                            },
                            Some(ValueGeneration::MaxNumber)
                        )
                    )
                {
                    Some(Err(DbFormatError::InvalidGeneratedColumn {
                        column: child.name.clone(),
                    }))
                } else {
                    Some(Ok((child.name.as_str(), *ty)))
                }
            }
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

fn generated_column(plan: &TablePlan<'_>, name: &str) -> Option<ValueGeneration> {
    plan.schema
        .child(name)
        .and_then(|column| column.value_generation)
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

    if !schema.database_relation_is_valid() {
        return Err(invalid_schema(
            schema,
            "declared database relation metadata is inconsistent",
        ));
    }
    let (columns, foreign_key_side) = match &schema.database_relation {
        Some(relation) => {
            require_column(conn, parent_table, &relation.parent_column)?;
            require_column(conn, child_table, &relation.child_column)?;
            (
                ForeignKeyColumns {
                    parent_column: relation.parent_column.clone(),
                    child_column: relation.child_column.clone(),
                },
                relation.foreign_key_side.into(),
            )
        }
        None => relation_columns_with_side(conn, parent_table, child_table, join_column)?,
    };
    let child = build_table_plan(conn, schema, child_table)?;
    Ok(Relation {
        child: Box::new(child),
        parent_key: columns.parent_column,
        child_key: columns.child_column,
        foreign_key_side,
    })
}

fn require_column(conn: &Connection, table: &str, column: &str) -> Result<(), DbFormatError> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|name| name.eq_ignore_ascii_case(column));
    if !exists {
        return Err(DbFormatError::MissingDeclaredRelationColumn {
            table: table.to_string(),
            column: column.to_string(),
        });
    }
    Ok(())
}

fn relation_columns(
    conn: &Connection,
    parent_table: &str,
    child_table: &str,
    join_column: &str,
) -> Result<ForeignKeyColumns, DbFormatError> {
    relation_columns_with_side(conn, parent_table, child_table, join_column)
        .map(|(columns, _)| columns)
}

fn relation_columns_with_side(
    conn: &Connection,
    parent_table: &str,
    child_table: &str,
    join_column: &str,
) -> Result<(ForeignKeyColumns, ForeignKeySide), DbFormatError> {
    let child_foreign_keys = foreign_keys(conn, child_table)?;
    let parent_foreign_keys = foreign_keys(conn, parent_table)?;
    let mut candidates = Vec::new();
    for key in child_foreign_keys {
        if key.from.eq_ignore_ascii_case(join_column)
            && key.table.eq_ignore_ascii_case(parent_table)
        {
            candidates.push((key.to, key.from, ForeignKeySide::Child));
        }
    }
    for key in parent_foreign_keys {
        if key.from.eq_ignore_ascii_case(join_column) && key.table.eq_ignore_ascii_case(child_table)
        {
            candidates.push((key.from, key.to, ForeignKeySide::Parent));
        }
    }

    let (parent_column, child_column, side) = match candidates.as_slice() {
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
    Ok((
        ForeignKeyColumns {
            parent_column,
            child_column,
        },
        side,
    ))
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
