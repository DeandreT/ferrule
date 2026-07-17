use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, SchemaKind, Value};
use mapping::XBRL_UNIT_FIELD_PREFIX;

const XBRLI: &str = "http://www.xbrl.org/2003/instance";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_unit_aliases_{}_{}",
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
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Record" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Entity" type="xs:string"/>
      <xs:element name="Scheme" type="xs:string"/>
      <xs:element name="Instant" type="xs:string"/>
      <xs:element name="Label" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("units.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Record" outkey="10">
          <entry name="Entity" outkey="11"/><entry name="Scheme" outkey="12"/>
          <entry name="Instant" outkey="13"/><entry name="Label" outkey="14"/>
        </entry>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="40"/></targets>
      <data><constant value="USD" datatype="string"/></data>
    </component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="41"/></targets>
      <data><constant value="{http://www.xbrl.org/2003/iso4217}USD" datatype="string"/></data>
    </component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="42"/></targets>
      <data><constant value="shares" datatype="string"/></data>
    </component>
    <component name="constant" library="core" kind="2">
      <targets><datapoint pos="0" key="43"/></targets>
      <data><constant value="{http://www.xbrl.org/2003/instance}shares" datatype="string"/></data>
    </component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/><data>
        <root><header><namespaces>
          <namespace uid=""/>
          <namespace uid="http://www.xbrl.org/2003/instance"/>
          <namespace uid="urn:ferrule:test:facts"/>
        </namespaces></header><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="unit"><entry name="id" type="attribute" inpkey="20"/><entry name="measure" inpkey="21"/></entry>
          <entry name="unit"><entry name="id" type="attribute" inpkey="22"/><entry name="measure" inpkey="23"/></entry>
          <entry name="Report"><entry name="row" inpkey="30">
            <entry name="identifier" ns="1" inpkey="31"><entry name="scheme" type="attribute" inpkey="32"/></entry>
            <entry name="period" ns="1"><entry name="instant" inpkey="33"/></entry>
            <entry name="Label" ns="2" inpkey="34"/>
          </entry></entry>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy/report.xsd" outputinstance="filing.xbrl"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="32"/></edges></vertex>
    <vertex vertexkey="13"><edges><edge vertexkey="33"/></edges></vertex>
    <vertex vertexkey="14"><edges><edge vertexkey="34"/></edges></vertex>
    <vertex vertexkey="40"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="41"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>
    <vertex vertexkey="43"><edges><edge vertexkey="23"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok(design)
}

fn source_instance() -> Instance {
    let scalar = |value: &str| Instance::Scalar(Value::String(value.to_string()));
    Instance::Group(vec![(
        "Record".to_string(),
        Instance::Repeated(vec![Instance::Group(vec![
            ("Entity".to_string(), scalar("Example Corp")),
            ("Scheme".to_string(), scalar("urn:ferrule:test:entity")),
            ("Instant".to_string(), scalar("2026-06-30")),
            ("Label".to_string(), scalar("reported")),
        ])]),
    )])
}

#[test]
fn distinct_top_level_units_survive_import_execution_and_xbrl_output() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let SchemaKind::Group { children, .. } = &imported.project.target.kind else {
        return Err("XBRL target root must remain a group".into());
    };
    let unit_aliases = children
        .iter()
        .filter(|child| child.name.starts_with(XBRL_UNIT_FIELD_PREFIX))
        .map(|child| child.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(unit_aliases.len(), 2, "{:#?}", imported.project.target);
    assert_ne!(unit_aliases[0], unit_aliases[1]);

    let output = engine::run(&imported.project, &source_instance())?;
    let options = imported
        .project
        .target_options
        .xbrl
        .as_ref()
        .ok_or("missing imported XBRL target options")?;
    let xml = format_xbrl::to_string(&imported.project.target, &output, options)?;
    assert!(!xml.contains(XBRL_UNIT_FIELD_PREFIX));

    let document = roxmltree::Document::parse(&xml)?;
    let units = document
        .descendants()
        .filter(|node| node.has_tag_name((XBRLI, "unit")))
        .collect::<Vec<_>>();
    assert_eq!(units.len(), 2, "{xml}");
    assert_eq!(units[0].attribute("id"), Some("USD"));
    assert_eq!(units[1].attribute("id"), Some("shares"));
    let measure = |unit: &roxmltree::Node<'_, '_>| {
        unit.descendants()
            .find(|node| node.has_tag_name((XBRLI, "measure")))
            .and_then(|node| node.text())
            .map(str::to_string)
    };
    assert_eq!(measure(&units[0]).as_deref(), Some("iso4217:USD"));
    assert_eq!(measure(&units[1]).as_deref(), Some("xbrli:shares"));
    Ok(())
}
