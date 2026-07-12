use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::IterationOutput;
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_standalone_query_{}_{}",
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

fn prepare(dir: &Path, populated: bool) -> PathBuf {
    let connection = Connection::open(dir.join("inventory.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE Articles (Number INTEGER PRIMARY KEY, Name TEXT NOT NULL, SinglePrice REAL NOT NULL);",
        )
        .unwrap();
    if populated {
        connection
            .execute_batch(
                "INSERT INTO Articles VALUES (1, 'Cable', 14.5), (2, 'Monitor', 240.0), (3, 'Stand', 85.0);",
            )
            .unwrap();
    }
    std::fs::write(
        dir.join("article.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Article"><xs:complexType><xs:sequence>
            <xs:element name="Number" type="xs:integer"/>
            <xs:element name="Name" type="xs:string"/>
            <xs:element name="SinglePrice" type="xs:decimal"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let design = dir.join("top-article.mfd");
    std::fs::write(&design, design_xml("LIMIT 1")).unwrap();
    design
}

fn design_xml(limit: &str) -> String {
    format!(
        r#"<mapping version="26"><resources><datasources><datasource name="inventory">
          <database_connection database_kind="SQLite" import_kind="SQLite" ConnectionString="inventory.sqlite" name="inventory">
            <LocalViewStorage><LocalViewElement SQL="SELECT * FROM Articles ORDER BY SinglePrice DESC {limit}">
              <PathElement Name="main" Kind="Database"/><PathElement Name="TopArticle" Kind="Select Statement"/>
            </LocalViewElement></LocalViewStorage>
          </database_connection></datasource></datasources></resources>
          <component name="map" uid="1"><structure><children>
            <component name="db" library="db" uid="2" kind="15"><data><root><entry name="document">
              <entry name="Articles" type="table"><entry name="Number"/><entry name="SinglePrice"/></entry>
              <entry name="TopArticle" type="routine" displayselectionmode="selection"/>
              <entry name="TopArticle" type="routine" outkey="1"><entry name="TopArticle" type="table">
                <entry name="Number" outkey="4"/><entry name="Name" outkey="7"/><entry name="SinglePrice" outkey="10"/>
              </entry></entry>
            </entry></root><database ref="inventory"><data><selections>
              <selection><PathElement Name="main" Kind="Database"/><PathElement Name="TopArticle" Kind="Select Statement"/></selection>
              <selection><PathElement Name="main" Kind="Database"/><PathElement Name="Articles" Kind="Table"/></selection>
            </selections></data></database></data></component>
            <component name="Article" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
              <entry name="FileInstance"><entry name="document"><entry name="Article" inpkey="2">
                <entry name="Number" inpkey="5"/><entry name="Name" inpkey="8"/><entry name="SinglePrice" inpkey="11"/>
              </entry></entry></entry>
            </root><document schema="article.xsd" instanceroot="{{}}Article"/><wsdl/></data></component>
          </children><graph><vertices>
            <vertex vertexkey="1"><edges><edge vertexkey="2"/></edges></vertex>
            <vertex vertexkey="4"><edges><edge vertexkey="5"/></edges></vertex>
            <vertex vertexkey="7"><edges><edge vertexkey="8"/></edges></vertex>
            <vertex vertexkey="10"><edges><edge vertexkey="11"/></edges></vertex>
          </vertices></graph></structure></component></mapping>"#
    )
}

fn run(dir: &Path, design: &Path) -> (mapping::Project, Instance) {
    let imported = mfd::import(design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.root.iteration_output,
        IterationOutput::First
    );
    assert_eq!(imported.project.root.source(), Some([].as_slice()));
    assert!(imported.project.root.take.is_some());
    assert!(engine::validate(&imported.project).is_empty());
    let source =
        format_db::read_instance(&dir.join("inventory.sqlite"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    (imported.project, output)
}

#[test]
fn all_columns_descending_limit_one_imports_the_winner() {
    let dir = TempDir::new();
    let design = prepare(&dir.0, true);
    let (project, output) = run(&dir.0, &design);

    assert_eq!(
        output.field("Number").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        output.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Monitor".to_string()))
    );
    assert_eq!(
        output.field("SinglePrice").and_then(Instance::as_scalar),
        Some(&Value::Float(240.0))
    );
    let xml = format_xml::to_string(&project.target, &output).unwrap();
    assert!(xml.contains("<Name>Monitor</Name>"), "{xml}");
}

#[test]
fn an_empty_query_still_produces_the_empty_document_root() {
    let dir = TempDir::new();
    let design = prepare(&dir.0, false);
    let (project, output) = run(&dir.0, &design);

    assert!(matches!(output, Instance::Group(ref fields) if fields.is_empty()));
    let xml = format_xml::to_string(&project.target, &output).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "Article");
    assert!(root.children().all(|child| !child.is_element()));
}

#[test]
fn unsupported_limit_forms_fall_back_with_an_actionable_warning() {
    for limit in ["LIMIT 2", "LIMIT :count", "LIMIT 1 OFFSET 0"] {
        let dir = TempDir::new();
        let design = prepare(&dir.0, false);
        std::fs::write(&design, design_xml(limit)).unwrap();
        let imported = mfd::import(&design).unwrap();
        assert!(
            imported.warnings.iter().any(|warning| {
                warning.contains("unsupported inline query")
                    && (warning.contains("LIMIT") || warning.contains("OFFSET"))
            }),
            "{limit}: {:?}",
            imported.warnings
        );
    }
}

#[test]
fn inline_result_columns_must_match_the_expanded_projection() {
    let dir = TempDir::new();
    let design = prepare(&dir.0, false);
    let mismatched = design_xml("LIMIT 1").replace(
        "<entry name=\"Name\" outkey=\"7\"/>",
        "<entry name=\"Label\" outkey=\"7\"/>",
    );
    std::fs::write(&design, mismatched).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("unsupported inline query")
                && warning.contains("inline query output `Label`")
        }),
        "{:?}",
        imported.warnings
    );
}

#[test]
fn controls_after_sql_limit_one_are_rejected_instead_of_reordered() {
    let dir = TempDir::new();
    let design = prepare(&dir.0, true);
    let controlled = design_xml("LIMIT 1")
        .replace(
            "<component name=\"db\"",
            "<component name=\"filter\" library=\"core\" kind=\"3\"><sources><datapoint pos=\"0\" key=\"20\"/><datapoint pos=\"1\" key=\"21\"/></sources><targets><datapoint pos=\"0\" key=\"30\"/><datapoint/></targets></component><component name=\"db\"",
        )
        .replace(
            "<vertex vertexkey=\"1\"><edges><edge vertexkey=\"2\"/></edges></vertex>",
            "<vertex vertexkey=\"1\"><edges><edge vertexkey=\"20\"/></edges></vertex><vertex vertexkey=\"30\"><edges><edge vertexkey=\"2\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"5\"/></edges></vertex>",
            "<vertex vertexkey=\"4\"><edges><edge vertexkey=\"21\"/><edge vertexkey=\"5\"/></edges></vertex>",
        );
    std::fs::write(&design, controlled).unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("database LIMIT 1") && warning.contains("order cannot be represented")
        }),
        "{:?}",
        imported.warnings
    );
}
