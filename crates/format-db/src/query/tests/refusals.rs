//! Binding, metadata, storage, resource, lock and cancellation refusals.

use super::*;

use QueryPhase as P;
use QueryReadErrorKind as K;

fn one_column(column_type: &str, field: ScalarType) -> (SchemaNode, ValidatedSqliteRead, String) {
    let schema = schema(&[("id", ScalarType::Int), ("value", field)]);
    let plan = plan(
        &schema,
        serde_json::json!({
            "kind": "sqlite_read_v1", "table": "t", "key": "id",
            "projection": [{"field": "id", "column": "id"}, {"field": "value", "column": "value"}],
            "parameters": [], "all": [],
            "order_by": [{"column": "id", "type": "int", "direction": "asc", "nulls": "last"}]
        }),
    );
    (
        schema,
        plan,
        format!("CREATE TABLE t (id INTEGER PRIMARY KEY, value {column_type});"),
    )
}

#[test]
fn binding_preflight_precedes_any_database_access() {
    let scratch = Scratch::new();
    let absent = scratch.path("absent.db");
    let authored = authored_plan();
    let orders = orders_schema();
    let cases: Vec<(Vec<(&str, Value)>, Envelope)> = vec![
        (
            vec![
                ("minimum", Value::Float(f64::NAN)),
                ("label", Value::String("a".into())),
            ],
            expect(
                P::Bind,
                K::ParameterType {
                    expected: ScalarType::Int,
                    found: "float",
                },
            ),
        ),
        (
            vec![("minimum", Value::Int(1)), ("label", Value::xml_nil())],
            expect(P::Bind, K::NullParameter),
        ),
        (
            vec![("minimum", Value::Int(1)), ("label", Value::Bool(true))],
            expect(
                P::Bind,
                K::ParameterType {
                    expected: ScalarType::String,
                    found: "bool",
                },
            ),
        ),
    ];
    for (values, (phase, kind, ..)) in cases {
        let error = read(&absent, &orders, &authored, &bindings(&values)).unwrap_err();
        assert_eq!((error.phase, error.kind.clone()), (phase, kind));
        assert!(error.parameter.is_some());
    }
    let float_schema = schema(&[("ratio", ScalarType::Float), ("id", ScalarType::Int)]);
    let float_plan = plan(
        &float_schema,
        serde_json::json!({
            "kind": "sqlite_read_v1", "table": "orders", "key": "id",
            "projection": [{"field": "ratio", "column": "ratio"}, {"field": "id", "column": "id"}],
            "parameters": [{"name": "r", "type": "float", "runtime_parameter": "r"}],
            "all": [{"op": "lt", "column": "ratio", "type": "float", "parameter": "r"}],
            "order_by": [{"column": "id", "type": "int", "direction": "asc", "nulls": "last"}]
        }),
    );
    let error = read(
        &absent,
        &float_schema,
        &float_plan,
        &bindings(&[("r", Value::Float(f64::INFINITY))]),
    )
    .unwrap_err();
    assert_eq!(
        envelope(&error),
        (P::Bind, K::NonFiniteParameter, Some("r".into()), None, None)
    );
    let error = read(&absent, &float_schema, &authored, &authored_bindings()).unwrap_err();
    assert_eq!(envelope(&error), expect(P::Bind, K::PlanSchemaMismatch));
    // Complete bindings reach the open step, which never creates the file.
    let error = read(&absent, &orders, &authored, &authored_bindings()).unwrap_err();
    assert_eq!((error.phase, error.kind.clone()), (P::Open, K::Open));
    assert_eq!(
        error.sqlite.map(|codes| codes.primary),
        Some(rusqlite::ffi::SQLITE_CANTOPEN)
    );
    assert!(!absent.exists());
    // A `file:` spelling is a literal filename, not a URI.
    let uri = Path::new("file:absent.db?mode=rwc");
    assert!(read(uri, &orders, &authored, &authored_bindings()).is_err());
    assert!(!Path::new("absent.db").exists());
}

