//! Declared read-only SQLite query acquisition (`sqlite_read_v1`).
//!
//! [`read_query`] executes a [`ValidatedSqliteRead`] plan against one existing
//! database file in this order: binding preflight, cancellation/deadline, a
//! read-only private-cache open, metadata/key admission inside one deferred
//! read transaction, validation of every referenced candidate cell in key
//! order, the admitted SELECT, then statement finalization, transaction end and
//! connection close. Every failure is a typed [`QueryReadError`]; no partial
//! instance is ever returned. Work, payload and time limits are cooperative
//! application limits, not hard isolation for adversarial database files.

mod admit;
mod bind;
mod execute;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::ValidatedSqliteRead;

#[cfg(test)]
mod tests;

/// Maximum candidate rows scanned during cell validation.
pub const MAX_QUERY_CANDIDATE_ROWS: u64 = 100_000;
pub const MAX_QUERY_OUTPUT_ROWS: u64 = 100_000;
/// Decoded bytes of one TEXT or BLOB cell; numeric cells count eight bytes.
pub const MAX_QUERY_CELL_BYTES: u64 = 1024 * 1024;
/// Cumulative decoded scalar bytes across validation and output.
pub const MAX_QUERY_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_QUERY_SQL_BYTES: usize = 64 * 1024;
pub const MAX_QUERY_DEADLINE: Duration = Duration::from_secs(5);
/// Approximate SQLite VM instructions, charged per progress callback.
pub const MAX_QUERY_VM_WORK: u64 = 10_000_000;
/// VM instructions between progress callbacks.
pub const QUERY_PROGRESS_INTERVAL: u64 = 1_000;

/// Host-selected resource limits. Each can only be lowered from the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryReadLimits {
    candidate_rows: u64,
    output_rows: u64,
    cell_bytes: u64,
    payload_bytes: u64,
    sql_bytes: usize,
    deadline: Duration,
    vm_work: u64,
}

impl Default for QueryReadLimits {
    fn default() -> Self {
        Self::MAXIMUM
    }
}

impl QueryReadLimits {
    pub const MAXIMUM: Self = Self {
        candidate_rows: MAX_QUERY_CANDIDATE_ROWS,
        output_rows: MAX_QUERY_OUTPUT_ROWS,
        cell_bytes: MAX_QUERY_CELL_BYTES,
        payload_bytes: MAX_QUERY_PAYLOAD_BYTES,
        sql_bytes: MAX_QUERY_SQL_BYTES,
        deadline: MAX_QUERY_DEADLINE,
        vm_work: MAX_QUERY_VM_WORK,
    };

    pub fn with_candidate_rows(self, rows: u64) -> Self {
        Self {
            candidate_rows: rows.min(self.candidate_rows),
            ..self
        }
    }

    pub fn with_output_rows(self, rows: u64) -> Self {
        Self {
            output_rows: rows.min(self.output_rows),
            ..self
        }
    }

    pub fn with_cell_bytes(self, bytes: u64) -> Self {
        Self {
            cell_bytes: bytes.min(self.cell_bytes),
            ..self
        }
    }

    pub fn with_payload_bytes(self, bytes: u64) -> Self {
        Self {
            payload_bytes: bytes.min(self.payload_bytes),
            ..self
        }
    }

    pub fn with_sql_bytes(self, bytes: usize) -> Self {
        Self {
            sql_bytes: bytes.min(self.sql_bytes),
            ..self
        }
    }

    pub fn with_deadline(self, deadline: Duration) -> Self {
        Self {
            deadline: deadline.min(self.deadline),
            ..self
        }
    }

    pub fn with_vm_work(self, work: u64) -> Self {
        Self {
            vm_work: work.min(self.vm_work),
            ..self
        }
    }
}

/// Cancellation and limits supplied by the acquisition owner.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryReadControl<'a> {
    cancellation: Option<&'a AtomicBool>,
    limits: QueryReadLimits,
}

impl<'a> QueryReadControl<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// A flag another thread may set; it interrupts SQLite through an owned
    /// handle whose watcher is joined before the connection closes.
    pub fn with_cancellation(mut self, cancellation: &'a AtomicBool) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn with_limits(mut self, limits: QueryReadLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> QueryReadLimits {
        self.limits
    }
}

