//! Declared read-only SQLite single-table query profile, version 1.
//!
//! A saved [`SqliteReadProfile`] is a closed descriptor: physical table,
//! proved rowid key, ordered projection, typed named parameters, a conjunction
//! of comparisons and an explicit total row order. It carries no SQL text.
//! [`validate_sqlite_read_profile`] checks it against the source schema without
//! any filesystem access and returns the only constructor of a
//! [`ValidatedSqliteRead`] plan. Database metadata, bindings and execution are
//! owned by the SQLite reader.

use std::collections::BTreeMap;
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use serde::{Deserialize, Serialize};

use crate::{FormatOptions, Project};

/// File capability recorded by saved designs that carry a query profile.
pub const SQLITE_READ_V1_CAPABILITY: &str = "sqlite_read_v1";
/// Distinct physical columns referenced by projection, predicates and order.
pub const MAX_SQLITE_READ_COLUMNS: usize = 128;
pub const MAX_SQLITE_READ_PARAMETERS: usize = 32;
pub const MAX_SQLITE_READ_PREDICATES: usize = 64;
pub const MAX_SQLITE_READ_ORDER_TERMS: usize = 128;
pub const MAX_SQLITE_READ_IDENTIFIER_BYTES: usize = 256;
pub const MAX_SQLITE_READ_PARAMETER_NAME_BYTES: usize = 128;

/// A tagged query descriptor. Unknown kinds and members refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SqliteReadProfile {
    SqliteReadV1(SqliteReadV1),
}

/// The version-1 descriptor body. Every member is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadV1 {
    pub table: String,
    pub key: String,
    pub projection: Vec<SqliteReadProjection>,
    pub parameters: Vec<SqliteReadParameter>,
    pub all: Vec<SqliteReadPredicate>,
    pub order_by: Vec<SqliteReadOrder>,
}

/// Maps one schema field to one physical column. The field is a schema name,
/// never an SQL expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadProjection {
    pub field: String,
    pub column: String,
}

/// A typed named query parameter bound from one required runtime parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ScalarType,
    pub runtime_parameter: String,
}

/// One conjunct of the `all` filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SqliteReadPredicate {
    Eq(SqliteReadComparison),
    Ne(SqliteReadComparison),
    Lt(SqliteReadComparison),
    Le(SqliteReadComparison),
    Gt(SqliteReadComparison),
    Ge(SqliteReadComparison),
    IsNull(SqliteReadNullTest),
    IsNotNull(SqliteReadNullTest),
}

/// A column compared with exactly one declared parameter of the same type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadComparison {
    pub column: String,
    #[serde(rename = "type")]
    pub ty: ScalarType,
    pub parameter: String,
}

/// An explicit SQL NULL test of one column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadNullTest {
    pub column: String,
    #[serde(rename = "type")]
    pub ty: ScalarType,
}

/// One ordering term with explicit direction and NULL placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqliteReadOrder {
    pub column: String,
    #[serde(rename = "type")]
    pub ty: ScalarType,
    pub direction: SqliteReadDirection,
    pub nulls: SqliteReadNulls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqliteReadDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqliteReadNulls {
    First,
    Last,
}

/// A comparison operator admitted by version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteReadComparisonOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl SqliteReadComparisonOp {
    /// The fixed SQL operator token.
    pub fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

impl SqliteReadPredicate {
    fn column(&self) -> &str {
        match self {
            Self::Eq(c) | Self::Ne(c) | Self::Lt(c) | Self::Le(c) | Self::Gt(c) | Self::Ge(c) => {
                &c.column
            }
            Self::IsNull(test) | Self::IsNotNull(test) => &test.column,
        }
    }

    fn ty(&self) -> ScalarType {
        match self {
            Self::Eq(c) | Self::Ne(c) | Self::Lt(c) | Self::Le(c) | Self::Gt(c) | Self::Ge(c) => {
                c.ty
            }
            Self::IsNull(test) | Self::IsNotNull(test) => test.ty,
        }
    }

    fn comparison(&self) -> Option<(SqliteReadComparisonOp, &SqliteReadComparison)> {
        use SqliteReadComparisonOp as Op;
        match self {
            Self::Eq(c) => Some((Op::Eq, c)),
            Self::Ne(c) => Some((Op::Ne, c)),
            Self::Lt(c) => Some((Op::Lt, c)),
            Self::Le(c) => Some((Op::Le, c)),
            Self::Gt(c) => Some((Op::Gt, c)),
            Self::Ge(c) => Some((Op::Ge, c)),
            Self::IsNull(_) | Self::IsNotNull(_) => None,
        }
    }
}

