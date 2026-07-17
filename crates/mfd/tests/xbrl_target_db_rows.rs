use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::Value;
use mapping::{Node, Scope};
use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_db_rows_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_fixture(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let connection = Connection::open(dir.join("facts.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE Facts (Date TEXT, Amount INTEGER); \
         INSERT INTO Facts VALUES ('2026-01-31', 10), ('2026-02-28', 20);",
    )?;
    drop(connection);

    let design = dir.join("db-target.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26">
  <resources><datasources><datasource name="facts">
    <database_connection database_kind="SQLite" import_kind="SQLite"
      ConnectionString="facts.sqlite" name="facts" path="facts"/>
  </datasource></datasources></resources>
  <component name="map"><structure><children>
    <component name="facts" library="db" kind="15"><data>
      <root><entry name="document"><entry name="Facts" type="table" outkey="10">
        <entry name="Date" outkey="11"/><entry name="Amount" outkey="12"/>
      </entry></entry></root><database ref="facts"/>
    </data></component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="30"/></targets>
      <data><constant value="Example Corp" datatype="string"/></data>
    </component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="31"/></targets>
      <data><constant value="https://example.test/entity" datatype="string"/></data>
    </component>
    <component name="filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/><data>
        <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Statement"><entry name="identifier">
            <entry name="identifier" inpkey="20">
              <entry name="scheme" type="attribute" inpkey="21"/>
            </entry>
            <entry name="period" inpkey="22"><entry name="instant" inpkey="23"/></entry>
            <entry name="context" inpkey="24"><entry name="Amount" inpkey="25"/></entry>
          </entry></entry>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy/report.xsd"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="24"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="22"/><edge vertexkey="23"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="25"/></edges></vertex>
    <vertex vertexkey="30"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="21"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok(design)
}

fn child_scope<'a>(scope: &'a Scope, field: &str) -> Result<&'a Scope, Box<dyn Error>> {
    scope
        .children
        .iter()
        .find(|child| child.target_field == field)
        .ok_or_else(|| format!("missing target scope `{field}`").into())
}

#[test]
fn xbrl_row_uses_database_collection_without_lifting_attribute_feed() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    let normalized_row = imported
        .project
        .target
        .child("Statement")
        .and_then(|statement| statement.child("identifier"))
        .ok_or("missing normalized XBRL row")?;
    assert!(normalized_row.repeating, "{normalized_row:#?}");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let statement = child_scope(&imported.project.root, "Statement")?;
    let row = child_scope(statement, "identifier")?;
    assert_eq!(row.source(), Some(&[][..]));

    let identifier = child_scope(row, "identifier")?;
    let scheme = identifier
        .bindings
        .iter()
        .find(|binding| binding.target_field == "scheme")
        .ok_or("missing identifier scheme binding")?;
    assert!(matches!(
        imported.project.graph.nodes.get(&scheme.node),
        Some(Node::Const {
            value: Value::String(value)
        }) if value == "https://example.test/entity"
    ));
    Ok(())
}
