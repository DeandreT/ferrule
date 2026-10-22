//! Authored-fixture oracles and refusal qualification for declared reads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{SqliteReadProfile, ValidatedSqliteRead, validate_sqlite_read_profile};
use rusqlite::Connection;

use super::*;

mod ferrule_tables;
mod refusals;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/sqlite-read-v1"
);
static NEXT: AtomicU64 = AtomicU64::new(0);

/// A unique scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-sqlite-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The authored seed database.
    fn seeded(&self) -> PathBuf {
        let path = self.path("orders.db");
        self.database(&path, &fixture("seed.sql"));
        path
    }

    fn database(&self, path: &Path, sql: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(sql).unwrap();
        conn.close().unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

fn json(name: &str) -> serde_json::Value {
    serde_json::from_str(&fixture(name)).unwrap()
}

fn scalar_type(name: &str) -> ScalarType {
    serde_json::from_value(name.into()).unwrap()
}

fn schema(fields: &[(&str, ScalarType)]) -> SchemaNode {
    SchemaNode::group(
        "orders",
        fields
            .iter()
            .map(|(name, ty)| SchemaNode::scalar(*name, *ty))
            .collect(),
    )
    .repeating()
}

fn orders_schema() -> SchemaNode {
    schema(&[
        ("id", ScalarType::Int),
        ("label", ScalarType::String),
        ("score", ScalarType::Int),
        ("active", ScalarType::Bool),
        ("ratio", ScalarType::Float),
    ])
}

fn plan(schema: &SchemaNode, profile: serde_json::Value) -> ValidatedSqliteRead {
    let profile: SqliteReadProfile = serde_json::from_value(profile).unwrap();
    validate_sqlite_read_profile(schema, &profile).unwrap()
}

fn authored_plan() -> ValidatedSqliteRead {
    plan(&orders_schema(), json("profile.json"))
}

/// Decodes the fixtures' `[tag, value]` scalar notation.
fn tagged(cell: &serde_json::Value) -> Value {
    let cell = cell.as_array().unwrap();
    match (cell[0].as_str().unwrap(), cell.get(1)) {
        ("null", None) => Value::Null,
        ("int", Some(value)) => Value::Int(value.as_i64().unwrap()),
        ("float", Some(value)) => Value::Float(value.as_f64().unwrap()),
        ("bool", Some(value)) => Value::Bool(value.as_bool().unwrap()),
        ("string", Some(value)) => Value::String(value.as_str().unwrap().into()),
        other => panic!("unknown fixture scalar {other:?}"),
    }
}

/// Decodes the explicit `repeated`/`group` profile-output notation.
fn tagged_instance(value: &serde_json::Value) -> Instance {
    Instance::Repeated(
        value["repeated"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                Instance::Group(
                    row["group"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|field| {
                            (
                                field[0].as_str().unwrap().to_owned(),
                                Instance::Scalar(tagged(&field[1])),
                            )
                        })
                        .collect::<Vec<_>>()
                        .into(),
                )
            })
            .collect(),
    )
}

fn bindings(values: &[(&str, Value)]) -> Vec<(String, Value)> {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn authored_bindings() -> Vec<(String, Value)> {
    bindings(&[
        ("minimum", Value::Int(10)),
        ("label", Value::String("alpha".into())),
    ])
}

/// The complete comparable error envelope.
type Envelope = (
    QueryPhase,
    QueryReadErrorKind,
    Option<String>,
    Option<String>,
    Option<u64>,
);

fn envelope(error: &QueryReadError) -> Envelope {
    (
        error.phase,
        error.kind.clone(),
        error.parameter.clone(),
        error.column.clone(),
        error.row,
    )
}

fn expect(phase: QueryPhase, kind: QueryReadErrorKind) -> Envelope {
    (phase, kind, None, None, None)
}

fn read(
    path: &Path,
    schema: &SchemaNode,
    plan: &ValidatedSqliteRead,
    values: &[(String, Value)],
) -> Result<Instance, QueryReadError> {
    read_query(path, schema, plan, values, &QueryReadControl::new())
}

/// No other connection holds a lock once acquisition has returned.
fn assert_unlocked(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE; ROLLBACK;").unwrap();
}

#[test]
fn authored_profile_matches_the_complete_original_instance() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let before = std::fs::read(&path).unwrap();
    let instance = read(
        &path,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
    )
    .unwrap();
    assert_eq!(instance, tagged_instance(&json("profile-output.json")));
    assert_unlocked(&path);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!scratch.path("orders.db-journal").exists());
}

