//! Self-authored SQLite-oracle checks for WHERE membership with nullable hosts.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{ExecutionContext, ExecutionPurpose, RuntimeParameters};
use ir::{Instance, ScalarType, Value};
use mapping::{Node, Project};
use rusqlite::{Connection, named_params};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_not_in_null_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn prepare_database(dir: &Path) {
    Connection::open(dir.join("numbers.sqlite"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE Numbers (RowID INTEGER PRIMARY KEY, Number INTEGER); \
             INSERT INTO Numbers VALUES (1,NULL),(2,1),(3,2),(4,3),(5,4),(6,2);",
        )
        .unwrap();
}

fn write_design(dir: &Path, membership: &str, defaults: bool) -> (PathBuf, String) {
    let sql = format!("SELECT RowID FROM Numbers WHERE Number {membership} ORDER BY RowID");
    let sql_attribute = sql
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let optional = if defaults { " optional=\"1\"" } else { "" };
    let default_components = if defaults {
        r#"<component name="constant" library="core" uid="8" kind="2">
          <targets><datapoint pos="0" key="80"/></targets><data><constant datatype="integer" value="1"/></data>
        </component><component name="constant" library="core" uid="9" kind="2">
          <targets><datapoint pos="0" key="90"/></targets><data><constant datatype="integer" value="4"/></data>
        </component>"#
    } else {
        ""
    };
    let default_edges = if defaults {
        r#"<vertex vertexkey="80"><edges><edge vertexkey="11"/></edges></vertex>
        <vertex vertexkey="90"><edges><edge vertexkey="13"/></edges></vertex>"#
    } else {
        ""
    };
    let xml = format!(
        r#"<mapping version="26"><resources><datasources><datasource name="numbers">
        <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="numbers.sqlite" name="numbers">
          <LocalViewStorage><LocalViewElement SQL="{sql_attribute}">
            <PathElement Name="main" Kind="Database"/><PathElement Name="Selected" Kind="Select Statement"/>
            <Parameters><Parameter name="First" type="integer"/><Parameter name="Second" type="integer"/></Parameters>
          </LocalViewElement></LocalViewStorage>
        </database_connection></datasource></datasources></resources>
        <component name="map" uid="1"><structure><children>
          {default_components}
          <component name="First" library="core" uid="2" kind="6">
            <sources><datapoint pos="0" key="11"/></sources><targets><datapoint pos="0" key="12"/></targets>
            <data><input datatype="integer" previewvalue="2" usepreviewvalue="1"/><parameter usageKind="input" name="First"{optional}/></data>
          </component>
          <component name="Second" library="core" uid="3" kind="6">
            <sources><datapoint pos="0" key="13"/></sources><targets><datapoint pos="0" key="14"/></targets>
            <data><input datatype="integer" previewvalue="3" usepreviewvalue="1"/><parameter usageKind="input" name="Second"{optional}/></data>
          </component>
          <component name="catalog" library="db" uid="4" kind="15"><data>
            <root><entry name="document"><entry name="Selected" type="routine" outkey="20"/></entry></root>
            <database ref="numbers"><data><selections><selection>
              <PathElement Name="main" Kind="Database"/><PathElement Name="Selected" Kind="Select Statement"/>
            </selection></selections></data></database>
          </data></component>
          <component name="Selected" library="db" uid="5" kind="28"><data>
            <root><entry name="procedure" inpkey="21"/><entry name="Selected">
              <entry name="First" type="attribute" inpkey="22"/><entry name="Second" type="attribute" inpkey="23"/>
            </entry></root>
            <root><entry name="Selected" outkey="30"><entry name="Selected">
              <entry name="RowID" type="attribute" outkey="31"/>
            </entry></entry></root>
          </data></component>
          <component name="rows" library="text" uid="6" kind="16"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40">
              <entry name="RowID" inpkey="41"/>
            </entry></entry></entry></root>
            <text type="csv" outputinstance="out.csv"><settings separator="," quote="&quot;" firstrownames="true">
              <names root="rows" block="Rows"><field0 name="RowID" type="string"/></names>
            </settings></text>
          </data></component>
        </children><graph><vertices>
          {default_edges}
          <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
          <vertex vertexkey="14"><edges><edge vertexkey="23"/></edges></vertex>
          <vertex vertexkey="20"><edges><edge vertexkey="21"/></edges></vertex>
          <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex>
          <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#
    );
    let path = dir.join("membership.mfd");
    std::fs::write(&path, xml).unwrap();
    (path, sql)
}

fn import(path: &Path) -> Project {
    let outcome = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(outcome.imported.warnings.is_empty());
    assert!(engine::validate(&outcome.imported.project).is_empty());
    outcome.imported.project
}

fn assert_late_query_coercion_refusal(dir: &Path, path: &Path, reason: &str) {
    // The CSV collection port names the normalized root, whose path is empty.
    let warning =
        format!("database query source feeding `` is unsupported: {reason}; iteration skipped");
    let imported = mfd::import(path).unwrap();
    assert_eq!(imported.warnings.as_slice(), std::slice::from_ref(&warning));
    assert!(matches!(
        &imported.project.root.iteration,
        mapping::ScopeIteration::None
    ));
    assert!(imported.project.root.bindings.is_empty());
    assert!(imported.project.root.children.is_empty());
    let report = mfd::assess_import(&imported);
    assert!(!report.executable);
    assert!(report.issues.iter().any(|issue| {
        issue.kind == mfd::ImportIssueKind::ImportWarning && issue.message == warning
    }));
    let best_effort = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::BestEffort,
    )
    .unwrap();
    assert_eq!(best_effort.report, report);
    assert_eq!(best_effort.imported.warnings, imported.warnings);
    match mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) {
        Err(mfd::MfdError::IncompatibleImport(strict)) => assert_eq!(*strict, report),
        Err(other) => panic!("expected executable refusal for {reason}, got {other}"),
        Ok(_) => panic!("executable import accepted unsupported query coercion: {reason}"),
    }
    let source =
        format_db::read_instance(&dir.join("numbers.sqlite"), &imported.project.source).unwrap();
    assert_eq!(source.as_repeated().unwrap().len(), 6);
    let output = engine::run(&imported.project, &source).unwrap();
    assert!(
        matches!(&output, Instance::Group(fields) if fields.is_empty())
            || matches!(&output, Instance::Repeated(rows) if rows.is_empty()),
        "a skipped query must not iterate unfiltered source rows: {output:?}"
    );
}