#[test]
fn cancellation_wins_over_the_deadline_before_open() {
    let scratch = Scratch::new();
    let absent = scratch.path("absent.db");
    let cancelled = AtomicBool::new(true);
    let expired = QueryReadLimits::MAXIMUM.with_deadline(Duration::ZERO);
    let both = QueryReadControl::new()
        .with_cancellation(&cancelled)
        .with_limits(expired);
    let error = read_query(
        &absent,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
        &both,
    )
    .unwrap_err();
    assert_eq!(envelope(&error), expect(P::Open, K::Cancelled));
    let error = read_query(
        &absent,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
        &QueryReadControl::new().with_limits(expired),
    )
    .unwrap_err();
    assert_eq!(envelope(&error), expect(P::Open, K::DeadlineExceeded));
    // Limits can only be lowered.
    assert_eq!(
        QueryReadLimits::MAXIMUM.with_deadline(Duration::from_secs(60)),
        QueryReadLimits::MAXIMUM
    );
}

#[test]
fn forbidden_tables_and_keys_refuse_in_metadata_order() {
    let scratch = Scratch::new();
    let cases: Vec<(&str, &str, &str, Envelope)> = vec![
        (
            "missing",
            "",
            "id",
            (
                P::Metadata,
                K::NoSuchTable,
                None,
                Some("missing".into()),
                None,
            ),
        ),
        (
            "v",
            "CREATE TABLE base (id INTEGER PRIMARY KEY, value TEXT); CREATE VIEW v AS SELECT * FROM base;",
            "id",
            (
                P::Metadata,
                K::ForbiddenTable(ForbiddenTable::View),
                None,
                Some("v".into()),
                None,
            ),
        ),
        (
            "fts",
            "CREATE VIRTUAL TABLE fts USING fts5(id, value);",
            "id",
            (
                P::Metadata,
                K::ForbiddenTable(ForbiddenTable::Virtual),
                None,
                Some("fts".into()),
                None,
            ),
        ),
        (
            "fts_data",
            "CREATE VIRTUAL TABLE fts USING fts5(value);",
            "id",
            (
                P::Metadata,
                K::ForbiddenTable(ForbiddenTable::Shadow),
                None,
                Some("fts_data".into()),
                None,
            ),
        ),
        (
            "sqlite_sequence",
            "CREATE TABLE a (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT); INSERT INTO a VALUES (NULL, 'x');",
            "id",
            (
                P::Metadata,
                K::ForbiddenTable(ForbiddenTable::Internal),
                None,
                Some("sqlite_sequence".into()),
                None,
            ),
        ),
        (
            "w",
            "CREATE TABLE w (id INTEGER PRIMARY KEY, value TEXT) WITHOUT ROWID;",
            "id",
            (
                P::Metadata,
                K::ForbiddenTable(ForbiddenTable::WithoutRowid),
                None,
                Some("w".into()),
                None,
            ),
        ),
        (
            "c",
            "CREATE TABLE c (id INTEGER, value TEXT, PRIMARY KEY (id, value));",
            "id",
            (
                P::Metadata,
                K::InvalidKey(KeyProblem::CompositePrimaryKey),
                None,
                Some("id".into()),
                None,
            ),
        ),
        (
            "n",
            "CREATE TABLE n (id INTEGER, value TEXT);",
            "id",
            (
                P::Metadata,
                K::InvalidKey(KeyProblem::NotPrimaryKey),
                None,
                Some("id".into()),
                None,
            ),
        ),
        (
            "i",
            "CREATE TABLE i (id INT PRIMARY KEY, value TEXT);",
            "id",
            (
                P::Metadata,
                K::InvalidKey(KeyProblem::NotRowidAlias),
                None,
                Some("id".into()),
                None,
            ),
        ),
        (
            "d",
            "CREATE TABLE d (id INTEGER PRIMARY KEY DESC, value TEXT);",
            "id",
            (
                P::Metadata,
                K::InvalidKey(KeyProblem::NotRowidAlias),
                None,
                Some("id".into()),
                None,
            ),
        ),
        (
            "k",
            "CREATE TABLE k (other INTEGER PRIMARY KEY, value TEXT);",
            "id",
            (P::Metadata, K::MissingColumn, None, Some("id".into()), None),
        ),
        (
            "g",
            "CREATE TABLE g (id INTEGER PRIMARY KEY, raw TEXT, value TEXT GENERATED ALWAYS AS (upper(raw)));",
            "id",
            (
                P::Metadata,
                K::GeneratedColumn,
                None,
                Some("value".into()),
                None,
            ),
        ),
        (
            "m",
            "CREATE TABLE m (id INTEGER PRIMARY KEY, other TEXT);",
            "id",
            (
                P::Metadata,
                K::MissingColumn,
                None,
                Some("value".into()),
                None,
            ),
        ),
    ];
    for (index, (table, sql, key, expected)) in cases.into_iter().enumerate() {
        let path = scratch.path(&format!("tables-{index}.db"));
        scratch.database(
            &path,
            if sql.is_empty() {
                "CREATE TABLE unrelated (x);"
            } else {
                sql
            },
        );
        let schema = schema(&[("id", ScalarType::Int), ("value", ScalarType::String)]);
        let plan = plan(
            &schema,
            serde_json::json!({
                "kind": "sqlite_read_v1", "table": table, "key": key,
                "projection": [{"field": "id", "column": key}, {"field": "value", "column": "value"}],
                "parameters": [], "all": [],
                "order_by": [{"column": key, "type": "int", "direction": "asc", "nulls": "last"}]
            }),
        );
        let error = read(&path, &schema, &plan, &[]).unwrap_err();
        assert_eq!(envelope(&error), expected, "{table}");
        assert_unlocked(&path);
    }
}

