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
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

mod relational;

#[cfg(test)]
mod relational_tests;

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
    #[error("row {row}: expected a group, got {got}")]
    RowShape { row: usize, got: &'static str },
    #[error("row {row}: missing column `{column}`")]
    MissingField { row: usize, column: String },
    #[error("row {row}: unexpected column `{column}`")]
    UnexpectedField { row: usize, column: String },
    #[error("row {row}: duplicate column `{column}`")]
    DuplicateField { row: usize, column: String },
    #[error("column `{column}`: cannot read SQLite {got} as {expected:?}")]
    CellType {
        column: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error(
        "existing column `{column}` has {affinity} affinity (declared as `{declared}`), which \
         cannot preserve {expected:?} values"
    )]
    ColumnAffinity {
        column: String,
        expected: ScalarType,
        declared: String,
        affinity: &'static str,
    },
    #[error("existing table has no column named `{0}`")]
    MissingColumn(String),
    #[error("relational schema node `{node}` is invalid: {reason}")]
    InvalidRelationalSchema { node: String, reason: &'static str },
    #[error(
        "no foreign-key relation connects `{parent_table}` to `{child_table}` through join column `{join_column}`"
    )]
    MissingForeignKeyRelation {
        parent_table: String,
        child_table: String,
        join_column: String,
    },
    #[error(
        "multiple foreign-key relations connect `{parent_table}` to `{child_table}` through join column `{join_column}`"
    )]
    AmbiguousForeignKeyRelation {
        parent_table: String,
        child_table: String,
        join_column: String,
    },
    #[error(
        "no foreign key connects `{parent_table}`.`{parent_column}` to `{child_table}`.`{child_column}`"
    )]
    MissingForeignKeyEndpoints {
        parent_table: String,
        parent_column: String,
        child_table: String,
        child_column: String,
    },
    #[error(
        "multiple foreign keys connect `{parent_table}`.`{parent_column}` to `{child_table}`.`{child_column}`"
    )]
    AmbiguousForeignKeyEndpoints {
        parent_table: String,
        parent_column: String,
        child_table: String,
        child_column: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKeySide {
    Parent,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyRelation {
    pub side: ForeignKeySide,
    pub join_column: String,
}

/// The exact scalar columns equated by one relational table edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyColumns {
    pub parent_column: String,
    pub child_column: String,
}

fn columns_of(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, DbFormatError> {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
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
    let canonical: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = ?1 COLLATE NOCASE",
            [table],
            |row| row.get(0),
        )
        .optional()?;
    let canonical = canonical.ok_or_else(|| DbFormatError::NoSuchTable(table.to_string()))?;
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(&canonical)))?;
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
    Ok(SchemaNode::group(canonical, children).repeating())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqliteAffinity {
    Integer,
    Text,
    Blob,
    Real,
    Numeric,
}

impl SqliteAffinity {
    fn name(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
            Self::Real => "REAL",
            Self::Numeric => "NUMERIC",
        }
    }
}

/// Applies SQLite's declared-type affinity rules in their documented order.
fn sqlite_affinity(decl_type: &str) -> SqliteAffinity {
    let upper = decl_type.to_ascii_uppercase();
    if upper.contains("INT") {
        SqliteAffinity::Integer
    } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
        SqliteAffinity::Text
    } else if upper.is_empty() || upper.contains("BLOB") {
        SqliteAffinity::Blob
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        SqliteAffinity::Real
    } else {
        SqliteAffinity::Numeric
    }
}

/// Maps a SQLite declared column type to the closest [`ScalarType`].
fn map_decl_type(decl_type: &str) -> ScalarType {
    let upper = decl_type.trim().to_ascii_uppercase();
    if is_temporal_decl_type(&upper) {
        // SQLite has no temporal storage class. In practice these declared
        // types commonly contain ISO lexical values, which the string IR can
        // preserve without guessing a timezone or numeric epoch convention.
        ScalarType::String
    } else if upper.contains("BOOL") {
        ScalarType::Bool
    } else {
        match sqlite_affinity(decl_type) {
            SqliteAffinity::Integer => ScalarType::Int,
            SqliteAffinity::Real | SqliteAffinity::Numeric => ScalarType::Float,
            SqliteAffinity::Text | SqliteAffinity::Blob => ScalarType::String,
        }
    }
}

