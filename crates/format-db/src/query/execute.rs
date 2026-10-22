//! Snapshot acquisition, bounded execution and connection cleanup.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ir::{Instance, ScalarType, Value};
use mapping::{SqliteReadDirection, SqliteReadNulls, ValidatedPredicateTest, ValidatedSqliteRead};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Statement};

use super::admit::{Admitted, admit};
use super::{
    BoundSqliteRead, CleanupStep, QueryCleanupFailure, QueryLimit, QueryPhase, QueryReadControl,
    QueryReadError, QueryReadErrorKind, QueryReadLimits, sqlite_codes,
};
use crate::{exact_f64, quote, sqlite_type_name};

const STOP_NONE: u8 = 0;
const STOP_CANCELLED: u8 = 1;
const STOP_DEADLINE: u8 = 2;
const STOP_WORK: u8 = 3;

/// State shared with the `'static` progress callback and the watcher thread.
struct Progress {
    cancelled: AtomicBool,
    deadline: Instant,
    work: AtomicU64,
    work_limit: u64,
    stop: AtomicU8,
}

impl Progress {
    /// Cancellation wins over the deadline, which wins over VM work.
    fn check(&self) -> u8 {
        let stop = if self.cancelled.load(Ordering::SeqCst) {
            STOP_CANCELLED
        } else if Instant::now() >= self.deadline {
            STOP_DEADLINE
        } else if self
            .work
            .fetch_add(super::QUERY_PROGRESS_INTERVAL, Ordering::SeqCst)
            .saturating_add(super::QUERY_PROGRESS_INTERVAL)
            > self.work_limit
        {
            STOP_WORK
        } else {
            STOP_NONE
        };
        if stop != STOP_NONE {
            let _ = self
                .stop
                .compare_exchange(STOP_NONE, stop, Ordering::SeqCst, Ordering::SeqCst);
        }
        stop
    }
}

/// Phase-scoped error construction plus the cooperative boundary checks.
struct Run<'a> {
    source: &'a Path,
    host: Option<&'a AtomicBool>,
    progress: Arc<Progress>,
    limits: QueryReadLimits,
    payload: u64,
}

impl Run<'_> {
    fn error(&self, phase: QueryPhase, kind: QueryReadErrorKind) -> QueryReadError {
        QueryReadError::new(self.source, phase, kind)
    }

    /// Checked at every row and cell boundary on the acquiring thread.
    fn boundary(&self, phase: QueryPhase) -> Result<(), QueryReadError> {
        if self.host.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            self.progress.cancelled.store(true, Ordering::SeqCst);
        }
        if self.progress.cancelled.load(Ordering::SeqCst) {
            return Err(self.error(phase, QueryReadErrorKind::Cancelled));
        }
        if Instant::now() >= self.progress.deadline {
            return Err(self.error(phase, QueryReadErrorKind::DeadlineExceeded));
        }
        if self.progress.stop.load(Ordering::SeqCst) == STOP_WORK {
            return Err(self.error(phase, QueryReadErrorKind::LimitExceeded(QueryLimit::VmWork)));
        }
        Ok(())
    }

    /// Maps an interrupted statement to the reason recorded by the callback.
    fn sqlite(&self, phase: QueryPhase, cause: rusqlite::Error) -> QueryReadError {
        let interrupted = sqlite_codes(&cause)
            .is_some_and(|codes| codes.primary == rusqlite::ffi::SQLITE_INTERRUPT);
        if !interrupted {
            return QueryReadError::sqlite(self.source, phase, cause);
        }
        let host_cancelled = self.host.is_some_and(|flag| flag.load(Ordering::SeqCst));
        let kind = match self.progress.stop.load(Ordering::SeqCst) {
            _ if host_cancelled || self.progress.cancelled.load(Ordering::SeqCst) => {
                QueryReadErrorKind::Cancelled
            }
            STOP_DEADLINE => QueryReadErrorKind::DeadlineExceeded,
            STOP_WORK => QueryReadErrorKind::LimitExceeded(QueryLimit::VmWork),
            _ => QueryReadErrorKind::Cancelled,
        };
        self.error(phase, kind).caused_by(cause)
    }

    /// Charges one decoded cell to the per-cell and cumulative byte ledgers.
    fn charge(
        &mut self,
        phase: QueryPhase,
        column: &str,
        row: u64,
        cell: ValueRef<'_>,
    ) -> Result<(), QueryReadError> {
        let bytes = match cell {
            ValueRef::Null => 0,
            ValueRef::Integer(_) | ValueRef::Real(_) => 8,
            ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes.len() as u64,
        };
        let limit = |limit| {
            self.error(phase, QueryReadErrorKind::LimitExceeded(limit))
                .column(column)
                .row(row)
        };
        if bytes > self.limits.cell_bytes {
            return Err(limit(QueryLimit::CellBytes));
        }
        match self.payload.checked_add(bytes) {
            Some(total) if total <= self.limits.payload_bytes => {
                self.payload = total;
                Ok(())
            }
            _ => Err(limit(QueryLimit::PayloadBytes)),
        }
    }
}

