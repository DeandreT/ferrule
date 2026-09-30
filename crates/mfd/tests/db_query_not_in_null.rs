//! Self-authored SQLite-oracle checks for WHERE membership with nullable hosts.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::{ExecutionContext, ExecutionPurpose, RuntimeParameters};
use ir::{Instance, Value};
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
          <LocalViewStorage><LocalViewElement SQL="{sql}">
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
    let source = format_db::read_instance(&dir.join("numbers.sqlite"), &project.source).unwrap();
    let mut parameters = RuntimeParameters::new();
    for (name, value) in supplied {
        parameters
            .insert(*name, value.map_or(Value::Null, Value::Int))
            .unwrap();
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
fn literal_null_and_empty_lists_remain_explicit_import_blockers() {
    let dir = TempDir::new();
    prepare_database(&dir.0);
    for membership in ["NOT IN ()", "IN ()", "NOT IN (NULL)", "IN (1, NULL)"] {
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
    // The importer rejects that grammar instead of applying a nonempty-list guard.
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