fn projects_after_cycles(dir: &Path, initial: Project) -> Vec<Project> {
    let mut projects = vec![initial.clone()];
    for (label, profile) in [
        ("strict", mfd::ExportProfile::NativeMfd),
        ("ferrule", mfd::ExportProfile::FerruleExtensions),
    ] {
        let mut current = initial.clone();
        for cycle in 0..2 {
            let path = dir.join(format!("{label}-{cycle}.mfd"));
            let report = mfd::export_with_profile(&current, &path, profile).unwrap();
            assert!(report.warnings.is_empty(), "{report}");
            if profile == mfd::ExportProfile::NativeMfd {
                assert!(report.is_native_compatible(), "{report}");
            }
            current = import(&path);
            projects.push(current.clone());
        }
    }
    projects
}

fn oracle(dir: &Path, sql: &str, first: Option<i64>, second: Option<i64>) -> Vec<i64> {
    let connection = Connection::open(dir.join("numbers.sqlite")).unwrap();
    let mut statement = connection.prepare(sql).unwrap();
    let read = |row: &rusqlite::Row<'_>| row.get::<_, i64>(0);
    let rows = if sql.contains(":Second") {
        statement.query_map(named_params! {":First": first, ":Second": second}, read)
    } else if sql.contains(":First") {
        statement.query_map(named_params! {":First": first}, read)
    } else {
        statement.query_map([], read)
    };
    rows.unwrap().collect::<Result<_, _>>().unwrap()
}

fn execute(
    dir: &Path,
    project: &Project,
    purpose: ExecutionPurpose,
    supplied: &[(&str, Option<i64>)],
) -> Result<Vec<i64>, engine::EngineError> {
    let supplied = supplied
        .iter()
        .map(|(name, value)| (*name, (*value).map_or(Value::Null, Value::Int)))
        .collect::<Vec<_>>();
    execute_values(dir, project, purpose, &supplied)
}

fn execute_values(
    dir: &Path,
    project: &Project,
    purpose: ExecutionPurpose,
    supplied: &[(&str, Value)],
) -> Result<Vec<i64>, engine::EngineError> {
    let source = format_db::read_instance(&dir.join("numbers.sqlite"), &project.source).unwrap();
    let mut parameters = RuntimeParameters::new();
    for (name, value) in supplied {
        parameters.insert(*name, value.clone()).unwrap();
    }
    let mapping_path = dir.join("membership.mfd");
    let context = ExecutionContext::new(&mapping_path)
        .with_parameters(&parameters)
        .with_purpose(purpose);
    let result = engine::run_with_context(project, &source, &context)?;
    Ok(result
        .as_repeated()
        .unwrap()
        .iter()
        .map(
            |row| match row.field("RowID").and_then(Instance::as_scalar) {
                Some(Value::Int(value)) => *value,
                Some(Value::String(value)) => value.parse().unwrap(),
                other => panic!("expected RowID, got {other:?}"),
            },
        )
        .collect())
}