pub(super) fn read(
    bound: &BoundSqliteRead<'_>,
    control: &QueryReadControl<'_>,
) -> Result<Instance, QueryReadError> {
    let started = Instant::now();
    let limits = control.limits;
    let progress = Arc::new(Progress {
        cancelled: AtomicBool::new(false),
        deadline: started.checked_add(limits.deadline).unwrap_or(started),
        work: AtomicU64::new(0),
        work_limit: limits.vm_work,
        stop: AtomicU8::new(STOP_NONE),
    });
    let mut run = Run {
        source: &bound.source,
        host: control.cancellation,
        progress: Arc::clone(&progress),
        limits,
        payload: 0,
    };
    run.boundary(QueryPhase::Open)?;
    let conn = open(&run)?;

    let done = AtomicBool::new(false);
    let interrupt = conn.get_interrupt_handle();
    // The watcher owns the only cross-thread interrupt path and is joined
    // before the connection is finalized or closed.
    let result = std::thread::scope(|scope| {
        let watcher = control.cancellation.map(|flag| {
            let progress = &progress;
            let done = &done;
            let interrupt = &interrupt;
            scope.spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    if flag.load(Ordering::SeqCst) {
                        progress.cancelled.store(true, Ordering::SeqCst);
                        interrupt.interrupt();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        });
        let result = acquire(&conn, bound.plan, &bound.values, &mut run);
        done.store(true, Ordering::SeqCst);
        if let Some(watcher) = watcher {
            let _ = watcher.join();
        }
        result
    });
    // A cancellation that arrived too late to stop the statement still wins.
    let result = result.and_then(|instance| {
        run.boundary(QueryPhase::Select)?;
        Ok(instance)
    });
    finish(&run, conn, result)
}

fn open(run: &Run<'_>) -> Result<Connection, QueryReadError> {
    // Explicit relative prefixing keeps `file:` and `:memory:` spellings literal
    // even when SQLite enables URI filenames globally.
    let literal = Path::new(".").join(run.source);
    let conn = Connection::open_with_flags(
        literal,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE,
    )
    .map_err(|cause| QueryReadError::sqlite(run.source, QueryPhase::Open, cause))?;
    configure(&conn).map_err(|cause| {
        QueryReadError::new(
            run.source,
            QueryPhase::Open,
            QueryReadErrorKind::Configuration,
        )
        .caused_by(cause)
    })?;
    let progress = Arc::clone(&run.progress);
    conn.progress_handler(
        super::QUERY_PROGRESS_INTERVAL as i32,
        Some(move || progress.check() != STOP_NONE),
    );
    Ok(conn)
}

/// Installs and verifies the fixed read-only connection settings. Extension
/// loading stays at SQLite's disabled default; the authorizer denies functions.
fn configure(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::config::DbConfig;
    conn.busy_timeout(Duration::ZERO)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "trusted_schema", false)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    let installed = conn.pragma_query_value(None, "query_only", |row| row.get::<_, i64>(0))? == 1
        && conn.pragma_query_value(None, "trusted_schema", |row| row.get::<_, i64>(0))? == 0
        && conn.pragma_query_value(None, "temp_store", |row| row.get::<_, i64>(0))? == 2
        && !conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    if !installed {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some("read-only connection settings were not installed".into()),
        ));
    }
    Ok(())
}

