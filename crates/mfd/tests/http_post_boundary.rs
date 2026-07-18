use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{ExternalPayloadFormat, ExternalSourceOrigin};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_http_post_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Prompt" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Answer" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="input" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Prompt" outkey="10"/></entry></root>
    <document schema="source.xsd" inputinstance="input.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="analyze" library="webservice" kind="20"><data>
    <root><entry name="HTTPMessage"><entry name="HTTPBody">
      <entry name="document" type="doc-json"><document encoding="UTF-8"/>
        <entry name="root"><entry name="object">
          <entry name="query" type="json-property"><entry name="string" inpkey="20"/></entry>
        </entry></entry>
      </entry>
    </entry></entry></root>
    <root rootindex="1"><entry name="HTTPMessage"><entry name="HTTPBody">
      <entry name="document" type="doc-json"><document encoding="UTF-8"/>
        <entry name="root"><entry name="object">
          <entry name="answer" type="json-property"><entry name="string" outkey="30"/></entry>
        </entry></entry>
      </entry>
    </entry></entry></root>
    <wsdl kind="call" sourceMode="manual" url="https://example.test/analyze"
      timeout="20" httpmethod="POST">
      <parameter name="Authorization" value="must-not-be-retained" style="header"
        required="1" mappable="1"/>
    </wsdl>
  </data></component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint key="31"/></targets>
    <data><constant value="2" datatype="decimal"/></data>
  </component>
  <component name="sleep" library="lang" kind="5">
    <sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources>
    <targets><datapoint pos="0" key="42"/></targets>
  </component>
  <component name="output" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Output"><entry name="Answer" inpkey="50"/></entry></root>
      <document schema="target.xsd" outputinstance="output.xml" instanceroot="{}Output"/>
    </data>
  </component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="42"><edges><edge vertexkey="50"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn write_legacy_copy_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(dir.join("any.schema.json"), "{}")?;
    let design = dir.join("legacy-copy.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source" outkey="10"><entry name="Value" outkey="11"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="preview" library="json" kind="31">
    <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="root">
        <entry name="object" inpkey="20"/>
      </entry></entry></entry></root>
      <json schema="any.schema.json" outputinstance="preview.json"/>
    </data>
  </component>
</children></structure><connections>
  <edge from="10" to="20"><data type="2"/></edge>
</connections></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn post_response_is_an_executable_captured_json_boundary() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    assert!(imported.project.source_options.external_source.is_none());
    let response_source = imported
        .project
        .extra_sources
        .iter()
        .find(|source| source.options.external_source.is_some())
        .ok_or("POST response was not retained as a named source")?;
    let boundary = response_source
        .options
        .external_source
        .as_ref()
        .ok_or("POST response source has no external boundary metadata")?;
    assert_eq!(boundary.payload(), ExternalPayloadFormat::Json);
    let ExternalSourceOrigin::HttpPost {
        request_format,
        request_schema,
        headers,
        ..
    } = boundary.origin()
    else {
        return Err("expected an HTTP POST boundary".into());
    };
    assert_eq!(*request_format, Some(ExternalPayloadFormat::Json));
    assert!(request_schema.is_some());
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].name(), "Authorization");
    assert!(headers[0].required());
    assert!(headers[0].mapped());
    let encoded = serde_json::to_string(&imported.project)?;
    assert!(!encoded.contains("must-not-be-retained"));

    let response = format_json::from_str(r#"{"answer":"ready"}"#, &response_source.schema)?;
    let input = format_xml::from_str(
        "<Input><Prompt>status</Prompt></Input>",
        &imported.project.source,
    )?;
    let target = engine::run_with_sources(
        &imported.project,
        &input,
        vec![(response_source.name.clone(), response)],
    )?;
    assert_eq!(
        target.field("Answer").and_then(Instance::as_scalar),
        Some(&Value::String("ready".into()))
    );

    let exported = dir.0.join("unsupported.mfd");
    assert!(matches!(
        mfd::export(&imported.project, &exported),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("captured external response in secondary source")
    ));
    assert!(!exported.exists());
    Ok(())
}

#[test]
fn legacy_copy_all_refines_an_unconstrained_json_preview() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_legacy_copy_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target, imported.project.source);
    assert_eq!(
        imported.project.root.construction,
        mapping::ScopeConstruction::CopyCurrentSource
    );
    assert!(engine::validate(&imported.project).is_empty());

    let source = format_xml::from_str(
        "<Source><Value>preserved</Value></Source>",
        &imported.project.source,
    )?;
    assert_eq!(engine::run(&imported.project, &source)?, source);
    Ok(())
}