#[test]
fn every_scalar_type_requires_its_single_compatible_affinity() {
    let scratch = Scratch::new();
    let cases = [
        ("TEXT", ScalarType::String, true),
        ("VARCHAR(10)", ScalarType::String, true),
        ("", ScalarType::String, false),
        ("BLOB", ScalarType::String, false),
        ("INTEGER", ScalarType::Int, true),
        ("NUMERIC", ScalarType::Int, false),
        ("INTEGER", ScalarType::Bool, true),
        ("BOOLEAN", ScalarType::Bool, false),
        ("REAL", ScalarType::Float, true),
        ("INTEGER", ScalarType::Float, true),
        ("DECIMAL(10,2)", ScalarType::Float, false),
        ("TEXT", ScalarType::Int, false),
    ];
    for (index, (declared, ty, admitted)) in cases.into_iter().enumerate() {
        let (schema, plan, sql) = one_column(declared, ty);
        let path = scratch.path(&format!("affinity-{index}.db"));
        scratch.database(&path, &sql);
        let result = read(&path, &schema, &plan, &[]);
        if admitted {
            assert_eq!(result.unwrap(), Instance::Repeated(vec![]), "{declared}");
        } else {
            let error = result.unwrap_err();
            assert!(
                matches!(&error.kind, K::ColumnAffinity { expected, declared: found, .. } if *expected == ty && found == declared),
                "{declared}: {error}"
            );
            assert_eq!(error.column.as_deref(), Some("value"));
        }
    }
}