#[test]
fn required_not_in_hosts_match_sqlite_through_native_and_ferrule_cycles() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let (path, sql) = write_design(&dir.0, "NOT IN (:First, :Second, 9)", false);
    let projects = projects_after_cycles(&dir.0, import(&path));
    for project in &projects {
        assert!(matches!(
            execute(&dir.0, project, ExecutionPurpose::Run, &[]),
            Err(engine::EngineError::MissingRuntimeParameter { .. })
        ));
        assert_eq!(
            execute(&dir.0, project, ExecutionPurpose::Preview, &[]).unwrap(),
            oracle(&dir.0, &sql, Some(2), Some(3))
        );
        for (first, second) in [
            (None, None),
            (None, Some(2)),
            (Some(2), None),
            (Some(2), Some(3)),
            (Some(9), Some(9)),
            (Some(1), Some(4)),
            (Some(2), Some(2)),
        ] {
            let expected = oracle(&dir.0, &sql, first, second);
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                assert_eq!(
                    execute(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", first), ("Second", second)]
                    )
                    .unwrap(),
                    expected,
                    "purpose={purpose:?}, First={first:?}, Second={second:?}"
                );
            }
        }
    }
    assert_eq!(oracle(&dir.0, &sql, Some(2), Some(3)), [2, 5]);
}

#[test]
fn null_hosts_override_connected_defaults_and_previews() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let (path, sql) = write_design(&dir.0, "NOT IN (:First, :Second)", true);
    let projects = projects_after_cycles(&dir.0, import(&path));
    for project in &projects {
        assert!(project.graph.nodes.values().any(|node| matches!(
            node,
            Node::RuntimeParameterDefault { name, preview: Some(value), .. }
                if name == "First" && value == "2"
        )));
        for (purpose, fallback) in [
            (ExecutionPurpose::Run, (Some(1), Some(4))),
            (ExecutionPurpose::Preview, (Some(2), Some(3))),
        ] {
            assert_eq!(
                execute(&dir.0, project, purpose, &[]).unwrap(),
                oracle(&dir.0, &sql, fallback.0, fallback.1)
            );
            for supplied in [
                vec![("First", None)],
                vec![("Second", None)],
                vec![("First", None), ("Second", Some(2))],
                vec![("First", Some(4))],
            ] {
                let first = supplied
                    .iter()
                    .find(|(name, _)| *name == "First")
                    .map_or(fallback.0, |(_, value)| *value);
                let second = supplied
                    .iter()
                    .find(|(name, _)| *name == "Second")
                    .map_or(fallback.1, |(_, value)| *value);
                assert_eq!(
                    execute(&dir.0, project, purpose, &supplied).unwrap(),
                    oracle(&dir.0, &sql, first, second),
                    "purpose={purpose:?}, supplied={supplied:?}"
                );
            }
        }
    }
}

#[test]
fn in_keeps_matches_when_another_list_operand_is_null() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let (path, sql) = write_design(&dir.0, "IN (:First, :Second)", false);
    let project = import(&path);
    for (first, second) in [
        (None, None),
        (None, Some(2)),
        (Some(2), None),
        (Some(2), Some(3)),
    ] {
        assert_eq!(
            execute(
                &dir.0,
                &project,
                ExecutionPurpose::Run,
                &[("First", first), ("Second", second)],
            )
            .unwrap(),
            oracle(&dir.0, &sql, first, second)
        );
    }
    assert_eq!(oracle(&dir.0, &sql, None, Some(2)), [3, 6]);
}

#[test]
fn one_operand_not_in_excludes_null_source_and_null_host() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let (path, sql) = write_design(&dir.0, "NOT IN (:First)", false);
    let project = import(&path);
    for first in [None, Some(2), Some(9)] {
        assert_eq!(
            execute(&dir.0, &project, ExecutionPurpose::Run, &[("First", first)]).unwrap(),
            oracle(&dir.0, &sql, first, None)
        );
    }
    assert_eq!(oracle(&dir.0, &sql, Some(9), None), [2, 3, 4, 5, 6]);
}

#[test]
fn quoted_and_malformed_memberships_remain_explicit_import_blockers() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    for membership in [
        r#"IN ("NULL")"#,
        "IN ([NULL])",
        "IN ((NULL))",
        "IN (NULL,)",
        "IN (,NULL)",
        "IN (NULL 1)",
        "IN (NULL,,1)",
        "IN (NULL",
        "IN (SELECT RowID FROM Numbers)",
    ] {
        let (path, _) = write_design(&dir.0, membership, false);
        // The rejected query is the only source, so even best-effort import
        // must fail rather than returning an unfiltered mapping.
        assert!(
            matches!(mfd::import(&path), Err(mfd::MfdError::UnsupportedImport(_))),
            "membership={membership}"
        );
        assert!(matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            ),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
    }
    // SQLite admits an empty list and selects even a NULL lhs for NOT IN.
    // That result differs from a nonempty list containing one NULL.
    assert_eq!(
        oracle(
            &dir.0,
            "SELECT RowID FROM Numbers WHERE Number NOT IN () ORDER BY RowID",
            None,
            None
        ),
        [1, 2, 3, 4, 5, 6]
    );
    assert!(
        oracle(
            &dir.0,
            "SELECT RowID FROM Numbers WHERE Number NOT IN (NULL) ORDER BY RowID",
            None,
            None,
        )
        .is_empty()
    );
}

