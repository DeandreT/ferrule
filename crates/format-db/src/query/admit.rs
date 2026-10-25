//! Metadata and key admission inside the acquisition's read snapshot.
//!
//! Only fixed host statements run here, before the authorizer is installed.
//! The first statement reads `sqlite_schema` and so establishes the snapshot
//! that candidate validation and selection reuse.

use std::path::Path;

use ir::ScalarType;
use mapping::ValidatedSqliteRead;
use rusqlite::{Connection, OptionalExtension};

use super::{ForbiddenTable, KeyProblem, QueryPhase, QueryReadError, QueryReadErrorKind};
use crate::{SqliteAffinity, column_sql_type, quote, sqlite_affinity};

/// Canonical metadata spellings for an admitted plan.
#[derive(Debug)]
pub(super) struct Admitted {
    pub table: String,
    pub key: String,
    /// Canonical names for `plan.columns()`, in the same order.
    pub columns: Vec<String>,
}

impl Admitted {
    /// The canonical spelling of one plan column (case-insensitive match).
    pub fn canonical<'a>(&'a self, plan: &ValidatedSqliteRead, column: &'a str) -> &'a str {
        plan.columns()
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(column))
            .map_or(column, |index| self.columns[index].as_str())
    }
}

struct ColumnInfo {
    name: String,
    declared: String,
    primary_key: i64,
    hidden: i64,
}

/// `sqlite` classifies statement failures, including progress interrupts.
pub(super) fn admit(
    conn: &Connection,
    source: &Path,
    plan: &ValidatedSqliteRead,
    sqlite: &dyn Fn(rusqlite::Error) -> QueryReadError,
) -> Result<Admitted, QueryReadError> {
    let error = |kind| QueryReadError::new(source, QueryPhase::Metadata, kind);

    // Table: exactly one ordinary rowid table in `main`.
    let mut statement = conn
        .prepare(
            "SELECT type, name, sql FROM main.sqlite_schema \
             WHERE type IN ('table', 'view') AND name = ?1 COLLATE NOCASE",
        )
        .map_err(sqlite)?;
    let matches = statement
        .query_map([plan.table()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite)?;
    drop(statement);
    let table_error = |kind| error(kind).column(plan.table());
    let [(kind, table, sql)] = matches.as_slice() else {
        return Err(table_error(QueryReadErrorKind::NoSuchTable));
    };
    if kind == "view" {
        return Err(table_error(QueryReadErrorKind::ForbiddenTable(
            ForbiddenTable::View,
        )));
    }
    if table
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sqlite_"))
    {
        return Err(table_error(QueryReadErrorKind::ForbiddenTable(
            ForbiddenTable::Internal,
        )));
    }
    if sql.as_deref().is_some_and(|sql| {
        sql.trim_start()
            .get(..20)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("CREATE VIRTUAL TABLE"))
    }) {
        return Err(table_error(QueryReadErrorKind::ForbiddenTable(
            ForbiddenTable::Virtual,
        )));
    }
    let listed: Option<(String, bool)> = conn
        .query_row(
            &format!("PRAGMA main.table_list({})", quote(table)),
            [],
            |row| Ok((row.get::<_, String>("type")?, row.get::<_, bool>("wr")?)),
        )
        .optional()
        .map_err(sqlite)?;
    match listed
        .as_ref()
        .map(|(kind, without_rowid)| (kind.as_str(), *without_rowid))
    {
        Some(("table", false)) => {}
        Some(("table", true)) => {
            return Err(table_error(QueryReadErrorKind::ForbiddenTable(
                ForbiddenTable::WithoutRowid,
            )));
        }
        Some(("virtual", _)) => {
            return Err(table_error(QueryReadErrorKind::ForbiddenTable(
                ForbiddenTable::Virtual,
            )));
        }
        Some(("shadow", _)) => {
            return Err(table_error(QueryReadErrorKind::ForbiddenTable(
                ForbiddenTable::Shadow,
            )));
        }
        _ => return Err(table_error(QueryReadErrorKind::NoSuchTable)),
    }

    let mut statement = conn
        .prepare(&format!("PRAGMA main.table_xinfo({})", quote(table)))
        .map_err(sqlite)?;
    let columns = statement
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get("name")?,
                declared: row.get("type")?,
                primary_key: row.get("pk")?,
                hidden: row.get("hidden")?,
            })
        })
        .map_err(sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite)?;
    drop(statement);

    // Key: the single INTEGER PRIMARY KEY rowid alias, never a guess.
    let key = find_column(&columns, plan.key()).map_err(|kind| error(kind).column(plan.key()))?;
    let key_error = |problem| error(QueryReadErrorKind::InvalidKey(problem)).column(plan.key());
    if key.hidden != 0 {
        return Err(error(QueryReadErrorKind::GeneratedColumn).column(plan.key()));
    }
    if key.primary_key == 0 {
        return Err(key_error(KeyProblem::NotPrimaryKey));
    }
    if columns
        .iter()
        .filter(|column| column.primary_key > 0)
        .count()
        != 1
    {
        return Err(key_error(KeyProblem::CompositePrimaryKey));
    }
    if !key.declared.trim().eq_ignore_ascii_case("INTEGER") {
        return Err(key_error(KeyProblem::NotRowidAlias));
    }
    // `INTEGER PRIMARY KEY DESC` is not an alias; SQLite backs it with an index.
    let mut statement = conn
        .prepare(&format!("PRAGMA main.index_list({})", quote(table)))
        .map_err(sqlite)?;
    let origins = statement
        .query_map([], |row| row.get::<_, String>("origin"))
        .map_err(sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite)?;
    drop(statement);
    if origins.iter().any(|origin| origin == "pk") {
        return Err(key_error(KeyProblem::NotRowidAlias));
    }

    // Columns in first-use order: projection, predicates, ordering.
    let mut canonical = Vec::with_capacity(plan.columns().len());
    for (name, ty) in plan.columns() {
        let column = find_column(&columns, name).map_err(|kind| error(kind).column(name))?;
        if column.hidden != 0 {
            return Err(error(QueryReadErrorKind::GeneratedColumn).column(name));
        }
        let affinity = sqlite_affinity(&column.declared);
        if !affinity_admits(*ty, affinity) && !writer_boolean(*ty, &column.declared) {
            return Err(error(QueryReadErrorKind::ColumnAffinity {
                expected: *ty,
                declared: column.declared.clone(),
                affinity: affinity.name(),
            })
            .column(name));
        }
        canonical.push(column.name.clone());
    }
    Ok(Admitted {
        table: table.clone(),
        key: key.name.clone(),
        columns: canonical,
    })
}