fn is_temporal_decl_type(decl_type: &str) -> bool {
    let base = decl_type
        .split(|character: char| character == '(' || character.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    matches!(base, "DATE" | "DATETIME" | "TIME" | "TIMESTAMP")
}

fn column_sql_type(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::Int => "INTEGER",
        ScalarType::Float => "REAL",
        ScalarType::Bool => "BOOLEAN",
        ScalarType::String => "TEXT",
    }
}

/// Reads every row of the table named by `schema` into one [`Instance::Group`]
/// per row. Rowid tables are read in rowid order; tables without a rowid use
/// SQLite's unspecified natural order.
pub fn read(db_path: &Path, schema: &SchemaNode) -> Result<Vec<Instance>, DbFormatError> {
    let columns = columns_of(schema)?;
    let conn = Connection::open(db_path)?;
    let column_list = columns
        .iter()
        .map(|(name, _)| quote(name))
        .collect::<Vec<_>>()
        .join(", ");
    let order = if conn
        .prepare(&format!(
            "SELECT rowid FROM {} LIMIT 0",
            quote(&schema.name)
        ))
        .is_ok()
    {
        " ORDER BY rowid"
    } else {
        ""
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {column_list} FROM {}{order}",
        quote(&schema.name),
    ))?;
    let mut out = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut fields = Vec::with_capacity(columns.len());
        for (i, (name, ty)) in columns.iter().enumerate() {
            let value = read_value(name, row.get_ref(i)?, *ty)?;
            fields.push((name.to_string(), Instance::Scalar(value)));
        }
        out.push(Instance::Group(fields));
    }
    Ok(out)
}

/// Reads either a conventional single-table schema or a relational database
/// schema into its complete instance shape.
///
/// A single table is a repeating group, as accepted by [`read`]. A composite
/// database root is a non-repeating group whose children are repeating table
/// groups. Tables may contain repeating relationship groups named
/// `PhysicalTable|JoinColumn`; the relationship direction and referenced key
/// are resolved from SQLite's foreign-key metadata.
pub fn read_instance(db_path: &Path, schema: &SchemaNode) -> Result<Instance, DbFormatError> {
    relational::read_instance(db_path, schema)
}

/// Replaces the rows described by either a flat table or relational schema.
/// Relationship insertion order follows the side that owns each foreign key.
pub fn write_instance(
    db_path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), DbFormatError> {
    if relational::is_flat_table(schema) {
        let rows = instance.as_repeated().ok_or(DbFormatError::RowShape {
            row: 0,
            got: instance_type_name(instance),
        })?;
        return write(db_path, schema, rows);
    }
    relational::write_instance(db_path, schema, instance)
}

/// Validates a relational database schema against SQLite's foreign-key
/// metadata without reading any table rows.
pub fn validate_relational_schema(
    db_path: &Path,
    schema: &SchemaNode,
) -> Result<(), DbFormatError> {
    relational::validate_schema(db_path, schema)
}

/// Resolves the columns joined by one `ChildTable|JoinColumn` relationship.
///
/// `join_column` is the column encoded in the relationship name. It may be
/// owned by either table; SQLite metadata determines the direction. Missing
/// or ambiguous relationships are rejected rather than guessed.
pub fn resolve_foreign_key_columns(
    db_path: &Path,
    parent_table: &str,
    child_table: &str,
    join_column: &str,
) -> Result<ForeignKeyColumns, DbFormatError> {
    relational::resolve_foreign_key_columns(db_path, parent_table, child_table, join_column)
}

/// Resolves one exact relationship endpoint pair against SQLite metadata.
/// The returned join column is the column on the table that owns the FK.
pub fn resolve_foreign_key_relation(
    db_path: &Path,
    parent_table: &str,
    parent_column: &str,
    child_table: &str,
    child_column: &str,
) -> Result<ForeignKeyRelation, DbFormatError> {
    relational::resolve_foreign_key_relation(
        db_path,
        parent_table,
        parent_column,
        child_table,
        child_column,
    )
}

