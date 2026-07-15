use ir::{Instance, ScalarType, SchemaNode, Value};
use rusqlite::Connection;

use super::{
    DbFormatError, ForeignKeySide, read_instance, resolve_foreign_key_columns,
    resolve_foreign_key_relation, write_instance,
};

fn test_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ferrule_format_db_relational_{name}_{}.db",
        std::process::id()
    ))
}

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn table(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children).repeating()
}

fn field<'a>(instance: &'a Instance, name: &str) -> &'a Instance {
    instance
        .field(name)
        .unwrap_or_else(|| panic!("missing field {name}"))
}

fn scalar_value<'a>(instance: &'a Instance, name: &str) -> &'a Value {
    field(instance, name)
        .as_scalar()
        .unwrap_or_else(|| panic!("field {name} was not scalar"))
}

fn value(name: &str, value: Value) -> (String, Instance) {
    (name.to_string(), Instance::Scalar(value))
}

#[test]
fn writes_child_owned_relations_with_generated_keys_idempotently() {
    let path = test_path("write_children");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL, value TEXT, \
             FOREIGN KEY(parent_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "parents",
        vec![
            scalar("id", ScalarType::Int),
            scalar("name", ScalarType::String),
            table(
                "children|parent_id",
                vec![
                    scalar("id", ScalarType::Int),
                    scalar("parent_id", ScalarType::Int),
                    scalar("value", ScalarType::String),
                ],
            ),
        ],
    );
    let row = Instance::Group(vec![
        value("id", Value::Null),
        value("name", Value::String("Parent".into())),
        (
            "children|parent_id".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    value("id", Value::Null),
                    value("parent_id", Value::Null),
                    value("value", Value::String("First".into())),
                ]),
                Instance::Group(vec![
                    value("id", Value::Null),
                    value("parent_id", Value::Null),
                    value("value", Value::String("Second".into())),
                ]),
            ]),
        ),
    ]);
    let instance = Instance::Repeated(vec![row]);

    write_instance(&path, &schema, &instance).unwrap();
    write_instance(&path, &schema, &instance).unwrap();
    let result = read_instance(&path, &schema).unwrap();
    let parents = result.as_repeated().unwrap();
    let children = field(&parents[0], "children|parent_id")
        .as_repeated()
        .unwrap();
    let parent_id = scalar_value(&parents[0], "id");
    std::fs::remove_file(&path).unwrap();

    assert_eq!(parents.len(), 1);
    assert_eq!(children.len(), 2);
    assert_ne!(parent_id, &Value::Null);
    assert_eq!(scalar_value(&children[0], "parent_id"), parent_id);
    assert_eq!(scalar_value(&children[1], "parent_id"), parent_id);
}

#[test]
fn writes_parent_owned_relations_before_the_referencing_row() {
    let path = test_path("write_reference");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE groups (id INTEGER PRIMARY KEY, name TEXT NOT NULL); \
         CREATE TABLE users (id INTEGER PRIMARY KEY, group_id INTEGER, name TEXT NOT NULL, \
             FOREIGN KEY(group_id) REFERENCES groups(id));",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "users",
        vec![
            scalar("id", ScalarType::Int),
            scalar("group_id", ScalarType::Int),
            scalar("name", ScalarType::String),
            table(
                "groups|group_id",
                vec![
                    scalar("id", ScalarType::Int),
                    scalar("name", ScalarType::String),
                ],
            ),
        ],
    );
    let instance = Instance::Repeated(vec![Instance::Group(vec![
        value("id", Value::Null),
        value("group_id", Value::Null),
        value("name", Value::String("User".into())),
        (
            "groups|group_id".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                value("id", Value::Null),
                value("name", Value::String("Group".into())),
            ])]),
        ),
    ])]);

    write_instance(&path, &schema, &instance).unwrap();
    let result = read_instance(&path, &schema).unwrap();
    let users = result.as_repeated().unwrap();
    let groups = field(&users[0], "groups|group_id").as_repeated().unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(groups.len(), 1);
    assert_eq!(
        scalar_value(&users[0], "group_id"),
        scalar_value(&groups[0], "id")
    );
}