/// Why a descriptor cannot be admitted. `path` is a JSON pointer into the
/// descriptor, or `schema` / `schema/<field>` for the source schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryProfileError {
    pub path: String,
    pub reason: QueryProfileReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryProfileReason {
    SchemaNotRepeatingGroup,
    SchemaFieldNotPlainScalar,
    DuplicateSchemaField,
    EmptyIdentifier,
    IdentifierTooLong {
        limit: usize,
    },
    IdentifierContainsNul,
    TooMany {
        limit: usize,
    },
    ProjectionFieldOrder {
        expected: String,
    },
    ProjectionCount {
        expected: usize,
    },
    DuplicateColumn,
    ColumnTypeConflict {
        first: ScalarType,
        found: ScalarType,
    },
    InvalidParameterName,
    DuplicateParameter,
    UnknownParameter,
    ParameterTypeMismatch {
        parameter: ScalarType,
    },
    UnreferencedParameter,
    BooleanOrdering,
    DuplicatePredicate,
    EmptyOrder,
    KeyTermNotLast,
    KeyTermShape,
}

impl fmt::Display for QueryProfileReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaNotRepeatingGroup => {
                f.write_str("the source schema must be a repeating group of scalar fields")
            }
            Self::SchemaFieldNotPlainScalar => f.write_str(
                "each field must be a nonrepeating String, Int, Float or Bool scalar without other format metadata",
            ),
            Self::DuplicateSchemaField => f.write_str("schema field names must be distinct"),
            Self::EmptyIdentifier => f.write_str("identifier must not be empty"),
            Self::IdentifierTooLong { limit } => {
                write!(f, "identifier exceeds {limit} UTF-8 bytes")
            }
            Self::IdentifierContainsNul => f.write_str("identifier must not contain NUL"),
            Self::TooMany { limit } => write!(f, "at most {limit} entries are allowed"),
            Self::ProjectionFieldOrder { expected } => {
                write!(f, "projection must follow schema field order; expected `{expected}`")
            }
            Self::ProjectionCount { expected } => {
                write!(f, "projection must map all {expected} schema fields exactly once")
            }
            Self::DuplicateColumn => f.write_str("column is repeated in this array"),
            Self::ColumnTypeConflict { first, found } => {
                write!(f, "column is used as {found:?} after {first:?}")
            }
            Self::InvalidParameterName => f.write_str(
                "parameter names must match [A-Za-z_][A-Za-z0-9_]* and be at most 128 bytes",
            ),
            Self::DuplicateParameter => f.write_str("parameter name is repeated"),
            Self::UnknownParameter => f.write_str("predicate references an undeclared parameter"),
            Self::ParameterTypeMismatch { parameter } => {
                write!(f, "predicate type differs from parameter type {parameter:?}")
            }
            Self::UnreferencedParameter => {
                f.write_str("declared parameter is not referenced by any predicate")
            }
            Self::BooleanOrdering => {
                f.write_str("Bool columns admit only eq, ne and NULL-test predicates")
            }
            Self::DuplicatePredicate => f.write_str("predicate is repeated"),
            Self::EmptyOrder => f.write_str("order_by must end with the key"),
            Self::KeyTermNotLast => f.write_str("the key must appear exactly once, as the last ordering term"),
            Self::KeyTermShape => {
                f.write_str("the final key term must be Int, ascending, with nulls last")
            }
        }
    }
}

impl fmt::Display for QueryProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid SQLite query profile at {}: {}",
            self.path, self.reason
        )
    }
}

impl std::error::Error for QueryProfileError {}

/// One projected schema field and its physical column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProjection {
    pub field: String,
    pub column: String,
    pub ty: ScalarType,
}

/// One declared parameter in stored order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedParameter {
    pub name: String,
    pub ty: ScalarType,
    pub runtime_parameter: String,
}

/// One admitted predicate. Comparisons name a parameter by its declaration index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatedPredicateTest {
    Compare {
        op: SqliteReadComparisonOp,
        parameter: usize,
    },
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPredicate {
    pub column: String,
    pub ty: ScalarType,
    pub test: ValidatedPredicateTest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedOrder {
    pub column: String,
    pub ty: ScalarType,
    pub direction: SqliteReadDirection,
    pub nulls: SqliteReadNulls,
}

/// A descriptor admitted against its source schema. It can be produced only by
/// [`validate_sqlite_read_profile`] and is never deserialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSqliteRead {
    table: String,
    key: String,
    projection: Vec<ValidatedProjection>,
    parameters: Vec<ValidatedParameter>,
    predicates: Vec<ValidatedPredicate>,
    order_by: Vec<ValidatedOrder>,
    columns: Vec<(String, ScalarType)>,
}

