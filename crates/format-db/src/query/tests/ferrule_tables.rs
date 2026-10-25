//! Tables created by Ferrule's writer and introspected by `import-db`.

use ir::ValueGeneration;

use super::*;
use crate::{introspect, write};

use QueryPhase as P;
use QueryReadErrorKind as K;

/// `id` is generated unless `generated` names other columns.
fn written_schema(generated: &[&str]) -> SchemaNode {
    let fields = [
        ("id", ScalarType::Int),
        ("label", ScalarType::String),
        ("rank", ScalarType::Int),
        ("active", ScalarType::Bool),
        ("ratio", ScalarType::Float),
    ];
    SchemaNode::group(
        "orders",
        fields
            .into_iter()
            .map(|(name, ty)| {
                let field = SchemaNode::scalar(name, ty);
                if generated.contains(&name) {
                    field
                        .with_value_generation(ValueGeneration::MaxNumber)
                        .unwrap()
                } else {
                    field
                }
            })
            .collect(),
    )
    .repeating()
}

fn row(fields: &[(&str, Value)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Instance::Scalar(value.clone())))
            .collect::<Vec<_>>()
            .into(),
    )
}

/// Rows the writer accepts; `generated` columns are left for it to fill.
fn rows(generated: &[&str]) -> Vec<Instance> {
    [
        ("alpha", 1, Value::Bool(true), Value::Float(1.5)),
        ("beta", 2, Value::Bool(false), Value::Float(2.0)),
        ("gamma", 3, Value::Bool(true), Value::Null),
        ("delta", 4, Value::Null, Value::Float(4.5)),
    ]
    .into_iter()
    .map(|(label, rank, active, ratio)| {
        let fields = [
            ("id", Value::Int(rank)),
            ("label", Value::String(label.into())),
            ("rank", Value::Int(rank)),
            ("active", active),
            ("ratio", ratio),
        ];
        let supplied: Vec<_> = fields
            .into_iter()
            .filter(|(name, _)| !generated.contains(name))
            .collect();
        row(&supplied)
    })
    .collect()
}

fn active_profile(key: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "sqlite_read_v1", "table": "orders", "key": key,
        "projection": [
            {"field": "id", "column": "id"}, {"field": "label", "column": "label"},
            {"field": "rank", "column": "rank"}, {"field": "active", "column": "active"},
            {"field": "ratio", "column": "ratio"}
        ],
        "parameters": [{"name": "active", "type": "bool", "runtime_parameter": "active"}],
        "all": [{"op": "eq", "column": "active", "type": "bool", "parameter": "active"}],
        "order_by": [
            {"column": "ratio", "type": "float", "direction": "desc", "nulls": "first"},
            {"column": key, "type": "int", "direction": "asc", "nulls": "last"}
        ]
    })
}

fn written(scratch: &Scratch, name: &str, generated: &[&str]) -> PathBuf {
    let path = scratch.path(name);
    write(&path, &written_schema(generated), &rows(generated)).unwrap();
    path
}