#[test]
fn reads_independent_tables_under_a_composite_root() {
    let path = test_path("composite");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE departments (id INTEGER, name TEXT); \
         CREATE TABLE offices (id INTEGER, city TEXT); \
         INSERT INTO departments VALUES (2, 'Engineering'), (1, 'Sales'); \
         INSERT INTO offices VALUES (1, 'Seattle');",
    )
    .unwrap();
    drop(conn);
    let schema = SchemaNode::group(
        "database",
        vec![
            table(
                "departments",
                vec![
                    scalar("id", ScalarType::Int),
                    scalar("name", ScalarType::String),
                ],
            ),
            table(
                "offices",
                vec![
                    scalar("id", ScalarType::Int),
                    scalar("city", ScalarType::String),
                ],
            ),
        ],
    );

    let instance = read_instance(&path, &schema).unwrap();
    let departments = field(&instance, "departments").as_repeated().unwrap();
    let offices = field(&instance, "offices").as_repeated().unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(departments.len(), 2);
    assert_eq!(
        scalar_value(&departments[0], "name"),
        &Value::String("Engineering".into())
    );
    assert_eq!(offices.len(), 1);
    assert_eq!(
        scalar_value(&offices[0], "city"),
        &Value::String("Seattle".into())
    );
}

#[test]
fn reads_child_rows_that_reference_the_parent() {
    let path = test_path("children");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE departments (id INTEGER PRIMARY KEY, name TEXT); \
         CREATE TABLE people (id INTEGER PRIMARY KEY, department_id INTEGER, name TEXT, \
             FOREIGN KEY(department_id) REFERENCES departments(id)); \
         INSERT INTO departments VALUES (1, 'Engineering'), (2, 'Sales'); \
         INSERT INTO people VALUES (1, 1, 'Ada'), (2, 1, 'Grace'), (3, 2, 'Linus');",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "departments",
        vec![table(
            "people|department_id",
            vec![scalar("name", ScalarType::String)],
        )],
    );

    let instance = read_instance(&path, &schema).unwrap();
    let departments = instance.as_repeated().unwrap();
    let engineering = field(&departments[0], "people|department_id")
        .as_repeated()
        .unwrap();
    let sales = field(&departments[1], "people|department_id")
        .as_repeated()
        .unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(engineering.len(), 2);
    assert_eq!(
        scalar_value(&engineering[0], "name"),
        &Value::String("Ada".into())
    );
    assert_eq!(sales.len(), 1);
    assert_eq!(
        scalar_value(&sales[0], "name"),
        &Value::String("Linus".into())
    );
}

#[test]
fn reads_the_row_referenced_by_its_parent() {
    let path = test_path("reference");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE groups (id INTEGER PRIMARY KEY, name TEXT); \
         CREATE TABLE users (id INTEGER PRIMARY KEY, group_id INTEGER, name TEXT, \
             FOREIGN KEY(group_id) REFERENCES groups(id)); \
         INSERT INTO groups VALUES (10, 'Admin'), (20, 'Reader'); \
         INSERT INTO users VALUES (1, 10, 'Alice'), (2, 20, 'Bob'), (3, NULL, 'Eve');",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "users",
        vec![
            scalar("name", ScalarType::String),
            table("groups|group_id", vec![scalar("name", ScalarType::String)]),
        ],
    );

    let instance = read_instance(&path, &schema).unwrap();
    let users = instance.as_repeated().unwrap();
    let alice_group = field(&users[0], "groups|group_id").as_repeated().unwrap();
    let bob_group = field(&users[1], "groups|group_id").as_repeated().unwrap();
    let no_group = field(&users[2], "groups|group_id").as_repeated().unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(alice_group.len(), 1);
    assert_eq!(
        scalar_value(&alice_group[0], "name"),
        &Value::String("Admin".into())
    );
    assert_eq!(
        scalar_value(&bob_group[0], "name"),
        &Value::String("Reader".into())
    );
    assert!(no_group.is_empty());
}

#[test]
fn rejects_a_relation_without_matching_foreign_key_metadata() {
    let path = test_path("missing");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parents (id INTEGER PRIMARY KEY); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER);",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "parents",
        vec![
            scalar("id", ScalarType::Int),
            table("children|parent_id", vec![scalar("id", ScalarType::Int)]),
        ],
    );

    let error = read_instance(&path, &schema).unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        error,
        DbFormatError::MissingForeignKeyRelation {
            parent_table,
            child_table,
            join_column,
        } if parent_table == "parents" && child_table == "children" && join_column == "parent_id"
    ));
}