impl ValidatedSqliteRead {
    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn projection(&self) -> &[ValidatedProjection] {
        &self.projection
    }

    pub fn parameters(&self) -> &[ValidatedParameter] {
        &self.parameters
    }

    pub fn predicates(&self) -> &[ValidatedPredicate] {
        &self.predicates
    }

    pub fn order_by(&self) -> &[ValidatedOrder] {
        &self.order_by
    }

    /// Distinct referenced columns in first-use order: projection, predicates,
    /// then ordering. Spellings are those first written in the descriptor.
    pub fn columns(&self) -> &[(String, ScalarType)] {
        &self.columns
    }
}

/// Validates a descriptor and its source schema in stored member/array order
/// without touching the filesystem.
pub fn validate_sqlite_read_profile(
    schema: &SchemaNode,
    profile: &SqliteReadProfile,
) -> Result<ValidatedSqliteRead, QueryProfileError> {
    let SqliteReadProfile::SqliteReadV1(profile) = profile;
    let fields = schema_fields(schema)?;
    identifier("/table", &profile.table)?;
    identifier("/key", &profile.key)?;

    let mut columns = ColumnTypes::default();
    limit(
        "/projection",
        profile.projection.len(),
        MAX_SQLITE_READ_COLUMNS,
    )?;
    let mut projection = Vec::with_capacity(profile.projection.len());
    for (index, entry) in profile.projection.iter().enumerate() {
        let path = format!("/projection/{index}");
        let (expected, ty) = fields.get(index).ok_or_else(|| {
            error(
                &path,
                QueryProfileReason::ProjectionCount {
                    expected: fields.len(),
                },
            )
        })?;
        identifier(&format!("{path}/field"), &entry.field)?;
        if entry.field != **expected {
            return Err(error(
                &format!("{path}/field"),
                QueryProfileReason::ProjectionFieldOrder {
                    expected: (*expected).to_owned(),
                },
            ));
        }
        let column_path = format!("{path}/column");
        identifier(&column_path, &entry.column)?;
        if profile.projection[..index]
            .iter()
            .any(|previous| same_identifier(&previous.column, &entry.column))
        {
            return Err(error(&column_path, QueryProfileReason::DuplicateColumn));
        }
        columns.use_column(&column_path, &entry.column, *ty)?;
        projection.push(ValidatedProjection {
            field: entry.field.clone(),
            column: entry.column.clone(),
            ty: *ty,
        });
    }
    if projection.len() != fields.len() {
        return Err(error(
            "/projection",
            QueryProfileReason::ProjectionCount {
                expected: fields.len(),
            },
        ));
    }

    limit(
        "/parameters",
        profile.parameters.len(),
        MAX_SQLITE_READ_PARAMETERS,
    )?;
    let mut parameters = Vec::with_capacity(profile.parameters.len());
    for (index, parameter) in profile.parameters.iter().enumerate() {
        let path = format!("/parameters/{index}");
        if !is_parameter_name(&parameter.name) {
            return Err(error(
                &format!("{path}/name"),
                QueryProfileReason::InvalidParameterName,
            ));
        }
        if profile.parameters[..index]
            .iter()
            .any(|previous| previous.name == parameter.name)
        {
            return Err(error(
                &format!("{path}/name"),
                QueryProfileReason::DuplicateParameter,
            ));
        }
        runtime_parameter_name(
            &format!("{path}/runtime_parameter"),
            &parameter.runtime_parameter,
        )?;
        parameters.push(ValidatedParameter {
            name: parameter.name.clone(),
            ty: parameter.ty,
            runtime_parameter: parameter.runtime_parameter.clone(),
        });
    }

    limit("/all", profile.all.len(), MAX_SQLITE_READ_PREDICATES)?;
    let mut referenced = vec![false; parameters.len()];
    let mut predicates = Vec::with_capacity(profile.all.len());
    for (index, predicate) in profile.all.iter().enumerate() {
        let path = format!("/all/{index}");
        let column_path = format!("{path}/column");
        identifier(&column_path, predicate.column())?;
        if profile.all[..index].contains(predicate) {
            return Err(error(&path, QueryProfileReason::DuplicatePredicate));
        }
        let test = match predicate.comparison() {
            Some((op, comparison)) => {
                if comparison.ty == ScalarType::Bool
                    && !matches!(op, SqliteReadComparisonOp::Eq | SqliteReadComparisonOp::Ne)
                {
                    return Err(error(&path, QueryProfileReason::BooleanOrdering));
                }
                let parameter_path = format!("{path}/parameter");
                let parameter = parameters
                    .iter()
                    .position(|declared| declared.name == comparison.parameter)
                    .ok_or_else(|| error(&parameter_path, QueryProfileReason::UnknownParameter))?;
                if parameters[parameter].ty != comparison.ty {
                    return Err(error(
                        &parameter_path,
                        QueryProfileReason::ParameterTypeMismatch {
                            parameter: parameters[parameter].ty,
                        },
                    ));
                }
                referenced[parameter] = true;
                ValidatedPredicateTest::Compare { op, parameter }
            }
            None if matches!(predicate, SqliteReadPredicate::IsNull(_)) => {
                ValidatedPredicateTest::IsNull
            }
            None => ValidatedPredicateTest::IsNotNull,
        };
        columns.use_column(&column_path, predicate.column(), predicate.ty())?;
        predicates.push(ValidatedPredicate {
            column: predicate.column().to_owned(),
            ty: predicate.ty(),
            test,
        });
    }
    if let Some(index) = referenced.iter().position(|referenced| !referenced) {
        return Err(error(
            &format!("/parameters/{index}"),
            QueryProfileReason::UnreferencedParameter,
        ));
    }

    limit(
        "/order_by",
        profile.order_by.len(),
        MAX_SQLITE_READ_ORDER_TERMS,
    )?;
    if profile.order_by.is_empty() {
        return Err(error("/order_by", QueryProfileReason::EmptyOrder));
    }
    let last = profile.order_by.len() - 1;
    let mut order_by = Vec::with_capacity(profile.order_by.len());
    for (index, term) in profile.order_by.iter().enumerate() {
        let path = format!("/order_by/{index}");
        let column_path = format!("{path}/column");
        identifier(&column_path, &term.column)?;
        if profile.order_by[..index]
            .iter()
            .any(|previous| same_identifier(&previous.column, &term.column))
        {
            return Err(error(&column_path, QueryProfileReason::DuplicateColumn));
        }
        let is_key = same_identifier(&term.column, &profile.key);
        if is_key != (index == last) {
            return Err(error(&column_path, QueryProfileReason::KeyTermNotLast));
        }
        if is_key
            && (term.ty != ScalarType::Int
                || term.direction != SqliteReadDirection::Asc
                || term.nulls != SqliteReadNulls::Last)
        {
            return Err(error(&path, QueryProfileReason::KeyTermShape));
        }
        columns.use_column(&column_path, &term.column, term.ty)?;
        order_by.push(ValidatedOrder {
            column: term.column.clone(),
            ty: term.ty,
            direction: term.direction,
            nulls: term.nulls,
        });
    }
    Ok(ValidatedSqliteRead {
        table: profile.table.clone(),
        key: profile.key.clone(),
        projection,
        parameters,
        predicates,
        order_by,
        columns: columns.order,
    })
}

