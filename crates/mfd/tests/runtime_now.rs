use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, RuntimeValue};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_runtime_now_{}_{}",
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

fn write_design(path: &Path, name: &str, library: &str) {
    let design = format!(
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document inputinstance="source.xml" instanceroot="{{}}Source"/></data></component>
          <component name="{name}" library="{library}" kind="5"><targets><datapoint pos="0" key="20"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="First" inpkey="30"/><entry name="Second" inpkey="31"/></entry></root><document outputinstance="target.xml" instanceroot="{{}}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="20"><edges><edge vertexkey="30"/><edge vertexkey="31"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    );
    std::fs::write(path, design).unwrap();
}

fn run(project: &mapping::Project) -> Instance {
    let execution = engine::ExecutionContext::new(Path::new("/maps/main.ferrule.json"))
        .with_current_datetime("2026-07-12T12:01:02.345-07:00");
    engine::run_with_context(project, &Instance::Group(Vec::new()), &execution).unwrap()
}

fn assert_now(output: &Instance) {
    for field in ["First", "Second"] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&Value::String("2026-07-12T12:01:02.345-07:00".into()))
        );
    }
}

#[test]
fn native_runtime_datetime_export_round_trips_both_import_spellings() {
    for (name, library) in [("now", "lang"), ("current-dateTime", "xpath2")] {
        let dir = TempDir::new();
        let design = dir.0.join("runtime-datetime.mfd");
        write_design(&design, name, library);

        let imported = mfd::import(&design).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{name}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty());
        assert_eq!(
            imported
                .project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(
                    node,
                    Node::RuntimeValue {
                        value: RuntimeValue::CurrentDateTime
                    }
                ))
                .count(),
            1,
            "{name}"
        );
        let original_output = run(&imported.project);
        assert_now(&original_output);

        let exported = dir.0.join("native-round-trip.mfd");
        let report = mfd::preflight_export(&imported.project, &exported).unwrap();
        assert!(report.is_native_compatible(), "{name}: {report:?}");
        assert!(!exported.exists());
        let published =
            mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd)
                .unwrap();
        assert!(published.is_native_compatible(), "{name}: {published:?}");
        let xml = std::fs::read_to_string(&exported).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let runtime_components = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component")
                    && matches!(node.attribute("name"), Some("now" | "current-dateTime"))
            })
            .collect::<Vec<_>>();
        let [runtime] = runtime_components.as_slice() else {
            panic!("{name}: expected one native date-time component");
        };
        assert_eq!(runtime.attribute("name"), Some("current-dateTime"));
        assert_eq!(runtime.attribute("library"), Some("xpath2"));
        assert_eq!(runtime.attribute("kind"), Some("5"));
        assert_eq!(
            runtime
                .descendants()
                .find(|node| node.has_tag_name("datapoint"))
                .and_then(|node| node.attribute("pos")),
            Some("0")
        );

        let reimported = mfd::import(&exported).unwrap();
        assert!(
            reimported.warnings.is_empty(),
            "{name}: {:?}",
            reimported.warnings
        );
        assert!(engine::validate(&reimported.project).is_empty());
        assert_eq!(run(&reimported.project), original_output, "{name}");
    }
}

#[test]
fn scalar_udfs_support_database_helpers_number_and_stable_now()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new();
    std::fs::write(
        dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Maybe" type="xs:string" minOccurs="0"/>
    <xs:element name="Amount" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Present" type="xs:boolean"/>
    <xs:element name="Amount" type="xs:decimal"/>
    <xs:element name="Timestamp" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.0.join("scalar-helpers.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26">
  <component name="map"><structure><children>
    <component name="source" library="xml" kind="14"><data>
      <root><entry name="Input"><entry name="Maybe" outkey="10"/><entry name="Amount" outkey="11"/></entry></root>
      <document schema="source.xsd" instanceroot="{}Input"/>
    </data></component>
    <component name="Normalize" library="helpers" kind="19"><data>
      <root><entry name="maybe" inpkey="20" componentid="100"/><entry name="amount" inpkey="21" componentid="101"/></root>
      <root rootindex="1"><entry name="present" outkey="22" componentid="105"/><entry name="amount" outkey="23" componentid="106"/><entry name="timestamp" outkey="24" componentid="107"/></root>
    </data></component>
    <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Present" inpkey="30"/><entry name="Amount" inpkey="31"/><entry name="Timestamp" inpkey="32"/></entry></root>
      <document schema="target.xsd" instanceroot="{}Output"/>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>
    <vertex vertexkey="24"><edges><edge vertexkey="32"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="Normalize" library="helpers" inline="1"><structure><children>
    <component name="maybe" library="core" uid="100" kind="6"><targets><datapoint key="1000"/></targets><data><input datatype="string"/><parameter usageKind="input" name="maybe"/></data></component>
    <component name="amount" library="core" uid="101" kind="6"><targets><datapoint key="1001"/></targets><data><input datatype="string"/><parameter usageKind="input" name="amount"/></data></component>
    <component name="is-not-null" library="db" uid="102" kind="5"><sources><datapoint key="1100"/></sources><targets><datapoint key="1101"/></targets></component>
    <component name="number" library="core" uid="103" kind="5"><sources><datapoint key="1102"/></sources><targets><datapoint key="1103"/></targets></component>
    <component name="now" library="lang" uid="104" kind="5"><targets><datapoint key="1104"/></targets></component>
    <component name="present" library="core" uid="105" kind="7"><sources><datapoint key="1200"/></sources><data><output datatype="boolean"/><parameter usageKind="output" name="present"/></data></component>
    <component name="amount" library="core" uid="106" kind="7"><sources><datapoint key="1201"/></sources><data><output datatype="decimal"/><parameter usageKind="output" name="amount"/></data></component>
    <component name="timestamp" library="core" uid="107" kind="7"><sources><datapoint key="1202"/></sources><data><output datatype="string"/><parameter usageKind="output" name="timestamp"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="1000"><edges><edge vertexkey="1100"/></edges></vertex>
    <vertex vertexkey="1001"><edges><edge vertexkey="1102"/></edges></vertex>
    <vertex vertexkey="1101"><edges><edge vertexkey="1200"/></edges></vertex>
    <vertex vertexkey="1103"><edges><edge vertexkey="1201"/></edges></vertex>
    <vertex vertexkey="1104"><edges><edge vertexkey="1202"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_xml::from_str(
        "<Input><Amount>12.5</Amount></Input>",
        &imported.project.source,
    )?;
    let execution = engine::ExecutionContext::new(&design)
        .with_current_datetime("2026-07-12T12:01:02.345-07:00");
    let output = engine::run_with_context(&imported.project, &source, &execution)?;
    assert_eq!(
        output.field("Present").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        output.field("Amount").and_then(Instance::as_scalar),
        Some(&Value::Float(12.5))
    );
    assert_eq!(
        output.field("Timestamp").and_then(Instance::as_scalar),
        Some(&Value::String("2026-07-12T12:01:02.345-07:00".into()))
    );
    Ok(())
}