fn acquire(
    conn: &Connection,
    plan: &ValidatedSqliteRead,
    values: &[Value],
    run: &mut Run<'_>,
) -> Result<Instance, QueryReadError> {
    conn.execute_batch("BEGIN DEFERRED")
        .map_err(|cause| run.sqlite(QueryPhase::Metadata, cause))?;
    run.boundary(QueryPhase::Metadata)?;
    let admitted = admit(conn, run.source, plan, &|cause| {
        run.sqlite(QueryPhase::Metadata, cause)
    })?;
    run.boundary(QueryPhase::Metadata)?;
    install_authorizer(conn, &admitted);
    validate_candidates(conn, plan, &admitted, run)?;
    select(conn, plan, &admitted, values, run)
}

/// Permits SELECT and reads of the admitted table and columns in `main`;
/// everything else, including functions and other tables, is denied.
pub(super) fn install_authorizer(conn: &Connection, admitted: &Admitted) {
    let table = admitted.table.clone();
    let columns = admitted.columns.clone();
    conn.authorizer(Some(move |context: AuthContext<'_>| {
        let main = context.database_name.is_none_or(|name| name == "main");
        match context.action {
            AuthAction::Select if context.accessor.is_none() => Authorization::Allow,
            AuthAction::Read {
                table_name,
                column_name,
            } if main
                && context.accessor.is_none()
                && table_name.eq_ignore_ascii_case(&table)
                && columns
                    .iter()
                    .any(|column| column.eq_ignore_ascii_case(column_name)) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }
    }));
}

/// Compiles one emitted statement and verifies its admitted shape.
fn prepare<'c>(
    conn: &'c Connection,
    sql: &str,
    phase: QueryPhase,
    columns: usize,
    run: &Run<'_>,
) -> Result<Statement<'c>, QueryReadError> {
    if sql.len() > run.limits.sql_bytes {
        return Err(run.error(
            phase,
            QueryReadErrorKind::LimitExceeded(QueryLimit::SqlBytes),
        ));
    }
    let statement = conn
        .prepare(sql)
        .map_err(|cause| run.sqlite(phase, cause))?;
    if !statement.readonly() || statement.column_count() != columns {
        return Err(run.error(phase, QueryReadErrorKind::StatementShape));
    }
    Ok(statement)
}

fn validate_candidates(
    conn: &Connection,
    plan: &ValidatedSqliteRead,
    admitted: &Admitted,
    run: &mut Run<'_>,
) -> Result<(), QueryReadError> {
    let phase = QueryPhase::Candidates;
    let sql = format!(
        "SELECT {} FROM \"main\".{} ORDER BY {} ASC",
        admitted
            .columns
            .iter()
            .map(|column| quote(column))
            .collect::<Vec<_>>()
            .join(", "),
        quote(&admitted.table),
        quote(&admitted.key),
    );
    let mut statement = prepare(conn, &sql, phase, admitted.columns.len(), run)?;
    if statement.parameter_count() != 0 {
        return Err(run.error(phase, QueryReadErrorKind::StatementShape));
    }
    let mut rows = statement.raw_query();
    let mut row_number = 0u64;
    loop {
        run.boundary(phase)?;
        let Some(row) = rows.next().map_err(|cause| run.sqlite(phase, cause))? else {
            break;
        };
        row_number += 1;
        if row_number > run.limits.candidate_rows {
            return Err(run.error(
                phase,
                QueryReadErrorKind::LimitExceeded(QueryLimit::CandidateRows),
            ));
        }
        for (index, (column, ty)) in plan.columns().iter().enumerate() {
            run.boundary(phase)?;
            let cell = row
                .get_ref(index)
                .map_err(|cause| run.sqlite(phase, cause))?;
            run.charge(phase, column, row_number, cell)?;
            convert(cell, *ty).map_err(|found| {
                run.error(
                    phase,
                    QueryReadErrorKind::CellType {
                        expected: *ty,
                        found,
                    },
                )
                .column(column)
                .row(row_number)
            })?;
        }
    }
    Ok(())
}