/// A plan whose bindings passed preflight, ready to open its database.
#[derive(Debug, Clone)]
pub struct BoundSqliteRead<'p> {
    source: PathBuf,
    plan: &'p ValidatedSqliteRead,
    /// One value per declared parameter, in declaration order.
    values: Vec<Value>,
}

impl BoundSqliteRead<'_> {
    pub fn source(&self) -> &Path {
        &self.source
    }
}

/// Checks bindings for `plan` without any filesystem access: duplicate names
/// in supplied order, then unknown names in supplied order, then each
/// declaration in stored order (missing, NULL, wrong tag, non-finite Float).
pub fn bind_query<'p>(
    path: &Path,
    schema: &SchemaNode,
    plan: &'p ValidatedSqliteRead,
    bindings: &[(String, Value)],
) -> Result<BoundSqliteRead<'p>, QueryReadError> {
    bind::bind(path, schema, plan, bindings)
}

/// Opens the bound database read-only and materializes its rows.
pub fn read_bound_query(
    bound: &BoundSqliteRead<'_>,
    control: &QueryReadControl<'_>,
) -> Result<Instance, QueryReadError> {
    execute::read(bound, control)
}

/// Binds and executes one declared query. Zero rows yield an empty
/// [`Instance::Repeated`]; SQL NULL becomes `Value::Null` in a present field.
pub fn read_query(
    path: &Path,
    schema: &SchemaNode,
    plan: &ValidatedSqliteRead,
    bindings: &[(String, Value)],
    control: &QueryReadControl<'_>,
) -> Result<Instance, QueryReadError> {
    read_bound_query(&bind_query(path, schema, plan, bindings)?, control)
}

/// The acquisition step that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPhase {
    Bind,
    Open,
    Metadata,
    Candidates,
    Select,
    Cleanup,
}

/// A resource limit that refused the read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLimit {
    CandidateRows,
    OutputRows,
    CellBytes,
    PayloadBytes,
    SqlBytes,
    VmWork,
}

/// Why the physical table cannot be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForbiddenTable {
    View,
    Virtual,
    Shadow,
    Internal,
    WithoutRowid,
}

/// Why the declared key is not a proved rowid alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProblem {
    NotPrimaryKey,
    CompositePrimaryKey,
    NotRowidAlias,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryReadErrorKind {
    /// The schema's fields differ from those the plan was validated against.
    PlanSchemaMismatch,
    DuplicateParameter,
    UnknownParameter,
    MissingParameter,
    NullParameter,
    ParameterType {
        expected: ScalarType,
        found: &'static str,
    },
    NonFiniteParameter,
    Cancelled,
    DeadlineExceeded,
    /// SQLITE_BUSY or SQLITE_LOCKED; v1 never retries.
    Busy,
    /// The existing database could not be opened read-only.
    Open,
    /// A required read-only connection setting could not be installed.
    Configuration,
    NoSuchTable,
    ForbiddenTable(ForbiddenTable),
    InvalidKey(KeyProblem),
    MissingColumn,
    AmbiguousColumn,
    GeneratedColumn,
    ColumnAffinity {
        expected: ScalarType,
        declared: String,
        affinity: &'static str,
    },
    CellType {
        expected: ScalarType,
        found: &'static str,
    },
    LimitExceeded(QueryLimit),
    /// The authorizer refused an emitted statement.
    Unauthorized,
    /// A compiled statement did not have the admitted read-only shape.
    StatementShape,
    /// Another SQLite failure; codes and cause are retained.
    Sqlite,
    /// Acquisition succeeded but ending the transaction or closing failed.
    Cleanup,
}

/// SQLite primary and extended result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteCodes {
    pub primary: i32,
    pub extended: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupStep {
    EndTransaction,
    Close,
}

/// A cleanup failure attached to the primary outcome without replacing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCleanupFailure {
    pub step: CleanupStep,
    pub sqlite: Option<SqliteCodes>,
    pub message: String,
}

/// A typed acquisition refusal. Parameter values are never retained. Its
/// details are boxed and reached through `Deref`.
#[derive(Debug)]
pub struct QueryReadError(Box<QueryReadDetails>);