#[test]
fn quoted_null_string_keeps_numeric_coercion_refusal_and_repair_policy() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    let (path, _) = write_design(&dir.0, "IN ('NULL')", false);
    assert_late_query_coercion_refusal(&dir.0, &path, "query operand is not an integer");
}

#[test]
fn literal_null_numeric_members_match_sqlite_through_both_profiles() {
    for real in [false, true] {
        let cases: &[(&str, &[i64])] = if real {
            &[
                ("IN (1.5, NULL)", &[2]),
                ("IN (NULL, 2.5, NULL)", &[3, 6]),
                ("IN (2.5, NULL, 2.5)", &[3, 6]),
                ("IN (NULL)", &[]),
                ("IN (null, 3.5, NuLl)", &[4]),
                ("NOT IN (NULL)", &[]),
                ("NOT IN (NULL, 2.5)", &[]),
                ("NOT IN (2.5, NULL, 3.5)", &[]),
                ("NOT IN (1.5, 4.5, NULL)", &[]),
                ("IN (1.5, 2.5)", &[2, 3, 6]),
                ("NOT IN (1.5, 2.5)", &[4, 5]),
                ("IS NULL", &[1]),
                ("IS NOT NULL", &[2, 3, 4, 5, 6]),
            ]
        } else {
            &[
                ("IN (1, NULL)", &[2]),
                ("IN (NULL, 2, NULL)", &[3, 6]),
                ("IN (2, NULL, 2)", &[3, 6]),
                ("IN (NULL)", &[]),
                ("IN (null, 3, NuLl)", &[4]),
                ("NOT IN (NULL)", &[]),
                ("NOT IN (NULL, 2)", &[]),
                ("NOT IN (2, NULL, 3)", &[]),
                ("NOT IN (1, 4, NULL)", &[]),
                ("IN (1, 2)", &[2, 3, 6]),
                ("NOT IN (1, 2)", &[4, 5]),
                ("IS NULL", &[1]),
                ("IS NOT NULL", &[2, 3, 4, 5, 6]),
            ]
        };
        for &(membership, expected) in cases {
            let dir = TempDir::new();
            if real {
                Connection::open(dir.0.join("numbers.sqlite"))
                    .unwrap()
                    .execute_batch(
                        "CREATE TABLE Numbers (RowID INTEGER PRIMARY KEY, Number REAL); \
                         INSERT INTO Numbers VALUES \
                         (1,NULL),(2,1.5),(3,2.5),(4,3.5),(5,4.5),(6,2.5);",
                    )
                    .unwrap();
            } else {
                prepare_database(&dir.0);
            }
            let (path, sql) = write_design(&dir.0, membership, false);
            assert_eq!(oracle(&dir.0, &sql, None, None), expected);
            let projects = projects_after_cycles(&dir.0, import(&path));
            for project in &projects {
                let column = project.source.child("Number").unwrap();
                let expected_type = if real {
                    ScalarType::Float
                } else {
                    ScalarType::Int
                };
                assert!(matches!(
                    column.kind,
                    ir::SchemaKind::Scalar { ty } if ty == expected_type
                ));
                for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                    assert_eq!(
                        execute(&dir.0, project, purpose, &[]).unwrap(),
                        expected,
                        "real={real}, membership={membership}, purpose={purpose:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn literal_null_members_keep_required_hosts_typed_and_empty_rows_lazy() {
    for membership in [
        "IN (NULL, :First, :Second)",
        "IN (:First, NULL, :Second)",
        "IN (:First, :Second, NULL)",
        "NOT IN (NULL, :First, :Second)",
        "NOT IN (:First, NULL, :Second)",
        "NOT IN (:First, :Second, NULL)",
    ] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, sql) = write_design(&dir.0, membership, false);
        let projects = projects_after_cycles(&dir.0, import(&path));
        for project in &projects {
            assert!(matches!(
                execute(&dir.0, project, ExecutionPurpose::Run, &[]),
                Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                    if name == "First"
            ));
            assert_eq!(
                execute(&dir.0, project, ExecutionPurpose::Preview, &[]).unwrap(),
                oracle(&dir.0, &sql, Some(2), Some(3))
            );
            for first in [None, Some(2)] {
                assert!(matches!(
                    execute(&dir.0, project, ExecutionPurpose::Run, &[("First", first)]),
                    Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                        if name == "Second"
                ));
            }
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                for (invalid, values) in [
                    (
                        "First",
                        [("First", Value::Bool(true)), ("Second", Value::Int(2))],
                    ),
                    (
                        "Second",
                        [("First", Value::Int(2)), ("Second", Value::Bool(true))],
                    ),
                ] {
                    assert!(matches!(
                        execute_values(&dir.0, project, purpose, &values),
                        Err(engine::EngineError::RuntimeParameterType {
                            name,
                            expected: ScalarType::Int,
                            found: "bool",
                            ..
                        }) if name == invalid
                    ));
                }
                for (first, second) in [
                    (None, None),
                    (None, Some(2)),
                    (Some(2), None),
                    (Some(2), Some(3)),
                    (Some(9), Some(9)),
                    (Some(1), Some(4)),
                    (Some(2), Some(2)),
                ] {
                    assert_eq!(
                        execute(
                            &dir.0,
                            project,
                            purpose,
                            &[("First", first), ("Second", second)]
                        )
                        .unwrap(),
                        oracle(&dir.0, &sql, first, second),
                        "membership={membership}, purpose={purpose:?}, First={first:?}, Second={second:?}"
                    );
                }
            }
        }
        Connection::open(dir.0.join("numbers.sqlite"))
            .unwrap()
            .execute("DELETE FROM Numbers", [])
            .unwrap();
        for project in &projects {
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                assert_eq!(
                    execute(&dir.0, project, purpose, &[]).unwrap(),
                    oracle(&dir.0, &sql, None, None)
                );
                assert!(
                    execute_values(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", Value::Bool(true)), ("Second", Value::Int(2))]
                    )
                    .unwrap()
                    .is_empty()
                );
            }
        }
    }
}

