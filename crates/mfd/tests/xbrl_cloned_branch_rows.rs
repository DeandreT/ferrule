use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_cloned_rows_{}_{}",
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
    <xs:element name="Duration" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Start" type="xs:string"/><xs:element name="End" type="xs:string"/>
      <xs:element name="Label" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
    <xs:element name="Instant" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="At" type="xs:string"/><xs:element name="Label" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("cloned-rows.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Duration" outkey="10"><entry name="Start" outkey="11"/>
          <entry name="End" outkey="12"/><entry name="Label" outkey="13"/></entry>
        <entry name="Instant" outkey="20"><entry name="At" outkey="21"/>
          <entry name="Label" outkey="22"/></entry>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="date-from-datetime" library="lang" kind="5">
      <sources><datapoint pos="0" key="30"/></sources>
      <targets><datapoint pos="0" key="31"/></targets>
    </component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/><data>
        <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Report"><entry name="row" inpkey="100">
            <entry name="period"><entry name="startDate" inpkey="101"/>
              <entry name="endDate" inpkey="102"/></entry>
            <entry name="Label" inpkey="103"/>
          </entry></entry>
          <entry name="Report"><entry name="row" inpkey="200">
            <entry name="period"><entry name="instant" inpkey="201"/></entry>
            <entry name="Label" inpkey="202"/><entry name="BroadcastStart" inpkey="203"/>
          </entry></entry>
          <entry name="GeneratedOn" inpkey="204"/>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy/report.xsd" outputinstance="filing.xbrl"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="100"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="101"/><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="102"/></edges></vertex>
    <vertex vertexkey="13"><edges><edge vertexkey="103"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="200"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="201"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="202"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="203"/><edge vertexkey="204"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok(design)
}

fn scalar(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.to_string()))
}

fn source_instance() -> Instance {
    Instance::Group(vec![
        (
            "Duration".to_string(),
            Instance::Repeated(vec![Instance::Group(vec![
                ("Start".to_string(), scalar("2026-01-01")),
                ("End".to_string(), scalar("2026-03-31")),
                ("Label".to_string(), scalar("duration")),
            ])]),
        ),
        (
            "Instant".to_string(),
            Instance::Repeated(vec![Instance::Group(vec![
                ("At".to_string(), scalar("2026-03-31")),
                ("Label".to_string(), scalar("instant")),
            ])]),
        ),
    ])
}

fn field<'a>(instance: &'a Instance, name: &str) -> Option<&'a Instance> {
    let Instance::Group(fields) = instance else {
        return None;
    };
    fields
        .iter()
        .find_map(|(candidate, value)| (candidate == name).then_some(value))
}

fn text<'a>(instance: &'a Instance, name: &str) -> Option<&'a Value> {
    field(instance, name).and_then(Instance::as_scalar)
}

#[test]
fn cloned_xbrl_row_branches_keep_scalar_bindings_with_their_source_frame()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let output = engine::run(&imported.project, &source_instance())?;
    let report = field(&output, "Report").ok_or("missing Report output")?;
    let rows = field(report, "row").ok_or("missing row output")?;
    let Instance::Repeated(rows) = rows else {
        return Err("XBRL rows must remain a repeated sequence".into());
    };
    assert_eq!(rows.len(), 2, "{output:#?}");
    assert_eq!(
        text(&output, "GeneratedOn"),
        Some(&Value::String("2026-01-01".into()))
    );

    let duration_period = field(&rows[0], "period").ok_or("missing duration period")?;
    assert_eq!(
        text(duration_period, "startDate"),
        Some(&Value::String("2026-01-01".into()))
    );
    assert_eq!(
        text(duration_period, "endDate"),
        Some(&Value::String("2026-03-31".into()))
    );
    assert_eq!(text(duration_period, "instant"), None);
    assert_eq!(
        text(&rows[0], "Label"),
        Some(&Value::String("duration".into()))
    );

    let instant_period = field(&rows[1], "period").ok_or("missing instant period")?;
    assert_eq!(text(instant_period, "startDate"), None);
    assert_eq!(text(instant_period, "endDate"), None);
    assert_eq!(
        text(instant_period, "instant"),
        Some(&Value::String("2026-03-31".into()))
    );
    assert_eq!(
        text(&rows[1], "Label"),
        Some(&Value::String("instant".into()))
    );
    assert_eq!(
        text(&rows[1], "BroadcastStart"),
        Some(&Value::String("2026-01-01".into()))
    );
    Ok(())
}