fn schema_fields(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, QueryProfileError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(error("schema", QueryProfileReason::SchemaNotRepeatingGroup));
    };
    let plain_root = SchemaNode::group(schema.name.clone(), children.clone()).repeating();
    if *schema != plain_root {
        return Err(error("schema", QueryProfileReason::SchemaNotRepeatingGroup));
    }
    let mut fields = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
        let path = format!("schema/{}", child.name);
        let SchemaKind::Scalar { ty } = child.kind else {
            return Err(error(&path, QueryProfileReason::SchemaFieldNotPlainScalar));
        };
        if *child != SchemaNode::scalar(child.name.clone(), ty) {
            return Err(error(&path, QueryProfileReason::SchemaFieldNotPlainScalar));
        }
        if children[..index]
            .iter()
            .any(|previous| previous.name == child.name)
        {
            return Err(error(&path, QueryProfileReason::DuplicateSchemaField));
        }
        fields.push((child.name.as_str(), ty));
    }
    if fields.is_empty() {
        return Err(error("schema", QueryProfileReason::SchemaNotRepeatingGroup));
    }
    Ok(fields)
}

/// Agreed scalar types for distinct columns, in first-use order.
#[derive(Default)]
struct ColumnTypes {
    order: Vec<(String, ScalarType)>,
    index: BTreeMap<String, usize>,
}