#[test]
fn literal_null_members_keep_optional_defaults_previews_and_null_overrides() {
    for membership in [
        "IN (NULL, :First, :Second)",
        "IN (:First, NULL, :Second)",
        "IN (:First, :Second, NULL)",
        "NOT IN (NULL, :First, :Second)",
        "NOT IN (:First, NULL, :Second)",
        "NOT IN (:First, :Second, NULL)",
    ] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, sql) = write_design(&dir.0, membership, true);
        let projects = projects_after_cycles(&dir.0, import(&path));
        for project in &projects {
            for (purpose, fallback) in [
                (ExecutionPurpose::Run, (Some(1), Some(4))),
                (ExecutionPurpose::Preview, (Some(2), Some(3))),
            ] {
                assert_eq!(
                    execute(&dir.0, project, purpose, &[]).unwrap(),
                    oracle(&dir.0, &sql, fallback.0, fallback.1)
                );
                for supplied in [
                    vec![("First", None)],
                    vec![("Second", None)],
                    vec![("First", None), ("Second", Some(2))],
                    vec![("First", Some(4))],
                    vec![("First", Some(2)), ("Second", Some(2))],
                ] {
                    let first = supplied
                        .iter()
                        .find(|(name, _)| *name == "First")
                        .map_or(fallback.0, |(_, value)| *value);
                    let second = supplied
                        .iter()
                        .find(|(name, _)| *name == "Second")
                        .map_or(fallback.1, |(_, value)| *value);
                    assert_eq!(
                        execute(&dir.0, project, purpose, &supplied).unwrap(),
                        oracle(&dir.0, &sql, first, second),
                        "membership={membership}, purpose={purpose:?}, supplied={supplied:?}"
                    );
                }
                assert!(matches!(
                    execute_values(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", Value::Bool(true))]
                    ),
                    Err(engine::EngineError::RuntimeParameterType {
                        name,
                        expected: ScalarType::Int,
                        found: "bool",
                        ..
                    }) if name == "First"
                ));
            }
        }
    }
}

#[test]
fn literal_null_members_at_the_256_item_limit_keep_exact_row_effects() {
    for operator in ["IN", "NOT IN"] {
        for first in [true, false] {
            let dir = TempDir::new();
            prepare_database(&dir.0);
            let mut operands = vec!["NULL"; 255];
            operands.insert(if first { 0 } else { operands.len() }, "1");
            let membership = format!("{operator} ({})", operands.join(", "));
            let (path, sql) = write_design(&dir.0, &membership, false);
            let expected: &[i64] = if operator == "IN" { &[2] } else { &[] };
            assert_eq!(oracle(&dir.0, &sql, None, None), expected);
            let projects = projects_after_cycles(&dir.0, import(&path));
            for project in &projects {
                for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                    assert_eq!(
                        execute(&dir.0, project, purpose, &[]).unwrap(),
                        expected,
                        "operator={operator}, first={first}, purpose={purpose:?}"
                    );
                }
            }
            operands.push("NULL");
            let membership = format!("{operator} ({})", operands.join(", "));
            let (path, _) = write_design(&dir.0, &membership, false);
            assert!(matches!(
                mfd::import(&path),
                Err(mfd::MfdError::UnsupportedImport(_))
            ));
            assert!(matches!(
                mfd::import_with_profile(
                    &path,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                ),
                Err(mfd::MfdError::UnsupportedImport(_))
            ));
        }
    }
}

