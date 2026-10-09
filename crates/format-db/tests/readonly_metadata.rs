use format_db::{
    DbFormatError, ForeignKeyColumns, ForeignKeyRelation, ForeignKeySide, introspect,
    resolve_foreign_key_columns, resolve_foreign_key_relation, validate_relational_schema,
};
use ir::{ScalarType, SchemaNode, ValueGeneration};
use rusqlite::ErrorCode;

#[path = "fixtures/readonly_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn generated(name: &str) -> SchemaNode {
    let mut node = SchemaNode::scalar(name, ScalarType::Int);
    node.value_generation = Some(ValueGeneration::MaxNumber);
    node
}

fn records_schema() -> SchemaNode {
    SchemaNode::group(
        "Records",
        vec![
            generated("Id"),
            SchemaNode::scalar("GroupId", ScalarType::Int),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Float),
            SchemaNode::scalar("Active", ScalarType::Bool),
        ],
    )
    .repeating()
}

fn groups_schema() -> SchemaNode {
    SchemaNode::group(
        "Groups",
        vec![
            generated("GroupId"),
            SchemaNode::scalar("Name", ScalarType::String),
        ],
    )
    .repeating()
}

fn relational_schema() -> SchemaNode {
    let mut child = records_schema();
    child.name = "Records|GroupId".into();
    SchemaNode::group(
        "Groups",
        vec![
            generated("GroupId"),
            SchemaNode::scalar("Name", ScalarType::String),
            child,
        ],
    )
    .repeating()
}

fn metadata_results(path: &std::path::Path) -> [(&'static str, Result<String, DbFormatError>); 4] {
    [
        (
            "introspect",
            introspect(path, "Records").map(|value| format!("{value:#?}")),
        ),
        (
            "validate",
            validate_relational_schema(path, &relational_schema())
                .map(|value| format!("{value:?}")),
        ),
        (
            "columns",
            resolve_foreign_key_columns(path, "Groups", "Records", "GroupId")
                .map(|value| format!("{value:#?}")),
        ),
        (
            "relation",
            resolve_foreign_key_relation(path, "Groups", "GroupId", "Records", "GroupId")
                .map(|value| format!("{value:#?}")),
        ),
    ]
}

fn assert_sqlite_error(result: &Result<String, DbFormatError>, expected: ErrorCode) {
    assert!(
        matches!(result, Err(DbFormatError::Sqlite(rusqlite::Error::SqliteFailure(error, _))) if error.code == expected),
        "original result: {result:#?}"
    );
}

#[test]
fn missing_database_refuses_every_metadata_operation_without_creating_files() {
    let fixture = Fixture::new("missing");
    fixture.retain(
        "expected.txt",
        "All four operations: SqliteFailure(CannotOpen); no database or sidecar created.\n",
    );
    let before = fixture.snapshot("before");
    let results = metadata_results(&fixture.database());
    for (name, result) in &results {
        fixture.original(&format!("{name}.original.txt"), result);
    }
    let after = fixture.snapshot("after");
    for (_, result) in &results {
        assert_sqlite_error(result, ErrorCode::CannotOpen);
    }
    assert_eq!(after, before);
}

#[test]
fn clean_database_preserves_columns_types_generated_keys_and_relationships() {
    let fixture = Fixture::new("clean");
    fixture.retain("expected.txt", format!("Groups={:#?}\nRecords={:#?}\nFK: Groups.GroupId=Records.GroupId, child-owned; relational validation succeeds.\n", groups_schema(), records_schema()));
    fixture.author_database();
    let before = fixture.snapshot("before");
    let groups = introspect(&fixture.database(), "groups");
    let records = introspect(&fixture.database(), "Records");
    let columns = resolve_foreign_key_columns(&fixture.database(), "Groups", "Records", "GroupId");
    let relation = resolve_foreign_key_relation(
        &fixture.database(),
        "Groups",
        "GroupId",
        "Records",
        "GroupId",
    );
    let validation = validate_relational_schema(&fixture.database(), &relational_schema());
    fixture.original("groups.original.txt", &groups);
    fixture.original("records.original.txt", &records);
    fixture.original("columns.original.txt", &columns);
    fixture.original("relation.original.txt", &relation);
    fixture.original("validation.original.txt", &validation);
    let after = fixture.snapshot("after");
    assert_eq!(groups.unwrap(), groups_schema());
    assert_eq!(records.unwrap(), records_schema());
    assert_eq!(
        columns.unwrap(),
        ForeignKeyColumns {
            parent_column: "GroupId".into(),
            child_column: "GroupId".into()
        }
    );
    assert_eq!(
        relation.unwrap(),
        ForeignKeyRelation {
            side: ForeignKeySide::Child,
            join_column: "GroupId".into()
        }
    );
    validation.unwrap();
    assert_eq!(after, before);
}

#[test]
fn hot_rollback_journal_refuses_every_metadata_operation_without_recovery() {
    let fixture = Fixture::new("hot");
    fixture.retain("expected.txt", "All four operations: SqliteFailure(ReadOnly); original database and hot journal bytes remain exact.\n");
    fixture.author_hot_journal();
    let before = fixture.snapshot("before");
    let results = metadata_results(&fixture.database());
    for (name, result) in &results {
        fixture.original(&format!("{name}.original.txt"), result);
    }
    let after = fixture.snapshot("after");
    for (_, result) in &results {
        assert_sqlite_error(result, ErrorCode::ReadOnly);
    }
    assert_eq!(after, before);
}

#[test]
fn metadata_path_does_not_interpret_sqlite_uri_options() {
    let fixture = Fixture::new("uri");
    fixture.retain("expected.txt", "The existing database's file: URI spelling is a literal nonexistent path: SqliteFailure(CannotOpen), no new files.\n");
    fixture.author_database();
    let before = fixture.snapshot("before");
    let literal = format!(
        "file:{}?mode=ro",
        fixture.directory().join("database.sqlite").display()
    );
    let result =
        introspect(std::path::Path::new(&literal), "Records").map(|value| format!("{value:#?}"));
    fixture.original("uri.original.txt", &result);
    let after = fixture.snapshot("after");
    assert_sqlite_error(&result, ErrorCode::CannotOpen);
    assert_eq!(after, before);
}
