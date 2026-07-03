//! Database schema introspection and instance read/write.
//!
//! v1 targets SQLite via `rusqlite` (synchronous, bundled -- no external
//! service needed); other engines can arrive later behind the same
//! interface. The convention mirroring the other flat-rows formats: a
//! table maps to a repeating [`SchemaNode`] group of scalar fields whose
//! `name` is the table name, and one table row maps to one
//! [`Instance::Group`].

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbFormatError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("table `{0}` does not exist or has no columns")]
    NoSuchTable(String),
    #[error("table schema must be a group of scalar fields")]
    UnsupportedSchema,
    #[error("column `{column}`: cannot store a {got} as {expected:?}")]
    ValueType {
        column: String,
        expected: ScalarType,
        got: &'static str,
    },
}

fn columns_of(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, DbFormatError> {
    match &schema.kind {
        SchemaKind::Group { children } => children
            .iter()
            .map(|c| match &c.kind {
                SchemaKind::Scalar { ty } if !c.repeating => Ok((c.name.as_str(), *ty)),
                _ => Err(DbFormatError::UnsupportedSchema),
            })
            .collect(),
        SchemaKind::Scalar { .. } => Err(DbFormatError::UnsupportedSchema),
    }
}

/// Quotes an identifier for SQLite (`"` doubling).
fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Reads a table's declared columns as a repeating [`SchemaNode`] group
/// named after the table.
pub fn introspect(db_path: &Path, table: &str) -> Result<SchemaNode, DbFormatError> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
    let columns = stmt
        .query_map([], |row| {
            let name: String = row.get("name")?;
            let decl_type: String = row.get("type")?;
            Ok((name, decl_type))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(DbFormatError::NoSuchTable(table.to_string()));
    }
    let children = columns
        .into_iter()
        .map(|(name, decl_type)| SchemaNode::scalar(name, map_decl_type(&decl_type)))
        .collect();
    Ok(SchemaNode::group(table, children).repeating())
}

/// Maps a SQLite declared column type to a [`ScalarType`], following
/// SQLite's own affinity rules (substring matching on the declared type).
fn map_decl_type(decl_type: &str) -> ScalarType {
    let upper = decl_type.to_uppercase();
    if upper.contains("BOOL") {
        ScalarType::Bool
    } else if upper.contains("INT") {
        ScalarType::Int
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        ScalarType::Float
    } else {
        ScalarType::String
    }
}

fn column_sql_type(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::Int => "INTEGER",
        ScalarType::Float => "REAL",
        ScalarType::Bool => "BOOLEAN",
        ScalarType::String => "TEXT",
    }
}

/// Reads every row of the table named by `schema` (in rowid order) into one
/// [`Instance::Group`] per row.
pub fn read(db_path: &Path, schema: &SchemaNode) -> Result<Vec<Instance>, DbFormatError> {
    let columns = columns_of(schema)?;
    let conn = Connection::open(db_path)?;
    let column_list = columns
        .iter()
        .map(|(name, _)| quote(name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT {column_list} FROM {}",
        quote(&schema.name)
    ))?;
    let mut out = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut fields = Vec::with_capacity(columns.len());
        for (i, (name, ty)) in columns.iter().enumerate() {
            let value = read_value(row.get_ref(i)?, *ty);
            fields.push((name.to_string(), Instance::Scalar(value)));
        }
        out.push(Instance::Group(fields));
    }
    Ok(out)
}

/// Converts a SQLite value to an ir [`Value`], guided by the declared
/// scalar type (SQLite is dynamically typed, so stored values may need
/// widening -- e.g. an INTEGER cell in a REAL column).
fn read_value(value: ValueRef, ty: ScalarType) -> Value {
    match (ty, value) {
        (_, ValueRef::Null) => Value::Null,
        (ScalarType::Bool, ValueRef::Integer(i)) => Value::Bool(i != 0),
        (ScalarType::Int, ValueRef::Integer(i)) => Value::Int(i),
        (ScalarType::Float, ValueRef::Integer(i)) => Value::Float(i as f64),
        (ScalarType::Float, ValueRef::Real(f)) => Value::Float(f),
        (_, ValueRef::Integer(i)) => Value::String(i.to_string()),
        (_, ValueRef::Real(f)) => Value::String(f.to_string()),
        (_, ValueRef::Text(t)) => Value::String(String::from_utf8_lossy(t).into_owned()),
        (_, ValueRef::Blob(_)) => Value::Null,
    }
}