#[test]
fn literal_null_members_at_the_256_item_limit_keep_late_host_reads_ordered() {
    for operator in ["IN", "NOT IN"] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let mut operands = vec!["NULL"; 254];
        // Reverse declaration order and place both hosts after every Null.
        operands.extend([":Second", ":First"]);
        let membership = format!("{operator} ({})", operands.join(", "));
        let (path, sql) = write_design(&dir.0, &membership, false);
        let expected: &[i64] = if operator == "IN" { &[3, 4, 6] } else { &[] };
        assert_eq!(oracle(&dir.0, &sql, Some(2), Some(3)), expected);
        let projects = projects_after_cycles(&dir.0, import(&path));
        for project in &projects {
            assert!(matches!(
                execute(&dir.0, project, ExecutionPurpose::Run, &[]),
                Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                    if name == "Second"
            ));
            assert!(matches!(
                execute(&dir.0, project, ExecutionPurpose::Run, &[("Second", None)]),
                Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                    if name == "First"
            ));
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                assert!(matches!(
                    execute_values(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", Value::Bool(true)), ("Second", Value::Bool(true))]
                    ),
                    Err(engine::EngineError::RuntimeParameterType {
                        name,
                        expected: ScalarType::Int,
                        found: "bool",
                        ..
                    }) if name == "Second"
                ));
                assert!(matches!(
                    execute_values(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", Value::Bool(true)), ("Second", Value::Null)]
                    ),
                    Err(engine::EngineError::RuntimeParameterType {
                        name,
                        expected: ScalarType::Int,
                        found: "bool",
                        ..
                    }) if name == "First"
                ));
                assert_eq!(
                    execute(
                        &dir.0,
                        project,
                        purpose,
                        &[("First", Some(2)), ("Second", Some(3))]
                    )
                    .unwrap(),
                    expected
                );
            }
            assert_eq!(
                execute(&dir.0, project, ExecutionPurpose::Preview, &[]).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn literal_null_members_do_not_admit_null_connected_query_defaults() {
    for membership in ["IN (:First)", "IN (NULL, :First)", "NOT IN (:First, NULL)"] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, sql) = write_design(&dir.0, membership, true);
        let numeric_control = import(&path);
        assert!(numeric_control.graph.nodes.values().any(|node| matches!(
            node,
            Node::RuntimeParameterDefault { name, default, .. }
                if name == "First" && matches!(
                    numeric_control.graph.nodes.get(default),
                    Some(Node::Const { value: Value::Int(1) })
                )
        )));
        assert_eq!(
            execute(&dir.0, &numeric_control, ExecutionPurpose::Run, &[]).unwrap(),
            oracle(&dir.0, &sql, Some(1), Some(4))
        );
        let xml = std::fs::read_to_string(&path).unwrap().replace(
            r#"datatype="integer" value="1""#,
            r#"datatype="integer" value="""#,
        );
        // Bind the exact static constant before the query import. The focused
        // parser/coercion unit also proves this encoding becomes Value::Null.
        let document = roxmltree::Document::parse(&xml).unwrap();
        let constant = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some("8"))
            .unwrap()
            .descendants()
            .find(|node| node.has_tag_name("constant"))
            .unwrap();
        assert_eq!(constant.attribute("datatype"), Some("integer"));
        assert_eq!(constant.attribute("value"), Some(""));
        std::fs::write(&path, &xml).unwrap();
        assert_late_query_coercion_refusal(
            &dir.0,
            &path,
            "query `Selected` parameter `:First` declared as Int cannot be converted: query parameters cannot be null",
        );
    }
}

#[test]
fn literal_null_members_lower_and_emit_both_languages_without_building() {
    for membership in ["IN (NULL, :First, 2)", "NOT IN (:First, 2, NULL)"] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, _) = write_design(&dir.0, membership, false);
        let projects = projects_after_cycles(&dir.0, import(&path));
        for project in &projects {
            let program = codegen::lower(project).unwrap();
            assert!(program.expressions.iter().any(|node| matches!(
                node.expression,
                codegen::Expression::Const { value: Value::Null }
            )));
            assert!(program.expressions.iter().any(|node| matches!(
                &node.expression,
                codegen::Expression::RuntimeParameter { name, ty: ScalarType::Int }
                    if name == "First"
            )));
            let options = codegen_rust::Options {
                package_name: "null-membership".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path("../runtime".into()),
            };
            let rust = codegen_rust::emit(&program, &options).unwrap();
            assert_eq!(rust, codegen_rust::emit(&program, &options).unwrap());
            let rust_source = rust
                .files()
                .iter()
                .find(|file| file.path.as_str() == "src/lib.rs")
                .and_then(|file| std::str::from_utf8(&file.contents).ok())
                .unwrap();
            assert!(rust_source.contains("Ok(Value::Null)"));
            assert!(rust_source.contains("call(\"exists\", &args)"));
            assert!(rust_source.contains("require_bool("));
            assert!(rust_source.contains("context.runtime_parameter("));
            let csharp = codegen_csharp::emit(&program).unwrap();
            assert_eq!(csharp, codegen_csharp::emit(&program).unwrap());
            let csharp_source = csharp
                .files()
                .iter()
                .find(|file| file.path.as_str() == "GeneratedMapping.cs")
                .and_then(|file| std::str::from_utf8(&file.contents).ok())
                .unwrap();
            assert!(csharp_source.contains("FerruleValue.Null"));
            assert!(csharp_source.contains("FerruleFunctions.Call(\"exists\","));
            assert!(csharp_source.contains("RequireBoolean("));
            assert!(csharp_source.contains("context.ResolveRuntimeParameter("));
        }
    }
}

