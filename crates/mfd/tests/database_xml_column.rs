use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;
use rusqlite::Connection;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_database_xml_column_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn payloads(output: &Instance) -> Vec<&str> {
    output
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            let value = row.field("payload").and_then(Instance::as_scalar).unwrap();
            let Value::String(value) = value else {
                panic!("expected XML text, got {value:?}");
            };
            value.as_str()
        })
        .collect()
}

fn write_packaged_mapping(
    package: &Path,
    database_xml_schema: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let maps = package.join("maps/orders");
    let schemas = package.join("schemas");
    let data = package.join("data");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&schemas)?;
    std::fs::create_dir_all(&data)?;
    std::fs::write(
        schemas.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Catalog"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence><xs:attribute name="sku" type="xs:string"/></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        schemas.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Item"><xs:complexType><xs:sequence>
    <xs:element name="Name" type="xs:string"/>
  </xs:sequence><xs:attribute name="sku" type="xs:string"/></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let database = data.join("inventory.sqlite");
    let connection = Connection::open(&database)?;
    connection.execute("CREATE TABLE Inventory (payload TEXT NOT NULL)", [])?;
    drop(connection);

    let mapping = maps.join("mapping.mfd");
    let design = r#"<mapping><resources><datasources><datasource name="inventory"><database_connection name="inventory" ConnectionString="..\..\data\inventory.sqlite" database_kind="SQLite" import_kind="SQLite"/></datasource></datasources></resources>
<component name="map"><structure><children>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="document"><entry name="Catalog" outkey="10"><entry name="Item" outkey="11"/></entry></entry></root><document schema="..\..\schemas\source.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="inventory" library="db" kind="15"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="Inventory" type="table" inpkey="20"><entry name="payload"><entry name="document" type="doc-xml"><document schemafile="DATABASE_XML_SCHEMA" root="Item" encoding="UTF-8"/><entry name="Item" inpkey="21"/></entry></entry></entry></entry></root><database ref="inventory"/></data></component>
</children><graph><edges><edge edgekey="100"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/><edge vertexkey="21" edgekey="100"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
        .replace("DATABASE_XML_SCHEMA", database_xml_schema);
    std::fs::write(&mapping, design)?;
    Ok(mapping)
}

#[test]
fn embedded_xml_database_columns_execute_compactly_and_round_trip() {
    let dir = TempDir::new();
    write(
        &dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Item"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/></xs:sequence><xs:attribute name="sku" type="xs:string"/></xs:complexType></xs:element>
  <xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element ref="Item" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    let database = dir.0.join("inventory.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute("CREATE TABLE Inventory (payload TEXT NOT NULL)", [])
        .unwrap();
    drop(connection);
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping><resources><datasources><datasource name="inventory"><database_connection name="inventory" ConnectionString="inventory.sqlite" database_kind="SQLite" import_kind="SQLite"/></datasource></datasources></resources>
<component name="map"><structure><children>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="document"><entry name="Catalog" outkey="10"><entry name="Item" outkey="11"/></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="inventory" library="db" kind="15"><properties XSLTDefaultOutput="1"/><data><root><entry name="document"><entry name="Inventory" type="table" inpkey="20"><entry name="payload"><entry name="document" type="doc-xml"><document schemafile="catalog.xsd" root="Item" encoding="UTF-8"/><entry name="Item" inpkey="21"/></entry></entry></entry></entry></root><database ref="inventory"/></data></component>
</children><graph><edges><edge edgekey="100"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/><edge vertexkey="21" edgekey="100"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::XmlSerialize {
            declaration: false,
            indent: false,
            ..
        }
    )));
    let input = format_xml::from_str(
        r#"<Catalog><Item sku="A-1"><Name>Alpha &amp; Beta</Name></Item><Item sku="B-2"><Name>Gamma</Name></Item></Catalog>"#,
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    assert_eq!(
        payloads(&output),
        [
            r#"<Item sku="A-1"><Name>Alpha &amp; Beta</Name></Item>"#,
            r#"<Item sku="B-2"><Name>Gamma</Name></Item>"#,
        ]
    );
    format_db::write_instance(&database, &imported.project.target, &output).unwrap();
    let connection = Connection::open(&database).unwrap();
    let stored = connection
        .prepare("SELECT payload FROM Inventory ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(stored, payloads(&output));

    let exported_path = dir.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    assert!(exported.contains(r#"ferrule-indent="0""#));
    let reimported = mfd::import(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let roundtrip = engine::run(&reimported.project, &input).unwrap();
    assert_eq!(payloads(&roundtrip), payloads(&output));
}

#[test]
fn relocated_package_resolves_windows_parent_database_xml_schema_and_executes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new();
    let original = directory.0.join("original");
    let mapping = write_packaged_mapping(&original, r"..\..\schemas\catalog.xsd")?;
    let relocated = directory.0.join("relocated");
    std::fs::rename(&original, &relocated)?;
    let relocated_mapping = relocated.join(mapping.strip_prefix(&original)?);
    let options = mfd::ImportOptions::default().with_package_root(&relocated);

    let imported = mfd::import_with_options(&relocated_mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    std::fs::remove_dir_all(relocated.join("schemas"))?;
    let input = format_xml::from_str(
        r#"<Catalog><Item sku="A-1"><Name>Portable</Name></Item></Catalog>"#,
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &input)?;
    assert_eq!(
        payloads(&output),
        [r#"<Item sku="A-1"><Name>Portable</Name></Item>"#]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn database_xml_schema_symlink_cannot_escape_selected_package()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new();
    let package = directory.0.join("package");
    let mapping = write_packaged_mapping(&package, r"..\..\schemas\catalog.xsd")?;
    let outside = directory.0.join("outside.xsd");
    std::fs::write(
        &outside,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Item" type="xs:string"/></xs:schema>"#,
    )?;
    let linked = package.join("schemas/catalog.xsd");
    std::fs::remove_file(&linked)?;
    symlink(&outside, &linked)?;
    let options = mfd::ImportOptions::default().with_package_root(&package);

    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("database component `inventory` XML column `payload` is unsupported")
            && warning.contains("database XML column schema")
            && warning.contains("resolves outside package root")
    }));
    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::XmlSerialize { .. }))
    );
    Ok(())
}