fn declared_sql(path: &Path) -> String {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'orders'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_written_and_introspected_table_carries_a_query_without_edits() {
    let scratch = Scratch::new();
    let path = written(&scratch, "written.db", &["id"]);
    assert_eq!(
        declared_sql(&path),
        "CREATE TABLE \"orders\" (\"id\" INTEGER PRIMARY KEY, \"label\" TEXT, \
         \"rank\" INTEGER, \"active\" BOOLEAN, \"ratio\" REAL)"
    );
    // `import-db` recovers the written schema, generated-key marker included.
    let schema = introspect(&path, "orders").unwrap();
    assert_eq!(schema, written_schema(&["id"]));

    let plan = plan(&schema, active_profile("id"));
    let instance = read(
        &path,
        &schema,
        &plan,
        &bindings(&[("active", Value::Bool(true))]),
    );
    assert_eq!(
        instance.unwrap(),
        Instance::Repeated(vec![
            row(&[
                ("id", Value::Int(3)),
                ("label", Value::String("gamma".into())),
                ("rank", Value::Int(3)),
                ("active", Value::Bool(true)),
                ("ratio", Value::Null),
            ]),
            row(&[
                ("id", Value::Int(1)),
                ("label", Value::String("alpha".into())),
                ("rank", Value::Int(1)),
                ("active", Value::Bool(true)),
                ("ratio", Value::Float(1.5)),
            ]),
        ])
    );
    let instance = read(
        &path,
        &schema,
        &plan,
        &bindings(&[("active", Value::Bool(false))]),
    );
    assert_eq!(
        instance.unwrap(),
        Instance::Repeated(vec![row(&[
            ("id", Value::Int(2)),
            ("label", Value::String("beta".into())),
            ("rank", Value::Int(2)),
            ("active", Value::Bool(false)),
            ("ratio", Value::Float(2.0)),
        ])])
    );
    // A rewrite keeps the created declaration and the same generated keys.
    write(&path, &schema, &rows(&["id"])).unwrap();
    assert!(declared_sql(&path).contains("\"id\" INTEGER PRIMARY KEY"));
    assert_unlocked(&path);
}

#[test]
fn written_tables_without_a_sole_generated_key_still_refuse() {
    let scratch = Scratch::new();
    let not_primary = (
        P::Metadata,
        K::InvalidKey(KeyProblem::NotPrimaryKey),
        None,
        Some("id".into()),
        None,
    );
    let plain = schema(&[
        ("id", ScalarType::Int),
        ("label", ScalarType::String),
        ("rank", ScalarType::Int),
        ("active", ScalarType::Bool),
        ("ratio", ScalarType::Float),
    ]);
    let active = bindings(&[("active", Value::Bool(true))]);

    // No generated column: the writer declares no key and none is guessed.
    let keyless = scratch.path("keyless.db");
    write(&keyless, &plain, &rows(&[])).unwrap();
    assert!(!declared_sql(&keyless).contains("PRIMARY KEY"));
    let schema = introspect(&keyless, "orders").unwrap();
    assert_eq!(schema, plain);
    let error = read(
        &keyless,
        &schema,
        &plan(&schema, active_profile("id")),
        &active,
    );
    assert_eq!(envelope(&error.unwrap_err()), not_primary);

    // Two generated columns: neither becomes the rowid alias.
    let both = written(&scratch, "both.db", &["id", "rank"]);
    assert!(!declared_sql(&both).contains("PRIMARY KEY"));
    let introspected = introspect(&both, "orders").unwrap();
    assert_eq!(introspected, plain);
    let error = read(&both, &plain, &plan(&plain, active_profile("id")), &active);
    assert_eq!(envelope(&error.unwrap_err()), not_primary);

    // An existing table keeps its declaration; the writer never adds a key.
    let existing = scratch.path("existing.db");
    scratch.database(
        &existing,
        "CREATE TABLE orders (id INTEGER, label TEXT, rank INTEGER, active BOOLEAN, ratio REAL);",
    );
    write(&existing, &written_schema(&["id"]), &rows(&["id"])).unwrap();
    let schema = introspect(&existing, "orders").unwrap();
    assert_eq!(schema, plain);
    let error = read(
        &existing,
        &schema,
        &plan(&schema, active_profile("id")),
        &active,
    );
    assert_eq!(envelope(&error.unwrap_err()), not_primary);
}

#[test]
fn boolean_columns_still_require_integer_zero_or_one_storage() {
    let scratch = Scratch::new();
    let schema = written_schema(&["id"]);
    let plan = plan(&schema, active_profile("id"));
    for (index, (corruption, found)) in [
        ("2", "integer other than 0 or 1"),
        ("'true'", "text"),
        ("0.5", "real"),
        ("x'01'", "blob"),
    ]
    .into_iter()
    .enumerate()
    {
        let path = written(&scratch, &format!("corrupt-{index}.db"), &["id"]);
        // Row 4 has a NULL Boolean, and the filter selects only true rows.
        scratch.database(
            &path,
            &format!("UPDATE orders SET active = {corruption} WHERE id = 4;"),
        );
        let error = read(
            &path,
            &schema,
            &plan,
            &bindings(&[("active", Value::Bool(true))]),
        );
        assert_eq!(
            envelope(&error.unwrap_err()),
            (
                P::Candidates,
                K::CellType {
                    expected: ScalarType::Bool,
                    found
                },
                None,
                Some("active".into()),
                Some(4)
            ),
            "{corruption}"
        );
        assert_unlocked(&path);
    }
}