/// Each authored SQL oracle, expressed as the equivalent closed descriptor.
#[test]
fn sql_oracles_match_their_closed_descriptors() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let order = |column: &str, ty: &str, direction: &str, nulls: &str| serde_json::json!({"column": column, "type": ty, "direction": direction, "nulls": nulls});
    let key = order("id", "int", "asc", "last");
    let parameter = |name: &str, ty: &str| serde_json::json!({"name": name, "type": ty, "runtime_parameter": name});
    let compare = |op: &str, column: &str, ty: &str, parameter: &str| serde_json::json!({"op": op, "column": column, "type": ty, "parameter": parameter});
    let descriptor = |projection: &[&str],
                      parameters: Vec<serde_json::Value>,
                      all: Vec<serde_json::Value>,
                      order_by: Vec<serde_json::Value>| {
        serde_json::json!({
            "kind": "sqlite_read_v1",
            "table": "orders",
            "key": "id",
            "projection": projection
                .iter()
                .map(|column| serde_json::json!({"field": column, "column": column}))
                .collect::<Vec<_>>(),
            "parameters": parameters,
            "all": all,
            "order_by": order_by,
        })
    };
    let all_columns = ["id", "label", "score", "active", "ratio"];
    type Case<'a> = (&'a str, serde_json::Value, Vec<(&'a str, Value)>);
    let cases: Vec<Case<'_>> = vec![
        (
            "stable_ties_and_nulls",
            descriptor(
                &all_columns,
                vec![],
                vec![],
                vec![order("score", "int", "desc", "last"), key.clone()],
            ),
            vec![],
        ),
        (
            "binary_comparison_and_duplicate_projection",
            descriptor(
                &["label", "score"],
                vec![parameter("minimum", "int"), parameter("label", "string")],
                vec![
                    compare("ge", "score", "int", "minimum"),
                    compare("eq", "label", "string", "label"),
                ],
                vec![order("score", "int", "desc", "last"), key.clone()],
            ),
            vec![
                ("minimum", Value::Int(10)),
                ("label", Value::String("alpha".into())),
            ],
        ),
        (
            "nulls_first",
            descriptor(
                &["id"],
                vec![],
                vec![],
                vec![order("score", "int", "asc", "first"), key.clone()],
            ),
            vec![],
        ),
        (
            "explicit_null_test",
            descriptor(
                &["id", "label"],
                vec![],
                vec![serde_json::json!({"op": "is_null", "column": "label", "type": "string"})],
                vec![key.clone()],
            ),
            vec![],
        ),
        (
            "parameter_is_data",
            descriptor(
                &["id"],
                vec![parameter("label", "string")],
                vec![compare("eq", "label", "string", "label")],
                vec![key.clone()],
            ),
            vec![("label", Value::String("x' OR 1=1 --".into()))],
        ),
        (
            "boolean_and_float_parameters",
            descriptor(
                &["id", "active", "ratio"],
                vec![parameter("active", "bool"), parameter("ratio", "float")],
                vec![
                    compare("eq", "active", "bool", "active"),
                    compare("ge", "ratio", "float", "ratio"),
                ],
                vec![key.clone()],
            ),
            vec![("active", Value::Bool(true)), ("ratio", Value::Float(2.0))],
        ),
        (
            "empty_result",
            descriptor(
                &["id"],
                vec![parameter("minimum", "int")],
                vec![compare("gt", "score", "int", "minimum")],
                vec![key],
            ),
            vec![("minimum", Value::Int(100))],
        ),
    ];
    let oracles = json("sql-cases.json");
    let oracles = oracles.as_array().unwrap();
    assert_eq!(oracles.len(), cases.len());
    for (id, profile, values) in cases {
        let oracle = oracles.iter().find(|case| case["id"] == id).unwrap();
        let columns: Vec<_> = profile["projection"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["column"].as_str().unwrap())
            .collect();
        let types: Vec<_> = oracle["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|ty| scalar_type(ty.as_str().unwrap()))
            .collect();
        let fields: Vec<_> = columns.iter().copied().zip(types.iter().copied()).collect();
        let schema = schema(&fields);
        let plan = plan(&schema, profile.clone());
        let expected = Instance::Repeated(
            oracle["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    Instance::Group(
                        columns
                            .iter()
                            .zip(row.as_array().unwrap())
                            .map(|(column, cell)| {
                                ((*column).to_owned(), Instance::Scalar(tagged(cell)))
                            })
                            .collect::<Vec<_>>()
                            .into(),
                    )
                })
                .collect(),
        );
        let actual = read(&path, &schema, &plan, &bindings(&values)).unwrap();
        assert_eq!(actual, expected, "{id}");
    }
}