#[test]
fn malformed_candidate_cells_refuse_even_when_a_filter_would_hide_them() {
    let scratch = Scratch::new();
    let cases = [
        (
            "UPDATE orders SET score = 'abc' WHERE id = 40",
            "score",
            4,
            ScalarType::Int,
            "text",
        ),
        (
            "UPDATE orders SET active = 2 WHERE id = 50",
            "active",
            5,
            ScalarType::Bool,
            "integer other than 0 or 1",
        ),
        (
            "UPDATE orders SET ratio = 'x' WHERE id = 10",
            "ratio",
            1,
            ScalarType::Float,
            "text",
        ),
        (
            "UPDATE orders SET label = x'00ff' WHERE id = 20",
            "label",
            2,
            ScalarType::String,
            "blob",
        ),
    ];
    for (index, (corruption, column, row, expected, found)) in cases.into_iter().enumerate() {
        let path = scratch.path(&format!("malformed-{index}.db"));
        scratch.database(
            &path,
            &format!(
                "{}\nPRAGMA ignore_check_constraints = ON;\n{corruption};",
                fixture("seed.sql")
            ),
        );
        // The authored filter selects only rows 10 and 30.
        let error = read(
            &path,
            &orders_schema(),
            &authored_plan(),
            &authored_bindings(),
        )
        .unwrap_err();
        assert_eq!(
            envelope(&error),
            (
                P::Candidates,
                K::CellType { expected, found },
                None,
                Some(column.into()),
                Some(row)
            ),
            "{corruption}"
        );
    }
    // Float accepts INTEGER storage only when it is exactly representable.
    let (schema, plan, sql) = one_column("INTEGER", ScalarType::Float);
    let path = scratch.path("exact.db");
    scratch.database(
        &path,
        &format!("{sql} INSERT INTO t VALUES (1, 9007199254740994);"),
    );
    let instance = read(&path, &schema, &plan, &[]).unwrap();
    assert_eq!(
        instance,
        Instance::Repeated(vec![Instance::Group(
            vec![
                ("id".to_owned(), Instance::Scalar(Value::Int(1))),
                (
                    "value".to_owned(),
                    Instance::Scalar(Value::Float(9_007_199_254_740_994.0))
                ),
            ]
            .into()
        )])
    );
    let path = scratch.path("inexact.db");
    scratch.database(
        &path,
        &format!("{sql} INSERT INTO t VALUES (1, 9007199254740993);"),
    );
    let error = read(&path, &schema, &plan, &[]).unwrap_err();
    assert_eq!(
        envelope(&error),
        (
            P::Candidates,
            K::CellType {
                expected: ScalarType::Float,
                found: "integer outside the exact f64 range"
            },
            None,
            Some("value".into()),
            Some(1)
        )
    );
}

#[test]
fn resource_limits_refuse_instead_of_truncating() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let attempt = |limits: QueryReadLimits| {
        let error = read_query(
            &path,
            &orders_schema(),
            &authored_plan(),
            &authored_bindings(),
            &QueryReadControl::new().with_limits(limits),
        )
        .unwrap_err();
        envelope(&error)
    };
    let maximum = QueryReadLimits::MAXIMUM;
    assert_eq!(
        attempt(maximum.with_candidate_rows(4)),
        expect(P::Candidates, K::LimitExceeded(QueryLimit::CandidateRows))
    );
    assert_eq!(
        attempt(maximum.with_output_rows(1)),
        expect(P::Select, K::LimitExceeded(QueryLimit::OutputRows))
    );
    assert_eq!(
        attempt(maximum.with_cell_bytes(8)),
        (
            P::Candidates,
            K::LimitExceeded(QueryLimit::CellBytes),
            None,
            Some("label".into()),
            Some(5)
        )
    );
    // Validation charges every referenced cell (numeric cells as 8 bytes,
    // NULL as 0): three 37-byte rows, the 8-byte NULL row and a 44-byte row.
    // Output then charges its projected cells again.
    let validation_bytes = 37 * 3 + 8 + 44;
    assert_eq!(
        attempt(maximum.with_payload_bytes(validation_bytes)),
        (
            P::Select,
            K::LimitExceeded(QueryLimit::PayloadBytes),
            None,
            Some("id".into()),
            Some(1)
        )
    );
    assert_eq!(
        attempt(maximum.with_payload_bytes(10)),
        (
            P::Candidates,
            K::LimitExceeded(QueryLimit::PayloadBytes),
            None,
            Some("label".into()),
            Some(1)
        )
    );
    assert_eq!(
        attempt(maximum.with_sql_bytes(40)),
        expect(P::Candidates, K::LimitExceeded(QueryLimit::SqlBytes))
    );
    assert_unlocked(&path);

    // Progress callbacks count VM instructions across one statement's steps.
    let large = scratch.path("work.db");
    scratch.database(&large, LARGE);
    let error = read_query(
        &large,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
        &QueryReadControl::new().with_limits(maximum.with_vm_work(1)),
    )
    .unwrap_err();
    assert_eq!(error.kind, K::LimitExceeded(QueryLimit::VmWork), "{error}");
    assert_eq!(
        error.sqlite.map(|codes| codes.primary),
        Some(rusqlite::ffi::SQLITE_INTERRUPT)
    );
    assert!(error.cleanup.is_empty());
    assert_unlocked(&large);
}