impl std::ops::Deref for QueryReadError {
    type Target = QueryReadDetails;

    fn deref(&self) -> &QueryReadDetails {
        &self.0
    }
}

impl std::ops::DerefMut for QueryReadError {
    fn deref_mut(&mut self) -> &mut QueryReadDetails {
        &mut self.0
    }
}

#[derive(Debug)]
pub struct QueryReadDetails {
    pub source: PathBuf,
    pub phase: QueryPhase,
    pub kind: QueryReadErrorKind,
    pub parameter: Option<String>,
    pub column: Option<String>,
    /// One-based candidate or output row in the phase's key/declared order.
    pub row: Option<u64>,
    pub sqlite: Option<SqliteCodes>,
    pub cleanup: Vec<QueryCleanupFailure>,
    cause: Option<rusqlite::Error>,
}

impl QueryReadError {
    pub(crate) fn new(source: &Path, phase: QueryPhase, kind: QueryReadErrorKind) -> Self {
        Self(Box::new(QueryReadDetails {
            source: source.to_path_buf(),
            phase,
            kind,
            parameter: None,
            column: None,
            row: None,
            sqlite: None,
            cleanup: Vec::new(),
            cause: None,
        }))
    }

    pub(crate) fn parameter(mut self, parameter: &str) -> Self {
        self.parameter = Some(parameter.to_owned());
        self
    }

    pub(crate) fn column(mut self, column: &str) -> Self {
        self.column = Some(column.to_owned());
        self
    }

    pub(crate) fn row(mut self, row: u64) -> Self {
        self.row = Some(row);
        self
    }

    pub(crate) fn caused_by(mut self, cause: rusqlite::Error) -> Self {
        self.sqlite = sqlite_codes(&cause);
        self.cause = Some(cause);
        self
    }

    /// Classifies a SQLite failure. Busy/locked results are typed refusals.
    pub(crate) fn sqlite(source: &Path, phase: QueryPhase, cause: rusqlite::Error) -> Self {
        let kind = match sqlite_codes(&cause).map(|codes| codes.primary) {
            Some(rusqlite::ffi::SQLITE_BUSY | rusqlite::ffi::SQLITE_LOCKED) => {
                QueryReadErrorKind::Busy
            }
            Some(rusqlite::ffi::SQLITE_AUTH) => QueryReadErrorKind::Unauthorized,
            _ if denied_by_authorizer(&cause) => QueryReadErrorKind::Unauthorized,
            _ if phase == QueryPhase::Open => QueryReadErrorKind::Open,
            _ => QueryReadErrorKind::Sqlite,
        };
        Self::new(source, phase, kind).caused_by(cause)
    }
}

pub(crate) fn sqlite_codes(error: &rusqlite::Error) -> Option<SqliteCodes> {
    let error = match error {
        rusqlite::Error::SqlInputError { error, .. } => error,
        other => other.sqlite_error()?,
    };
    Some(SqliteCodes {
        primary: error.extended_code & 0xff,
        extended: error.extended_code,
    })
}

/// SQLite reports a denied function as an ordinary prepare error.
fn denied_by_authorizer(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::SqlInputError { msg, .. } if msg.starts_with("not authorized"))
}

impl fmt::Display for QueryReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SQLite query {} of {} failed: {:?}",
            match self.phase {
                QueryPhase::Bind => "binding",
                QueryPhase::Open => "open",
                QueryPhase::Metadata => "metadata admission",
                QueryPhase::Candidates => "candidate validation",
                QueryPhase::Select => "selection",
                QueryPhase::Cleanup => "cleanup",
            },
            self.source.display(),
            self.kind
        )?;
        if let Some(parameter) = &self.parameter {
            write!(f, " (parameter `{parameter}`)")?;
        }
        if let Some(column) = &self.column {
            write!(f, " (column `{column}`)")?;
        }
        if let Some(row) = self.row {
            write!(f, " (row {row})")?;
        }
        if let Some(codes) = self.sqlite {
            write!(f, " (SQLite code {}/{})", codes.primary, codes.extended)?;
        }
        for failure in &self.cleanup {
            write!(
                f,
                "; cleanup {:?} also failed: {}",
                failure.step, failure.message
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for QueryReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0
            .cause
            .as_ref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}
