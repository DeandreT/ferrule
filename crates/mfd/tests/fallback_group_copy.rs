use std::error::Error;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-fallback-group-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn mapping(source_schema: Option<&str>, source_name: &str, target_name: &str) -> String {
    let source_schema = source_schema
        .map(|schema| format!(" schema=\"{schema}\""))
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="22">
  <component name="map">
    <structure>
      <children>
        <component name="source" library="xml" kind="14">
          <data>
            <root>
              <entry name="FileInstance"><entry name="document">
                <entry name="Inbound"><entry name="Container">
                  <entry name="{source_name}" outkey="10"/>
                </entry></entry>
              </entry></entry>
            </root>
            <document{source_schema} instanceroot="{{}}Inbound" inputinstance="input.xml"/>
          </data>
        </component>
        <component name="target" library="xml" kind="14">
          <properties XSLTDefaultOutput="1"/>
          <data>
            <root>
              <entry name="FileInstance"><entry name="document">
                <entry name="Output"><entry name="Container">
                  <entry name="{target_name}" inpkey="20"/>
                </entry></entry>
              </entry></entry>
            </root>
            <document schema="target.xsd" instanceroot="{{}}Output" outputinstance="output.xml"/>
          </data>
        </component>
      </children>
      <graph directed="1">
        <edges>
          <edge edgekey="1"><data><dataconnection type="2"/></data></edge>
        </edges>
        <vertices>
          <vertex vertexkey="10"><edges>
            <edge vertexkey="20" edgekey="1"/>
          </edges></vertex>
        </vertices>
      </graph>
    </structure>
  </component>
</mapping>
"#
    )
}

fn write_target_schema(path: &Path, group_name: &str) -> Result<(), std::io::Error> {
    write(
        path,
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output">
    <xs:complexType><xs:sequence>
      <xs:element name="Container"><xs:complexType><xs:sequence>
        <xs:element name="{group_name}"><xs:complexType><xs:sequence>
          <xs:element name="Line" minOccurs="0" maxOccurs="unbounded">
            <xs:complexType><xs:sequence>
              <xs:element name="Code" type="xs:string"/>
              <xs:element name="Quantity" type="xs:integer"/>
            </xs:sequence></xs:complexType>
          </xs:element>
        </xs:sequence></xs:complexType></xs:element>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>
"#
        ),
    )
}

#[test]
fn copy_all_recovers_a_same_named_group_lost_by_entry_tree_fallback() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new("copy")?;
    write(
        &directory.path("source.xsd"),
        r###"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Inbound">
    <xs:complexType><xs:sequence>
      <xs:any namespace="##any" processContents="strict"
              minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>
"###,
    )?;
    write_target_schema(&directory.path("target.xsd"), "Items")?;
    let design = directory.path("mapping.mfd");
    write(&design, &mapping(Some("source.xsd"), "Items", "Items"))?;

    let imported = mfd::import(&design)?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains("xs:any wildcard cannot be represented"),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported
            .warnings
            .iter()
            .all(|warning| !warning.contains("connection into non-repeating group")),
        "{:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty());

    let input_xml = "<Inbound><Container><Items>\
                     <Line><Code>A</Code><Quantity>2</Quantity></Line>\
                     <Line><Code>B</Code><Quantity>5</Quantity></Line>\
                     </Items></Container></Inbound>";
    let output_xml = "<Output><Container><Items>\
                      <Line><Code>A</Code><Quantity>2</Quantity></Line>\
                      <Line><Code>B</Code><Quantity>5</Quantity></Line>\
                      </Items></Container></Output>";
    let source = format_xml::from_str(input_xml, &imported.project.source)?;
    let expected = format_xml::from_str(output_xml, &imported.project.target)?;
    assert_eq!(engine::run(&imported.project, &source)?, expected);

    let roundtrip_design = directory.path("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &roundtrip_design)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let roundtrip = mfd::import(&roundtrip_design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source, imported.project.source);
    let source = format_xml::from_str(input_xml, &roundtrip.project.source)?;
    let expected = format_xml::from_str(output_xml, &roundtrip.project.target)?;
    assert_eq!(engine::run(&roundtrip.project, &source)?, expected);
    Ok(())
}

#[test]
fn mismatched_fallback_leaf_to_group_keeps_the_unsupported_warning() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("mismatch")?;
    write_target_schema(&directory.path("target.xsd"), "Record")?;
    let design = directory.path("mapping.mfd");
    write(&design, &mapping(None, "Payload", "Record"))?;

    let imported = mfd::import(&design)?;
    assert!(
        imported.warnings.iter().any(|warning| {
            warning == "connection into non-repeating group `Container/Record` ignored"
        }),
        "{:?}",
        imported.warnings
    );
    Ok(())
}