const LARGE: &str = "CREATE TABLE orders (id INTEGER PRIMARY KEY, label TEXT, score INTEGER, active INTEGER, ratio REAL);
WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 90000)
INSERT INTO orders SELECT i, 'alpha', (i * 7919) % 1000, i % 2, i * 0.5 FROM n;";

#[test]
fn a_locked_database_is_a_typed_refusal_without_waiting() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("BEGIN EXCLUSIVE; INSERT INTO orders VALUES (70, 'z', 1, 0, 1.0);")
        .unwrap();
    let started = std::time::Instant::now();
    let error = read(
        &path,
        &orders_schema(),
        &authored_plan(),
        &authored_bindings(),
    )
    .unwrap_err();
    assert_eq!((error.phase, error.kind.clone()), (P::Metadata, K::Busy));
    assert_eq!(
        error.sqlite.map(|codes| codes.primary),
        Some(rusqlite::ffi::SQLITE_BUSY)
    );
    assert!(error.cleanup.is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    writer.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn live_cancellation_interrupts_work_and_joins_before_close() {
    let scratch = Scratch::new();
    let path = scratch.path("large.db");
    scratch.database(&path, LARGE);
    let schema = orders_schema();
    let plan = authored_plan();
    let cancellation = AtomicBool::new(false);
    let control = QueryReadControl::new().with_cancellation(&cancellation);
    let error = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(5));
            cancellation.store(true, Ordering::SeqCst);
        });
        read_query(&path, &schema, &plan, &authored_bindings(), &control).unwrap_err()
    });
    assert_eq!(error.kind, K::Cancelled, "{error}");
    // The phase depends on scheduling; the refusal and cleanup do not.
    assert_ne!(error.phase, P::Cleanup);
    assert!(error.cleanup.is_empty());
    assert_unlocked(&path);
}

#[test]
fn the_authorizer_denies_everything_outside_the_admitted_columns() {
    let scratch = Scratch::new();
    let path = scratch.seeded();
    let conn = Connection::open(&path).unwrap();
    execute::install_authorizer(
        &conn,
        &admit::Admitted {
            table: "orders".into(),
            key: "id".into(),
            columns: vec!["id".into(), "label".into()],
        },
    );
    assert!(
        conn.prepare("SELECT \"id\", \"label\" FROM \"main\".\"orders\"")
            .is_ok()
    );
    for denied in [
        "SELECT score FROM orders",
        "SELECT abs(id) FROM orders",
        "SELECT name FROM sqlite_schema",
        "INSERT INTO orders (id) VALUES (99)",
        "PRAGMA table_info(orders)",
        "ATTACH ':memory:' AS other",
    ] {
        let error = conn.prepare(denied).err().unwrap();
        let classified = QueryReadError::sqlite(&path, P::Select, error);
        assert_eq!(classified.kind, K::Unauthorized, "{denied}");
    }
}
