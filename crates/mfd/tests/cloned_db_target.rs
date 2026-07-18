use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_cloned_db_target_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let connection = Connection::open(dir.join("target.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE Parent (Id INTEGER PRIMARY KEY, Name TEXT); \
         CREATE TABLE Child (ParentId INTEGER, Value TEXT, \
           FOREIGN KEY (ParentId) REFERENCES Parent(Id));",
    )?;
    drop(connection);

    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="Parent"><xs:sequence>
    <xs:element name="Id" type="xs:integer"/><xs:element name="Name" type="xs:string"/>
    <xs:element name="Child" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType>
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="First" type="Parent" maxOccurs="unbounded"/>
    <xs:element name="Second" type="Parent" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><resources><datasources><datasource name="target-db">
  <database_connection database_kind="SQLite" import_kind="SQLite"
    ConnectionString="target.sqlite" name="target-db" path="target-db"/>
</datasource></datasources></resources><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source">
      <entry name="First" outkey="10"><entry name="Id" outkey="11"/><entry name="Name" outkey="12"/>
        <entry name="Child" outkey="13"><entry name="Value" outkey="14"/></entry></entry>
      <entry name="Second" outkey="40"><entry name="Id" outkey="41"/><entry name="Name" outkey="42"/>
        <entry name="Child" outkey="43"><entry name="Value" outkey="44"/></entry></entry>
    </entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="target" library="db" kind="15"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="document">
      <entry name="Parent" type="table" inpkey="20"><entry name="Id" inpkey="21"/><entry name="Name" inpkey="22"/>
        <entry name="Child|ParentId" type="table" inpkey="23"><entry name="ParentId" inpkey="24"/><entry name="Value" inpkey="25"/></entry></entry>
      <entry name="Parent" type="table" inpkey="30"><entry name="Id" inpkey="31"/><entry name="Name" inpkey="32"/>
        <entry name="Child|ParentId" type="table" inpkey="33"><entry name="ParentId" inpkey="34"/><entry name="Value" inpkey="35"/></entry></entry>
    </entry></root><database ref="target-db"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="23"/></edges></vertex>
  <vertex vertexkey="14"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="31"/><edge vertexkey="34"/></edges></vertex>
  <vertex vertexkey="42"><edges><edge vertexkey="32"/></edges></vertex>
  <vertex vertexkey="43"><edges><edge vertexkey="33"/></edges></vertex>
  <vertex vertexkey="44"><edges><edge vertexkey="35"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn cloned_relational_branches_distribute_nested_table_sequences()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");

    let parent = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Parent")
        .ok_or("missing Parent target scope")?;
    assert!(parent.children.is_empty());
    let segments = parent
        .concatenated()
        .ok_or("cloned Parent tables were not concatenated")?
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(segments.len(), 2);
    assert!(segments.iter().all(|segment| {
        segment
            .children
            .iter()
            .find(|child| child.target_field == "Child|ParentId")
            .is_some_and(mapping::Scope::iterates)
    }));
    Ok(())
}