#[test]
fn rejects_a_relation_that_matches_both_directions() {
    let path = test_path("ambiguous");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY, link_id INTEGER, \
             FOREIGN KEY(link_id) REFERENCES children(id)); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, link_id INTEGER, \
             FOREIGN KEY(link_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);
    let schema = table(
        "parents",
        vec![
            scalar("id", ScalarType::Int),
            table("children|link_id", vec![scalar("id", ScalarType::Int)]),
        ],
    );

    let error = read_instance(&path, &schema).unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        error,
        DbFormatError::AmbiguousForeignKeyRelation {
            parent_table,
            child_table,
            join_column,
        } if parent_table == "parents" && child_table == "children" && join_column == "link_id"
    ));
}

#[test]
fn resolves_columns_when_the_child_owns_the_foreign_key() {
    let path = test_path("child_owned_columns");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER, \
             FOREIGN KEY(parent_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);

    let columns = resolve_foreign_key_columns(&path, "parents", "children", "parent_id").unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(columns.parent_column, "id");
    assert_eq!(columns.child_column, "parent_id");
}

#[test]
fn resolves_columns_when_the_parent_owns_the_foreign_key() {
    let path = test_path("parent_owned_columns");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE groups (id INTEGER PRIMARY KEY); \
         CREATE TABLE users (id INTEGER PRIMARY KEY, group_id INTEGER, \
             FOREIGN KEY(group_id) REFERENCES groups(id));",
    )
    .unwrap();
    drop(conn);

    let columns = resolve_foreign_key_columns(&path, "users", "groups", "group_id").unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(columns.parent_column, "group_id");
    assert_eq!(columns.child_column, "id");
}

#[test]
fn column_resolution_rejects_a_missing_relation() {
    let path = test_path("missing_columns");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parents (id INTEGER PRIMARY KEY); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER);",
    )
    .unwrap();
    drop(conn);

    let error = resolve_foreign_key_columns(&path, "parents", "children", "parent_id").unwrap_err();
    std::fs::remove_file(&path).unwrap();

    assert!(matches!(
        error,
        DbFormatError::MissingForeignKeyRelation {
            parent_table,
            child_table,
            join_column,
        } if parent_table == "parents" && child_table == "children" && join_column == "parent_id"
    ));
}

#[test]
fn column_resolution_rejects_relations_in_both_directions() {
    let path = test_path("ambiguous_columns");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY, link_id INTEGER, \
             FOREIGN KEY(link_id) REFERENCES children(id)); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, link_id INTEGER, \
             FOREIGN KEY(link_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);

    let error = resolve_foreign_key_columns(&path, "parents", "children", "link_id").unwrap_err();
    std::fs::remove_file(&path).unwrap();

    assert!(matches!(
        error,
        DbFormatError::AmbiguousForeignKeyRelation {
            parent_table,
            child_table,
            join_column,
        } if parent_table == "parents" && child_table == "children" && join_column == "link_id"
    ));
}

#[test]
fn resolves_exact_forward_and_reverse_foreign_key_endpoints() {
    let path = test_path("exact_endpoints");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY, referenced_child INTEGER, \
             FOREIGN KEY(referenced_child) REFERENCES children(id)); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER, \
             FOREIGN KEY(parent_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);

    let forward =
        resolve_foreign_key_relation(&path, "parents", "id", "children", "parent_id").unwrap();
    let reverse =
        resolve_foreign_key_relation(&path, "parents", "referenced_child", "children", "id")
            .unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(forward.side, ForeignKeySide::Child);
    assert_eq!(forward.join_column, "parent_id");
    assert_eq!(reverse.side, ForeignKeySide::Parent);
    assert_eq!(reverse.join_column, "referenced_child");
}

#[test]
fn exact_endpoint_resolution_rejects_mismatch_and_ambiguity() {
    let path = test_path("endpoint_errors");
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; \
         CREATE TABLE parents (id INTEGER PRIMARY KEY, other_id INTEGER); \
         CREATE TABLE children (id INTEGER PRIMARY KEY, parent_id INTEGER, \
             FOREIGN KEY(parent_id) REFERENCES parents(id), \
             FOREIGN KEY(parent_id) REFERENCES parents(id));",
    )
    .unwrap();
    drop(conn);

    let mismatch =
        resolve_foreign_key_relation(&path, "parents", "other_id", "children", "parent_id")
            .unwrap_err();
    let ambiguous =
        resolve_foreign_key_relation(&path, "parents", "id", "children", "parent_id").unwrap_err();
    std::fs::remove_file(&path).unwrap();

    assert!(matches!(
        mismatch,
        DbFormatError::MissingForeignKeyEndpoints { .. }
    ));
    assert!(matches!(
        ambiguous,
        DbFormatError::AmbiguousForeignKeyEndpoints { .. }
    ));
}
