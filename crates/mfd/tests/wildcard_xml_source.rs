use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-wildcard-xml-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

#[test]
fn imports_local_xml_wildcard_as_a_typed_file_set() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    write(
        &directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Value" type="xs:string"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    write(
        &directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Target"><xs:complexType><xs:sequence>
            <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Value" type="xs:string"/>
              <xs:element name="FileName" type="xs:string"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    write(
        &directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance" outkey="9"><entry name="document"><entry name="Source">
              <entry name="Item" outkey="10"><entry name="Value" outkey="11"/></entry>
            </entry></entry></entry></root>
            <document schema="source.xsd" inputinstance="records-*.xml" instanceroot="{}Source"/>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Target">
              <entry name="Item" inpkey="20"><entry name="Value" inpkey="21"/><entry name="FileName" inpkey="22"/></entry>
            </entry></entry></entry></root>
            <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
          <vertex vertexkey="9"><edges><edge vertexkey="22"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("records-*.xml")
    );
    assert!(imported.project.source_options.xml_document);
    assert!(imported.project.source_options.local_xml_file_set);
    assert!(!imported.project.target_options.local_xml_file_set);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::SourceDocumentPath))
    );
    write(
        &directory.0.join("records-b.xml"),
        "<Source><Item><Value>b</Value></Item></Source>",
    )?;
    write(
        &directory.0.join("records-a.xml"),
        "<Source><Item><Value>a</Value></Item></Source>",
    )?;
    let source = format_xml::read_local_file_set(
        &directory.0,
        Path::new("records-*.xml"),
        &imported.project.source,
        format_xml::LocalFileSetLimits::default(),
    )?;
    let output = engine::run(&imported.project, &source.instance)?;
    let items = output
        .field("Item")
        .and_then(Instance::as_repeated)
        .ok_or("missing mapped target items")?;
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].field("FileName").and_then(Instance::as_scalar),
        Some(&Value::String(
            source.paths[0].to_string_lossy().into_owned()
        ))
    );
    assert_eq!(
        items[1].field("FileName").and_then(Instance::as_scalar),
        Some(&Value::String(
            source.paths[1].to_string_lossy().into_owned()
        ))
    );

    let exported_path = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source_path, imported.project.source_path);
    assert!(roundtrip.project.source_options.local_xml_file_set);
    assert!(
        roundtrip
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::SourceDocumentPath))
    );
    Ok(())
}