impl ColumnTypes {
    fn use_column(
        &mut self,
        path: &str,
        column: &str,
        ty: ScalarType,
    ) -> Result<(), QueryProfileError> {
        let folded = column.to_ascii_lowercase();
        if let Some(&index) = self.index.get(&folded) {
            let first = self.order[index].1;
            if first != ty {
                return Err(error(
                    path,
                    QueryProfileReason::ColumnTypeConflict { first, found: ty },
                ));
            }
            return Ok(());
        }
        if self.order.len() == MAX_SQLITE_READ_COLUMNS {
            return Err(error(
                path,
                QueryProfileReason::TooMany {
                    limit: MAX_SQLITE_READ_COLUMNS,
                },
            ));
        }
        self.index.insert(folded, self.order.len());
        self.order.push((column.to_owned(), ty));
        Ok(())
    }
}

/// SQLite compares identifiers with ASCII-only case folding.
fn same_identifier(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn identifier(path: &str, value: &str) -> Result<(), QueryProfileError> {
    if value.is_empty() {
        Err(error(path, QueryProfileReason::EmptyIdentifier))
    } else if value.len() > MAX_SQLITE_READ_IDENTIFIER_BYTES {
        Err(error(
            path,
            QueryProfileReason::IdentifierTooLong {
                limit: MAX_SQLITE_READ_IDENTIFIER_BYTES,
            },
        ))
    } else if value.contains('\0') {
        Err(error(path, QueryProfileReason::IdentifierContainsNul))
    } else {
        Ok(())
    }
}

/// Runtime parameter names follow the host's runtime-parameter limits.
fn runtime_parameter_name(path: &str, name: &str) -> Result<(), QueryProfileError> {
    if name.is_empty() {
        Err(error(path, QueryProfileReason::EmptyIdentifier))
    } else if name.len() > crate::MAX_RUNTIME_PARAMETER_NAME_BYTES {
        Err(error(
            path,
            QueryProfileReason::IdentifierTooLong {
                limit: crate::MAX_RUNTIME_PARAMETER_NAME_BYTES,
            },
        ))
    } else if name.contains('\0') {
        Err(error(path, QueryProfileReason::IdentifierContainsNul))
    } else {
        Ok(())
    }
}

fn is_parameter_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_SQLITE_READ_PARAMETER_NAME_BYTES
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn limit(path: &str, length: usize, limit: usize) -> Result<(), QueryProfileError> {
    if length > limit {
        Err(error(path, QueryProfileReason::TooMany { limit }))
    } else {
        Ok(())
    }
}

fn error(path: &str, reason: QueryProfileReason) -> QueryProfileError {
    QueryProfileError {
        path: path.to_owned(),
        reason,
    }
}

/// The owner of one declared query input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteReadOwner<'a> {
    Primary,
    Named(&'a str),
}

/// One declared query input with its schema.
#[derive(Debug, Clone, Copy)]
pub struct SqliteReadInput<'a> {
    pub owner: SqliteReadOwner<'a>,
    pub schema: &'a SchemaNode,
    pub profile: &'a SqliteReadProfile,
    /// Whether the owner is a per-item dynamic source, which v1 refuses.
    pub dynamic: bool,
}

impl Project {
    /// Every source-side query profile in primary-then-declaration order.
    pub fn sqlite_read_inputs(&self) -> Vec<SqliteReadInput<'_>> {
        let primary = self
            .source_options
            .sqlite_read
            .as_ref()
            .map(|profile| SqliteReadInput {
                owner: SqliteReadOwner::Primary,
                schema: &self.source,
                profile,
                dynamic: false,
            });
        primary
            .into_iter()
            .chain(self.extra_sources.iter().filter_map(|source| {
                source
                    .options
                    .sqlite_read
                    .as_ref()
                    .map(|profile| SqliteReadInput {
                        owner: SqliteReadOwner::Named(&source.name),
                        schema: &source.schema,
                        profile,
                        dynamic: source.dynamic_path.is_some(),
                    })
            }))
            .collect()
    }

    /// Whether any format options anywhere in the project carry a query
    /// profile, including invalid target placements.
    pub(crate) fn uses_sqlite_read(&self) -> bool {
        let has = |options: &FormatOptions| options.sqlite_read.is_some();
        has(&self.source_options)
            || has(&self.target_options)
            || self.extra_sources.iter().any(|source| has(&source.options))
            || self.extra_targets.iter().any(|target| has(&target.options))
    }
}

#[cfg(test)]
#[path = "sqlite_read_tests.rs"]
mod tests;