fn find_column<'c>(
    columns: &'c [ColumnInfo],
    name: &str,
) -> Result<&'c ColumnInfo, QueryReadErrorKind> {
    let mut matches = columns
        .iter()
        .filter(|column| column.name.eq_ignore_ascii_case(name));
    let column = matches.next().ok_or(QueryReadErrorKind::MissingColumn)?;
    if matches.next().is_some() {
        return Err(QueryReadErrorKind::AmbiguousColumn);
    }
    Ok(column)
}

/// The single declared affinity each scalar type admits; empty, NUMERIC and
/// BLOB affinities refuse.
fn affinity_admits(ty: ScalarType, affinity: SqliteAffinity) -> bool {
    match ty {
        ScalarType::String => affinity == SqliteAffinity::Text,
        ScalarType::Int | ScalarType::Bool => affinity == SqliteAffinity::Integer,
        ScalarType::Float => matches!(affinity, SqliteAffinity::Real | SqliteAffinity::Integer),
    }
}

/// Bool also admits the exact `BOOLEAN` declaration Ferrule's writer creates.
/// Its NUMERIC affinity is otherwise refused; candidate validation still
/// requires INTEGER 0/1 storage in every cell.
fn writer_boolean(ty: ScalarType, declared: &str) -> bool {
    ty == ScalarType::Bool && declared.trim().eq_ignore_ascii_case(column_sql_type(ty))
}