fn assert_empty_membership_emits_both_languages(project: &Project, expected: bool) {
    assert!(project.root.source().is_some());
    assert!(project.root.filter.is_some());
    let program = codegen::lower(project).unwrap();
    assert!(program.expressions.iter().any(|node| matches!(
        &node.expression,
        codegen::Expression::Const { value: Value::Bool(value) } if *value == expected
    )));
    let options = codegen_rust::Options {
        package_name: "empty-membership".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path("../runtime".into()),
    };
    let rust = codegen_rust::emit(&program, &options).unwrap();
    assert_eq!(rust, codegen_rust::emit(&program, &options).unwrap());
    assert!(
        rust.files()
            .iter()
            .any(|file| file.path.as_str() == "src/lib.rs")
    );
    let csharp = codegen_csharp::emit(&program).unwrap();
    assert_eq!(csharp, codegen_csharp::emit(&program).unwrap());
    assert!(
        csharp
            .files()
            .iter()
            .any(|file| file.path.as_str() == "GeneratedMapping.cs")
    );
}

#[test]
fn sqlite_empty_memberships_match_exact_rows_for_numeric_and_text_columns() {
    for ty in [ScalarType::Int, ScalarType::Float, ScalarType::String] {
        for operator in ["IN", "NOT IN"] {
            let dir = TempDir::new();
            match ty {
                ScalarType::Int => prepare_database(&dir.0),
                ScalarType::Float => {
                    Connection::open(dir.0.join("numbers.sqlite"))
                        .unwrap()
                        .execute_batch(
                            "CREATE TABLE Numbers (RowID INTEGER PRIMARY KEY, Number REAL); \
                             INSERT INTO Numbers VALUES \
                             (1,NULL),(2,1.5),(3,2.5),(4,3.5),(5,4.5),(6,2.5);",
                        )
                        .unwrap();
                }
                ScalarType::String => {
                    Connection::open(dir.0.join("numbers.sqlite"))
                        .unwrap()
                        .execute_batch(
                            "CREATE TABLE Numbers (RowID INTEGER PRIMARY KEY, Number TEXT); \
                             INSERT INTO Numbers VALUES \
                             (1,NULL),(2,'alpha'),(3,'beta'),(4,''),(5,'gamma'),(6,'beta');",
                        )
                        .unwrap();
                }
                ScalarType::Bool => unreachable!(),
            }
            let membership = format!("{operator} ()");
            let (path, sql) = write_design(&dir.0, &membership, false);
            let expected: &[i64] = if operator == "IN" {
                &[]
            } else {
                &[1, 2, 3, 4, 5, 6]
            };
            assert_eq!(oracle(&dir.0, &sql, None, None), expected);
            let projects = projects_after_cycles(&dir.0, import(&path));
            for project in &projects {
                assert!(matches!(
                    project.source.child("Number").unwrap().kind,
                    ir::SchemaKind::Scalar { ty: actual } if actual == ty
                ));
                assert_empty_membership_emits_both_languages(project, operator == "NOT IN");
                for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                    assert_eq!(execute(&dir.0, project, purpose, &[]).unwrap(), expected);
                }
            }
            if ty == ScalarType::String {
                let (path, _) = write_design(&dir.0, "IN ('alpha')", false);
                assert_late_query_coercion_refusal(
                    &dir.0,
                    &path,
                    "text IN collation cannot be established from SQLite schema metadata",
                );
            }
        }
    }
}

#[test]
fn sqlite_empty_memberships_keep_source_sort_and_window_controls() {
    for operator in ["IN", "NOT IN"] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, sql) = write_design(&dir.0, &format!("{operator} ()"), false);
        let order = "ORDER BY RowID DESC LIMIT 3 OFFSET 1";
        let sql = sql.replace("ORDER BY RowID", order);
        let xml = std::fs::read_to_string(&path).unwrap();
        assert_eq!(xml.matches("ORDER BY RowID").count(), 1);
        std::fs::write(&path, xml.replace("ORDER BY RowID", order)).unwrap();
        let expected: &[i64] = if operator == "IN" { &[] } else { &[5, 4, 3] };
        assert_eq!(oracle(&dir.0, &sql, None, None), expected);
        let projects = projects_after_cycles(&dir.0, import(&path));
        for project in &projects {
            assert!(project.root.source().is_some());
            assert!(project.root.filter.is_some());
            assert!(project.root.sort_by.is_some());
            assert!(project.root.sort_descending);
            assert_eq!(project.root.windows.len(), 2);
            for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                assert_eq!(execute(&dir.0, project, purpose, &[]).unwrap(), expected);
            }
        }
    }
}