fn select(
    conn: &Connection,
    plan: &ValidatedSqliteRead,
    admitted: &Admitted,
    values: &[Value],
    run: &mut Run<'_>,
) -> Result<Instance, QueryReadError> {
    let phase = QueryPhase::Select;
    let sql = selection_sql(plan, admitted);
    let mut statement = prepare(conn, &sql, phase, plan.projection().len(), run)?;
    if statement.parameter_count() != plan.parameters().len() {
        return Err(run.error(phase, QueryReadErrorKind::StatementShape));
    }
    for (parameter, value) in plan.parameters().iter().zip(values) {
        let index = statement
            .parameter_index(&format!(":{}", parameter.name))
            .map_err(|cause| run.sqlite(phase, cause))?
            .ok_or_else(|| run.error(phase, QueryReadErrorKind::StatementShape))?;
        let bound = match value {
            Value::Int(value) => statement.raw_bind_parameter(index, value),
            Value::Bool(value) => statement.raw_bind_parameter(index, i64::from(*value)),
            Value::Float(value) => statement.raw_bind_parameter(index, value),
            Value::String(value) => statement.raw_bind_parameter(index, value.as_str()),
            _ => return Err(run.error(phase, QueryReadErrorKind::StatementShape)),
        };
        bound.map_err(|cause| run.sqlite(phase, cause))?;
    }

    let mut rows = statement.raw_query();
    let mut output = Vec::new();
    let mut row_number = 0u64;
    loop {
        run.boundary(phase)?;
        let Some(row) = rows.next().map_err(|cause| run.sqlite(phase, cause))? else {
            break;
        };
        row_number += 1;
        if row_number > run.limits.output_rows {
            return Err(run.error(
                phase,
                QueryReadErrorKind::LimitExceeded(QueryLimit::OutputRows),
            ));
        }
        let mut fields = Vec::with_capacity(plan.projection().len());
        for (index, projected) in plan.projection().iter().enumerate() {
            run.boundary(phase)?;
            let cell = row
                .get_ref(index)
                .map_err(|cause| run.sqlite(phase, cause))?;
            run.charge(phase, &projected.column, row_number, cell)?;
            let value = convert(cell, projected.ty).map_err(|found| {
                run.error(
                    phase,
                    QueryReadErrorKind::CellType {
                        expected: projected.ty,
                        found,
                    },
                )
                .column(&projected.column)
                .row(row_number)
            })?;
            fields.push((projected.field.clone(), Instance::Scalar(value)));
        }
        output.push(Instance::Group(fields.into()));
    }
    Ok(Instance::Repeated(output))
}