/// Converts a SQLite value to an ir [`Value`], guided by the declared
/// scalar type (SQLite is dynamically typed, so stored values may need
/// widening -- e.g. an INTEGER cell in a REAL column).
fn read_value(column: &str, value: ValueRef, ty: ScalarType) -> Result<Value, DbFormatError> {
    let incompatible = |got| DbFormatError::CellType {
        column: column.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, ValueRef::Null) => Ok(Value::Null),
        (ScalarType::Bool, ValueRef::Integer(0)) => Ok(Value::Bool(false)),
        (ScalarType::Bool, ValueRef::Integer(1)) => Ok(Value::Bool(true)),
        (ScalarType::Int, ValueRef::Integer(i)) => Ok(Value::Int(i)),
        (ScalarType::Float, ValueRef::Integer(i)) => exact_f64(i)
            .map(Value::Float)
            .ok_or_else(|| incompatible("integer outside the exact f64 range")),
        (ScalarType::Float, ValueRef::Real(f)) if f.is_finite() => Ok(Value::Float(f)),
        (ScalarType::String, ValueRef::Text(text)) => std::str::from_utf8(text)
            .map(|text| Value::String(text.to_string()))
            .map_err(|_| incompatible("non-UTF-8 text")),
        (_, other) => Err(incompatible(sqlite_type_name(other))),
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

fn sqlite_type_name(value: ValueRef) -> &'static str {
    match value {
        ValueRef::Null => "null",
        ValueRef::Integer(_) => "integer",
        ValueRef::Real(value) if value.is_finite() => "real",
        ValueRef::Real(_) => "non-finite real",
        ValueRef::Text(_) => "text",
        ValueRef::Blob(_) => "blob",
    }
}