#[test]
fn sqlite_empty_memberships_keep_unrelated_host_reads_and_empty_source_laziness() {
    for operator in ["IN", "NOT IN"] {
        for empty_first in [false, true] {
            for defaults in [false, true] {
                let dir = TempDir::new();
                prepare_database(&dir.0);
                let membership = if empty_first {
                    format!("{operator} () AND Number > :First")
                } else {
                    format!("> :First AND Number {operator} ()")
                };
                let (path, sql) = write_design(&dir.0, &membership, defaults);
                let projects = projects_after_cycles(&dir.0, import(&path));
                for project in &projects {
                    if defaults {
                        let expected = oracle(&dir.0, &sql, Some(1), None);
                        assert_eq!(
                            execute(&dir.0, project, ExecutionPurpose::Run, &[]).unwrap(),
                            expected
                        );
                    } else {
                        assert!(matches!(
                            execute(&dir.0, project, ExecutionPurpose::Run, &[]),
                            Err(engine::EngineError::MissingRuntimeParameter { name, .. })
                                if name == "First"
                        ));
                    }
                    assert_eq!(
                        execute(&dir.0, project, ExecutionPurpose::Preview, &[]).unwrap(),
                        oracle(&dir.0, &sql, Some(2), None)
                    );
                    for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                        assert!(matches!(
                            execute_values(&dir.0, project, purpose, &[("First", Value::Bool(true))]),
                            Err(engine::EngineError::RuntimeParameterType {
                                name,
                                expected: ScalarType::Int,
                                found: "bool",
                                ..
                            }) if name == "First"
                        ));
                        for first in [None, Some(1), Some(2), Some(9)] {
                            assert_eq!(
                                execute(&dir.0, project, purpose, &[("First", first)]).unwrap(),
                                oracle(&dir.0, &sql, first, None),
                                "operator={operator}, empty_first={empty_first}, defaults={defaults}, first={first:?}"
                            );
                        }
                    }
                }
                Connection::open(dir.0.join("numbers.sqlite"))
                    .unwrap()
                    .execute("DELETE FROM Numbers", [])
                    .unwrap();
                for project in &projects {
                    for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
                        assert!(oracle(&dir.0, &sql, None, None).is_empty());
                        assert!(execute(&dir.0, project, purpose, &[]).unwrap().is_empty());
                        assert!(
                            execute_values(
                                &dir.0,
                                project,
                                purpose,
                                &[("First", Value::Bool(true))]
                            )
                            .unwrap()
                            .is_empty()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn sqlite_empty_memberships_require_both_explicit_sqlite_boundary_kinds() {
    for operator in ["IN", "NOT IN"] {
        let dir = TempDir::new();
        prepare_database(&dir.0);
        let (path, _) = write_design(&dir.0, &format!("{operator} ()"), false);
        let sqlite = std::fs::read_to_string(&path).unwrap();
        let qualified = sqlite.replace(
            r#"database_kind="SQLite" import_kind="SQLite""#,
            r#"database_kind="sQlItE" import_kind="SQLITE""#,
        );
        assert_ne!(qualified, sqlite);
        std::fs::write(&path, qualified).unwrap();
        let expected: &[i64] = if operator == "IN" {
            &[]
        } else {
            &[1, 2, 3, 4, 5, 6]
        };
        assert_eq!(
            execute(&dir.0, &import(&path), ExecutionPurpose::Run, &[]).unwrap(),
            expected
        );
        for (from, to) in [
            (r#"database_kind="SQLite""#, ""),
            (r#"import_kind="SQLite""#, ""),
            (r#"database_kind="SQLite""#, r#"database_kind="PostgreSQL""#),
            (r#"import_kind="SQLite""#, r#"import_kind="PostgreSQL""#),
        ] {
            let xml = sqlite.replace(from, to);
            assert_ne!(xml, sqlite);
            std::fs::write(&path, xml).unwrap();
            assert!(matches!(
                mfd::import(&path),
                Err(mfd::MfdError::UnsupportedImport(_))
            ));
            assert!(matches!(
                mfd::import_with_profile(
                    &path,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                ),
                Err(mfd::MfdError::UnsupportedImport(_))
            ));
        }
    }
}

#[test]
fn sqlite_empty_memberships_still_require_a_resolved_column_and_exact_list_grammar() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    for membership in ["IN (,)", "IN (())", "IN (1,)", "NOT IN (1,,2)", "IN (,1)"] {
        let (path, _) = write_design(&dir.0, membership, false);
        assert!(matches!(
            mfd::import(&path),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
        assert!(matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            ),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
    }
    for operator in ["IN", "NOT IN"] {
        let (path, _) = write_design(&dir.0, &format!("{operator} ()"), false);
        let xml = std::fs::read_to_string(&path)
            .unwrap()
            .replace("WHERE Number", "WHERE Missing");
        assert!(xml.contains("WHERE Missing"));
        std::fs::write(&path, xml).unwrap();
        assert!(matches!(
            mfd::import(&path),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
        assert!(matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            ),
            Err(mfd::MfdError::UnsupportedImport(_))
        ));
    }
}
