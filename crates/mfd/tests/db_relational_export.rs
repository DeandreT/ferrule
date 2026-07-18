use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Graph, Project, Scope};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_db_export_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn empty_project(source: SchemaNode, target: SchemaNode) -> Project {
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::new(),
        },
        root: Scope::default(),
    }
}

fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

#[test]
fn exports_and_reimports_one_table_with_nested_foreign_key_relations() {
    let directory = TempDir::new("nested_source");
    let database = directory.0.join("company.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE departments (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE people (
               id INTEGER PRIMARY KEY,
               department_id INTEGER NOT NULL,
               name TEXT NOT NULL,
               FOREIGN KEY(department_id) REFERENCES departments(id)
             );",
        )
        .unwrap();
    drop(connection);

    let people = SchemaNode::group(
        "people|department_id",
        vec![
            scalar("id", ScalarType::Int),
            scalar("department_id", ScalarType::Int),
            scalar("name", ScalarType::String),
        ],
    )
    .repeating();
    let departments = SchemaNode::group(
        "departments",
        vec![
            scalar("id", ScalarType::Int),
            scalar("name", ScalarType::String),
            people,
        ],
    )
    .repeating();
    let mut project = empty_project(
        departments,
        SchemaNode::group("Result", vec![scalar("Value", ScalarType::String)]),
    );
    project.source_path = Some("company.sqlite".into());
    let design = directory.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    assert!(xml.contains("name=\"departments\" type=\"table\" outkey="));
    assert!(xml.contains("name=\"people|department_id\" type=\"table\" outkey="));
    assert!(xml.contains("name=\"id\" outkey="));
    assert!(xml.contains("datatype=\"integer\""));
    assert!(xml.contains("Name=\"departments\" Kind=\"Table\""));
    assert!(xml.contains("Name=\"people\" Kind=\"Table\""));

    std::fs::remove_file(database).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn exports_and_reimports_single_nested_table_below_database_wrapper() {
    let directory = TempDir::new("wrapped_nested_source");
    let people = SchemaNode::group(
        "people|department_id",
        vec![
            scalar("id", ScalarType::Int),
            scalar("department_id", ScalarType::Int),
            scalar("name", ScalarType::String),
        ],
    )
    .repeating();
    let departments = SchemaNode::group(
        "departments",
        vec![
            scalar("id", ScalarType::Int),
            scalar("name", ScalarType::String),
            people,
        ],
    )
    .repeating();
    let source = SchemaNode::group("database", vec![departments]);
    let mut project = empty_project(
        source,
        SchemaNode::group("Result", vec![scalar("Value", ScalarType::String)]),
    );
    project.source_path = Some("company.sqlite".into());
    let design = directory.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    assert!(xml.contains("ferrule-database-wrapper=\"1\""));
    let imported = mfd::import(&design).unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source, project.source);
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn exports_and_reimports_multiple_target_tables_with_a_nested_relation() {
    let directory = TempDir::new("multi_target");
    let database = directory.0.join("target.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE addresses (
               id INTEGER PRIMARY KEY,
               customer_id INTEGER NOT NULL,
               city TEXT NOT NULL,
               FOREIGN KEY(customer_id) REFERENCES customers(id)
             );
             CREATE TABLE orders (
               id INTEGER PRIMARY KEY,
               customer_id INTEGER NOT NULL,
               FOREIGN KEY(customer_id) REFERENCES customers(id)
             );",
        )
        .unwrap();
    drop(connection);

    let addresses = SchemaNode::group(
        "addresses|customer_id",
        vec![
            scalar("id", ScalarType::Int),
            scalar("customer_id", ScalarType::Int),
            scalar("city", ScalarType::String),
        ],
    )
    .repeating();
    let customers = SchemaNode::group(
        "customers",
        vec![
            scalar("id", ScalarType::Int),
            scalar("name", ScalarType::String),
            addresses,
        ],
    )
    .repeating();
    let orders = SchemaNode::group(
        "orders",
        vec![
            scalar("id", ScalarType::Int),
            scalar("customer_id", ScalarType::Int),
        ],
    )
    .repeating();
    let mut project = empty_project(
        SchemaNode::group("Source", vec![scalar("Value", ScalarType::String)]),
        SchemaNode::group("database", vec![customers, orders]),
    );
    project.target_path = Some("target.sqlite".into());
    let design = directory.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    assert!(xml.contains("name=\"customers\" type=\"table\" inpkey="));
    assert!(xml.contains("name=\"orders\" type=\"table\" inpkey="));
    assert!(xml.contains("name=\"addresses|customer_id\" type=\"table\" inpkey="));

    std::fs::remove_file(database).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target, project.target);
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn rejects_non_relational_nested_groups_without_touching_the_design() {
    let invalid = SchemaNode::group(
        "departments",
        vec![SchemaNode::group(
            "profile",
            vec![scalar("name", ScalarType::String)],
        )],
    )
    .repeating();
    let mut project = empty_project(
        invalid,
        SchemaNode::group("Result", vec![scalar("Value", ScalarType::String)]),
    );
    project.source_path = Some("source.sqlite".into());
    let directory = TempDir::new("invalid");
    let design = directory.0.join("mapping.mfd");
    std::fs::write(&design, "sentinel").unwrap();

    let error = mfd::export(&project, &design).unwrap_err();
    assert!(format!("{error:#}").contains("canonical relational table tree"));
    assert_eq!(std::fs::read_to_string(design).unwrap(), "sentinel");
}