/// Replaces the contents of the table named by `schema` with `rows`,
/// creating the table if it doesn't exist. The full replace makes repeated
/// mapping runs idempotent.
pub fn write(db_path: &Path, schema: &SchemaNode, rows: &[Instance]) -> Result<(), DbFormatError> {
    let columns = columns_of(schema)?;
    let records = rows
        .iter()
        .enumerate()
        .map(|(row, instance)| row_values(row, instance, &columns))
        .collect::<Result<Vec<_>, _>>()?;
    let mut conn = Connection::open(db_path)?;

    let column_defs = columns
        .iter()
        .map(|(name, ty)| format!("{} {}", quote(name), column_sql_type(*ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let tx = conn.transaction()?;
    let existing_columns = declared_columns(&tx, &schema.name)?;
    if existing_columns.is_empty() {
        tx.execute(
            &format!("CREATE TABLE {} ({column_defs})", quote(&schema.name)),
            [],
        )?;
    } else {
        validate_column_affinities(&existing_columns, &columns)?;
    }

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
        for params in records {
            stmt.execute(rusqlite::params_from_iter(params))?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn row_values(
    row: usize,
    instance: &Instance,
    columns: &[(&str, ScalarType)],
) -> Result<Vec<rusqlite::types::Value>, DbFormatError> {
    let Instance::Group(fields) = instance else {
        return Err(DbFormatError::RowShape {
            row,
            got: instance_type_name(instance),
        });
    };
    for (index, (name, _)) in fields.iter().enumerate() {
        if !columns.iter().any(|(column, _)| column == name) {
            return Err(DbFormatError::UnexpectedField {
                row,
                column: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(DbFormatError::DuplicateField {
                row,
                column: name.clone(),
            });
        }
    }

    columns
        .iter()
        .map(|(name, ty)| {
            let (_, value) = fields
                .iter()
                .find(|(field, _)| field == name)
                .ok_or_else(|| DbFormatError::MissingField {
                    row,
                    column: (*name).to_string(),
                })?;
            let Instance::Scalar(value) = value else {
                return Err(DbFormatError::ValueType {
                    column: (*name).to_string(),
                    expected: *ty,
                    got: instance_type_name(value),
                });
            };
            to_sql_value(name, *ty, value)
        })
        .collect()
}

fn instance_type_name(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated",
        Instance::MappedSequence(_) => "mapped sequence",
    }
}

fn declared_columns(
    conn: &Connection,
    table: &str,
) -> Result<Vec<(String, String)>, DbFormatError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
    Ok(stmt
        .query_map([], |row| Ok((row.get("name")?, row.get("type")?)))?
        .collect::<Result<_, _>>()?)
}

fn validate_column_affinities(
    declared: &[(String, String)],
    columns: &[(&str, ScalarType)],
) -> Result<(), DbFormatError> {
    for (name, ty) in columns {
        let (_, decl_type) = declared
            .iter()
            .find(|(declared_name, _)| declared_name.eq_ignore_ascii_case(name))
            .ok_or_else(|| DbFormatError::MissingColumn((*name).to_string()))?;
        let affinity = sqlite_affinity(decl_type);
        let temporal_string = *ty == ScalarType::String
            && is_temporal_decl_type(&decl_type.trim().to_ascii_uppercase());
        if !temporal_string && !affinity_preserves(*ty, affinity) {
            return Err(DbFormatError::ColumnAffinity {
                column: (*name).to_string(),
                expected: *ty,
                declared: decl_type.clone(),
                affinity: affinity.name(),
            });
        }
    }
    Ok(())
}

/// Whether binding the scalar's native SQLite storage class can survive the
/// column affinity and still be accepted by `read_value`.
fn affinity_preserves(ty: ScalarType, affinity: SqliteAffinity) -> bool {
    match ty {
        ScalarType::Int | ScalarType::Bool => matches!(
            affinity,
            SqliteAffinity::Integer | SqliteAffinity::Numeric | SqliteAffinity::Blob
        ),
        ScalarType::Float => affinity != SqliteAffinity::Text,
        ScalarType::String => matches!(affinity, SqliteAffinity::Text | SqliteAffinity::Blob),
    }
}

fn to_sql_value(
    column: &str,
    ty: ScalarType,
    value: &Value,
) -> Result<rusqlite::types::Value, DbFormatError> {
    use rusqlite::types::Value as Sql;
    match (ty, value) {
        (_, Value::Null) => Ok(Sql::Null),
        (ScalarType::Int, Value::Int(i)) => Ok(Sql::Integer(*i)),
        (ScalarType::Float, Value::Float(f)) if f.is_finite() => Ok(Sql::Real(*f)),
        (ScalarType::Float, Value::Float(_)) => Err(DbFormatError::ValueType {
            column: column.to_string(),
            expected: ty,
            got: "non-finite float",
        }),
        (ScalarType::Float, Value::Int(i)) => {
            exact_f64(*i)
                .map(Sql::Real)
                .ok_or_else(|| DbFormatError::ValueType {
                    column: column.to_string(),
                    expected: ty,
                    got: "int outside the exact f64 range",
                })
        }
        (ScalarType::Bool, Value::Bool(b)) => Ok(Sql::Integer(i64::from(*b))),
        (ScalarType::String, Value::String(s)) => Ok(Sql::Text(s.clone())),
        (_, other) => Err(DbFormatError::ValueType {
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

    #[test]
    fn introspect_preserves_temporal_lexicals_as_strings() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_temporal_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE events (created_at TIMESTAMP, event_date DATE); \
             INSERT INTO events VALUES ('2026-07-16 12:34:56', '2026-07-16');",
        )
        .unwrap();
        drop(conn);

        let schema = introspect(&path, "events").unwrap();
        let rows = read(&path, &schema).unwrap();
        write(&path, &schema, &rows).unwrap();
        let roundtrip = read(&path, &schema).unwrap();

        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            schema,
            SchemaNode::group(
                "events",
                vec![
                    SchemaNode::scalar("created_at", ScalarType::String),
                    SchemaNode::scalar("event_date", ScalarType::String),
                ],
            )
            .repeating()
        );
        assert_eq!(
            rows,
            vec![Instance::Group(vec![
                (
                    "created_at".into(),
                    Instance::Scalar(Value::String("2026-07-16 12:34:56".into())),
                ),
                (
                    "event_date".into(),
                    Instance::Scalar(Value::String("2026-07-16".into())),
                ),
            ])]
        );
        assert_eq!(roundtrip, rows);
    }

    #[test]
    fn read_rejects_dynamic_cells_that_violate_the_declared_schema() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_dynamic_types_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "typed",
            vec![
                SchemaNode::scalar("age", ScalarType::Int),
                SchemaNode::scalar("payload", ScalarType::String),
                SchemaNode::scalar("member", ScalarType::Bool),
            ],
        )
        .repeating();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE typed (age INTEGER, payload TEXT, member BOOLEAN); \
             INSERT INTO typed VALUES (1.5, 'ok', 1);",
        )
        .unwrap();
        drop(conn);

        assert!(matches!(
            read(&path, &schema),
            Err(DbFormatError::CellType {
                column,
                expected: ScalarType::Int,
                got: "real"
            }) if column == "age"
        ));

        let conn = Connection::open(&path).unwrap();
        conn.execute("UPDATE typed SET age = 1, payload = x'00FF'", [])
            .unwrap();
        drop(conn);
        assert!(matches!(
            read(&path, &schema),
            Err(DbFormatError::CellType {
                column,
                expected: ScalarType::String,
                got: "blob"
            }) if column == "payload"
        ));

        let conn = Connection::open(&path).unwrap();
        conn.execute("UPDATE typed SET payload = 'ok', member = 2", [])
            .unwrap();
        drop(conn);
        let error = read(&path, &schema).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            error,
            DbFormatError::CellType {
                column,
                expected: ScalarType::Bool,
                got: "integer"
            } if column == "member"
        ));
    }

    #[test]
    fn write_enforces_declared_types_and_exact_integer_widening() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_write_types_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "metrics",
            vec![
                SchemaNode::scalar("score", ScalarType::Float),
                SchemaNode::scalar("member", ScalarType::Bool),
            ],
        )
        .repeating();
        let row = |score, member| {
            Instance::Group(vec![
                ("score".into(), Instance::Scalar(score)),
                ("member".into(), Instance::Scalar(member)),
            ])
        };

        write(&path, &schema, &[row(Value::Int(42), Value::Bool(true))]).unwrap();
        let rows = read(&path, &schema).unwrap();
        assert_eq!(
            rows[0].field("score").and_then(Instance::as_scalar),
            Some(&Value::Float(42.0))
        );
        assert_eq!(exact_f64(i64::MIN), Some(i64::MIN as f64));

        let mismatch = write(&path, &schema, &[row(Value::Float(1.0), Value::Int(1))]).unwrap_err();
        assert!(matches!(
            mismatch,
            DbFormatError::ValueType {
                column,
                expected: ScalarType::Bool,
                got: "int"
            } if column == "member"
        ));

        let precision_loss = write(
            &path,
            &schema,
            &[row(Value::Int((1_i64 << 53) + 1), Value::Bool(false))],
        )
        .unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            precision_loss,
            DbFormatError::ValueType {
                column,
                expected: ScalarType::Float,
                got: "int outside the exact f64 range"
            } if column == "score"
        ));
    }

    #[test]
    fn read_orders_rows_by_rowid() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_row_order_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "ordered",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        )
        .repeating();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE ordered (name TEXT); \
             INSERT INTO ordered(rowid, name) VALUES (10, 'ten'), (2, 'two'), (7, 'seven');",
        )
        .unwrap();
        drop(conn);

        let names: Vec<_> = read(&path, &schema)
            .unwrap()
            .into_iter()
            .map(|row| row.field("name").unwrap().as_scalar().unwrap().clone())
            .collect();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            names,
            vec![
                Value::String("two".into()),
                Value::String("seven".into()),
                Value::String("ten".into())
            ]
        );
    }

    #[test]
    fn read_supports_tables_without_rowid() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_without_rowid_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "keyed",
            vec![
                SchemaNode::scalar("group_id", ScalarType::Int),
                SchemaNode::scalar("item_id", ScalarType::Int),
                SchemaNode::scalar("name", ScalarType::String),
            ],
        )
        .repeating();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE keyed (\
                 group_id INTEGER NOT NULL, \
                 item_id INTEGER NOT NULL, \
                 name TEXT, \
                 PRIMARY KEY (group_id, item_id)\
             ) WITHOUT ROWID; \
             INSERT INTO keyed VALUES (2, 1, 'second'), (1, 1, 'first');",
        )
        .unwrap();
        drop(conn);

        let mut names: Vec<_> = read(&path, &schema)
            .unwrap()
            .into_iter()
            .map(|row| row.field("name").unwrap().as_scalar().unwrap().clone())
            .collect();
        std::fs::remove_file(&path).unwrap();
        names.sort_by_key(|value| match value {
            Value::String(text) => text.clone(),
            other => panic!("expected a string, got {other:?}"),
        });
        assert_eq!(
            names,
            vec![
                Value::String("first".into()),
                Value::String("second".into())
            ]
        );
    }

    #[test]
    fn write_rejects_incompatible_existing_affinity_before_replacing_rows() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_existing_affinity_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "metrics",
            vec![SchemaNode::scalar("score", ScalarType::Float)],
        )
        .repeating();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metrics (score TEXT); INSERT INTO metrics VALUES ('old');",
        )
        .unwrap();
        drop(conn);

        let rows = [Instance::Group(vec![(
            "score".into(),
            Instance::Scalar(Value::Float(1.5)),
        )])];
        let error = write(&path, &schema, &rows).unwrap_err();
        assert!(matches!(
            error,
            DbFormatError::ColumnAffinity {
                column,
                expected: ScalarType::Float,
                declared,
                affinity: "TEXT",
            } if column == "score" && declared == "TEXT"
        ));

        let conn = Connection::open(&path).unwrap();
        let preserved: String = conn
            .query_row("SELECT score FROM metrics", [], |row| row.get(0))
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(preserved, "old");
    }

    #[test]
    fn failed_first_write_does_not_leave_a_table() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_atomic_create_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "metrics",
            vec![SchemaNode::scalar("score", ScalarType::Int)],
        )
        .repeating();
        let rows = [Instance::Group(vec![(
            "score".into(),
            Instance::Scalar(Value::String("not an integer".into())),
        )])];

        assert!(matches!(
            write(&path, &schema, &rows),
            Err(DbFormatError::ValueType {
                column,
                expected: ScalarType::Int,
                got: "string",
            }) if column == "score"
        ));
        let conn = Connection::open(&path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'metrics'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(table_count, 0);
    }

    #[test]
    fn malformed_rows_are_rejected_before_opening_the_database() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_db_test_row_shape_{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let schema = SchemaNode::group(
            "metrics",
            vec![SchemaNode::scalar("score", ScalarType::Int)],
        )
        .repeating();

        assert!(matches!(
            write(&path, &schema, &[Instance::Scalar(Value::Int(1))]),
            Err(DbFormatError::RowShape { row: 0, got: "int" })
        ));
        assert!(matches!(
            write(&path, &schema, &[Instance::MappedSequence(Vec::new())]),
            Err(DbFormatError::RowShape {
                row: 0,
                got: "mapped sequence"
            })
        ));
        assert!(matches!(
            write(&path, &schema, &[Instance::Group(Vec::new())]),
            Err(DbFormatError::MissingField { row: 0, column }) if column == "score"
        ));
        assert!(matches!(
            write(
                &path,
                &schema,
                &[Instance::Group(vec![(
                    "score".into(),
                    Instance::MappedSequence(Vec::new()),
                )])],
            ),
            Err(DbFormatError::ValueType {
                column,
                expected: ScalarType::Int,
                got: "mapped sequence",
            }) if column == "score"
        ));
        assert!(matches!(
            write(
                &path,
                &schema,
                &[Instance::Group(vec![
                    ("score".into(), Instance::Scalar(Value::Int(1))),
                    ("extra".into(), Instance::Scalar(Value::Int(2))),
                ])],
            ),
            Err(DbFormatError::UnexpectedField { row: 0, column }) if column == "extra"
        ));
        assert!(!path.exists());
    }
}