/// Replaces the contents of the table named by `schema` with `rows`,
/// creating the table if it doesn't exist. The full replace makes repeated
/// mapping runs idempotent.
pub fn write(db_path: &Path, schema: &SchemaNode, rows: &[Instance]) -> Result<(), DbFormatError> {
    let columns = columns_of(schema)?;
    let mut conn = Connection::open(db_path)?;

    let column_defs = columns
        .iter()
        .map(|(name, ty)| format!("{} {}", quote(name), column_sql_type(*ty)))
        .collect::<Vec<_>>()
        .join(", ");
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {} ({column_defs})",
            quote(&schema.name)
        ),
        [],
    )?;

    let tx = conn.transaction()?;
    tx.execute(&format!("DELETE FROM {}", quote(&schema.name)), [])?;
    let column_list = columns
        .iter()
        .map(|(name, _)| quote(name))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = vec!["?"; columns.len()].join(", ");
    {
        let mut stmt = tx.prepare(&format!(
            "INSERT INTO {} ({column_list}) VALUES ({placeholders})",
            quote(&schema.name)
        ))?;
        for row in rows {
            let params = columns
                .iter()
                .map(|(name, ty)| {
                    to_sql_value(name, *ty, row.field(name).and_then(Instance::as_scalar))
                })
                .collect::<Result<Vec<_>, _>>()?;
            stmt.execute(rusqlite::params_from_iter(params))?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn to_sql_value(
    column: &str,
    ty: ScalarType,
    value: Option<&Value>,
) -> Result<rusqlite::types::Value, DbFormatError> {
    use rusqlite::types::Value as Sql;
    match value {
        None | Some(Value::Null) => Ok(Sql::Null),
        Some(Value::Int(i)) => Ok(Sql::Integer(*i)),
        Some(Value::Float(f)) => Ok(Sql::Real(*f)),
        Some(Value::Bool(b)) => Ok(Sql::Integer(i64::from(*b))),
        Some(Value::String(s)) if ty == ScalarType::String => Ok(Sql::Text(s.clone())),
        Some(other) => Err(DbFormatError::ValueType {
            column: column.to_string(),
            expected: ty,
            got: other.type_name(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "people",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
                SchemaNode::scalar("score", ScalarType::Float),
                SchemaNode::scalar("member", ScalarType::Bool),
            ],
        )
        .repeating()
    }

    fn person(name: &str, age: i64, score: f64, member: bool) -> Instance {
        Instance::Group(vec![
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
            ("age".into(), Instance::Scalar(Value::Int(age))),
            ("score".into(), Instance::Scalar(Value::Float(score))),
            ("member".into(), Instance::Scalar(Value::Bool(member))),
        ])
    }

    #[test]
    fn write_then_read_roundtrips_and_is_idempotent() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("ferrule_format_db_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let rows = vec![person("Jane", 29, 1.5, true), person("Bob", 17, 0.5, false)];
        write(&path, &schema(), &rows).unwrap();
        // Second write must fully replace, not append.
        write(&path, &schema(), &rows).unwrap();
        let read_back = read(&path, &schema()).unwrap();

        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_back, rows);
    }

    #[test]
    fn introspect_recovers_the_written_schema() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_db_test_introspect_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        write(&path, &schema(), &[]).unwrap();
        let introspected = introspect(&path, "people").unwrap();
        let missing = introspect(&path, "nope").unwrap_err();

        std::fs::remove_file(&path).unwrap();
        assert_eq!(introspected, schema());
        assert!(matches!(missing, DbFormatError::NoSuchTable(t) if t == "nope"));
    }
}