/// The admitted SELECT: quoted canonical identifiers, named parameters,
/// explicit BINARY collation for String columns and explicit NULL placement.
pub(super) fn selection_sql(plan: &ValidatedSqliteRead, admitted: &Admitted) -> String {
    let column = |name: &str, ty: ScalarType| {
        let quoted = quote(admitted.canonical(plan, name));
        if ty == ScalarType::String {
            format!("{quoted} COLLATE BINARY")
        } else {
            quoted
        }
    };
    let projection = plan
        .projection()
        .iter()
        .map(|projected| quote(admitted.canonical(plan, &projected.column)))
        .collect::<Vec<_>>()
        .join(", ");
    let predicates = plan
        .predicates()
        .iter()
        .map(|predicate| match predicate.test {
            ValidatedPredicateTest::Compare { op, parameter } => format!(
                "{} {} :{}",
                column(&predicate.column, predicate.ty),
                op.sql(),
                plan.parameters()[parameter].name
            ),
            ValidatedPredicateTest::IsNull => {
                format!(
                    "{} IS NULL",
                    quote(admitted.canonical(plan, &predicate.column))
                )
            }
            ValidatedPredicateTest::IsNotNull => format!(
                "{} IS NOT NULL",
                quote(admitted.canonical(plan, &predicate.column))
            ),
        })
        .collect::<Vec<_>>();
    let order = plan
        .order_by()
        .iter()
        .map(|term| {
            format!(
                "{} {} {}",
                column(&term.column, term.ty),
                match term.direction {
                    SqliteReadDirection::Asc => "ASC",
                    SqliteReadDirection::Desc => "DESC",
                },
                match term.nulls {
                    SqliteReadNulls::First => "NULLS FIRST",
                    SqliteReadNulls::Last => "NULLS LAST",
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let filter = if predicates.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", predicates.join(" AND "))
    };
    format!(
        "SELECT {projection} FROM \"main\".{}{filter} ORDER BY {order}",
        quote(&admitted.table)
    )
}

/// The existing table-read cell conversion, with a typed found tag.
fn convert(cell: ValueRef<'_>, ty: ScalarType) -> Result<Value, &'static str> {
    match (ty, cell) {
        (_, ValueRef::Null) => Ok(Value::Null),
        (ScalarType::Bool, ValueRef::Integer(0)) => Ok(Value::Bool(false)),
        (ScalarType::Bool, ValueRef::Integer(1)) => Ok(Value::Bool(true)),
        (ScalarType::Bool, ValueRef::Integer(_)) => Err("integer other than 0 or 1"),
        (ScalarType::Int, ValueRef::Integer(value)) => Ok(Value::Int(value)),
        (ScalarType::Float, ValueRef::Integer(value)) => exact_f64(value)
            .map(Value::Float)
            .ok_or("integer outside the exact f64 range"),
        (ScalarType::Float, ValueRef::Real(value)) if value.is_finite() => Ok(Value::Float(value)),
        (ScalarType::String, ValueRef::Text(text)) => std::str::from_utf8(text)
            .map(|text| Value::String(text.to_owned()))
            .map_err(|_| "non-UTF-8 text"),
        (_, other) => Err(sqlite_type_name(other)),
    }
}

/// Finalizes statements, ends the read transaction and closes the
/// connection. A cleanup failure never replaces a primary error.
fn finish(
    run: &Run<'_>,
    conn: Connection,
    result: Result<Instance, QueryReadError>,
) -> Result<Instance, QueryReadError> {
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    conn.progress_handler(0, None::<fn() -> bool>);
    let mut failures = Vec::new();
    let mut first_cause = None;
    if !conn.is_autocommit()
        && let Err(cause) = conn.execute_batch("COMMIT")
    {
        failures.push(cleanup_failure(CleanupStep::EndTransaction, &cause));
        first_cause.get_or_insert(cause);
    }
    if let Err((_, cause)) = conn.close() {
        failures.push(cleanup_failure(CleanupStep::Close, &cause));
        first_cause.get_or_insert(cause);
    }
    match (result, first_cause) {
        (Err(mut error), _) => {
            error.cleanup = failures;
            Err(error)
        }
        (Ok(instance), None) => Ok(instance),
        (Ok(_), Some(cause)) => {
            let mut error = run
                .error(QueryPhase::Cleanup, QueryReadErrorKind::Cleanup)
                .caused_by(cause);
            error.cleanup = failures;
            Err(error)
        }
    }
}

fn cleanup_failure(step: CleanupStep, cause: &rusqlite::Error) -> QueryCleanupFailure {
    QueryCleanupFailure {
        step,
        sqlite: sqlite_codes(cause),
        message: cause.to_string(),
    }
}