#[test]
fn emitted_selection_quotes_identifiers_and_fixes_collation_and_nulls() {
    let plan = authored_plan();
    let admitted = admit::Admitted {
        table: "orders".into(),
        key: "id".into(),
        columns: plan
            .columns()
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
    };
    assert_eq!(
        execute::selection_sql(&plan, &admitted),
        "SELECT \"id\", \"label\", \"score\", \"active\", \"ratio\" FROM \"main\".\"orders\" \
         WHERE \"score\" >= :minimum AND \"label\" COLLATE BINARY = :label \
         ORDER BY \"score\" DESC NULLS LAST, \"id\" ASC NULLS LAST"
    );
}

#[test]
fn injection_shaped_identifiers_and_bindings_remain_literal_data() {
    let scratch = Scratch::new();
    let path = scratch.path("odd.db");
    scratch.database(
        &path,
        "CREATE TABLE \"we\"\"ird; DROP TABLE keep; --\" (\"i\"\"d\" INTEGER PRIMARY KEY, \"la\"\"bel\" TEXT);
         CREATE TABLE keep (x INTEGER);
         INSERT INTO \"we\"\"ird; DROP TABLE keep; --\" VALUES (2, 'a\"b'), (1, ''' OR 1=1; --');",
    );
    let schema = schema(&[("id", ScalarType::Int), ("label", ScalarType::String)]);
    let plan = plan(
        &schema,
        serde_json::json!({
            "kind": "sqlite_read_v1",
            "table": "WE\"IRD; drop table keep; --",
            "key": "I\"D",
            "projection": [
                {"field": "id", "column": "i\"d"},
                {"field": "label", "column": "la\"bel"}
            ],
            "parameters": [{"name": "label", "type": "string", "runtime_parameter": "label"}],
            "all": [{"op": "ne", "column": "la\"bel", "type": "string", "parameter": "label"}],
            "order_by": [{"column": "i\"d", "type": "int", "direction": "asc", "nulls": "last"}]
        }),
    );
    let instance = read(
        &path,
        &schema,
        &plan,
        &bindings(&[("label", Value::String("\" OR 1=1 --".into()))]),
    )
    .unwrap();
    let row = |id, label: &str| {
        Instance::Group(
            vec![
                ("id".to_owned(), Instance::Scalar(Value::Int(id))),
                (
                    "label".to_owned(),
                    Instance::Scalar(Value::String(label.into())),
                ),
            ]
            .into(),
        )
    };
    assert_eq!(
        instance,
        Instance::Repeated(vec![row(1, "' OR 1=1; --"), row(2, "a\"b")])
    );
    let conn = Connection::open(&path).unwrap();
    let kept: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'keep'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1);
}

#[test]
fn snapshot_reads_survive_a_concurrent_wal_writer() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let writer = Connection::open(&path).unwrap();
    let mode: String = writer
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    // An uncommitted write is invisible to the read snapshot.
    writer
        .execute_batch("BEGIN; INSERT INTO orders VALUES (60, 'alpha', 50, 1, 9.0);")
        .unwrap();
    let instance = read(
        &path,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
    )
    .unwrap();
    assert_eq!(instance, tagged_instance(&json("profile-output.json")));
    writer.execute_batch("COMMIT").unwrap();
    let after = read(
        &path,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
    )
    .unwrap();
    assert_eq!(after.as_repeated().map(<[Instance]>::len), Some(3));
}
