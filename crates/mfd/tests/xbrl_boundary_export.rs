use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::{XbrlBoundaryMode, XbrlBoundaryOptions};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_export_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

fn source_fixture(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    write(
        &directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Amount" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let path = directory.join("source.mfd");
    write(
        &path,
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Facts" library="xbrl" kind="27"><data>
      <root><header><namespaces><namespace/><namespace uid="urn:example"/></namespaces></header>
        <entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Report" outkey="10"><entry name="Amount" ns="1" outkey="11"/></entry>
        </entry></entry></entry>
      </root>
      <xbrl schema="taxonomy/source.xsd" inputinstance="input/facts.xbrl"/>
    </data></component>
    <component name="Target" library="xml" kind="14">
      <properties XSLTDefaultOutput="1"/>
      <data><root><entry name="FileInstance"><entry name="document"><entry name="Target">
        <entry name="Amount" inpkey="20"/>
      </entry></entry></entry></root>
      <document schema="target.xsd" outputinstance="out.xml" instanceroot="{}Target"/></data>
    </component>
  </children></structure><connections><edge from="11" to="20"/></connections></component>
</mapping>"#,
    )?;
    Ok(path)
}

fn target_fixture(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::create_dir_all(directory.join("presentation"))?;
    write(
        &directory.join("presentation/table.sps"),
        r#"<structure><schemasources><namespaces><nspair prefix="ex" uri="urn:example"/></namespaces></schemasources><template subtype="xbrl-concept-aspect" match="ex:Amount"><children><calltemplate subtype="named" match="monetaryItemType"/></children></template></structure>"#,
    )?;
    write(
        &directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
    <xs:element name="Scheme" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let path = directory.join("target.mfd");
    write(
        &path,
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Value" outkey="10"/><entry name="Scheme" outkey="11"/>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="input.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/>
      <data><root><header><namespaces><namespace/><namespace uid="urn:example"/></namespaces></header>
        <entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Report"><entry name="Amount" ns="1" inpkey="20"/>
            <entry name="identifier" inpkey="21"><entry name="scheme" type="attribute" inpkey="22"/></entry>
          </entry>
        </entry></entry></entry>
      </root>
      <xbrl schema="taxonomy/target.xsd" sps="presentation/table.sps" outputinstance="filing.xbrl"/></data>
    </component>
  </children></structure><connections>
    <edge from="10" to="20"/><edge from="10" to="21"/><edge from="11" to="22"/>
  </connections></component>
</mapping>"#,
    )?;
    Ok(path)
}

#[test]
fn xbrl_source_boundary_exports_and_reimports_without_warnings() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&source_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let original = imported
        .project
        .source_options
        .xbrl
        .clone()
        .ok_or_else(|| std::io::Error::other("missing source XBRL options"))?;

    let exported_path = dir.0.join("source-roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(exported.contains("library=\"xbrl\""));
    assert!(exported.contains("schema=\"taxonomy/source.xsd\""));
    assert!(exported.contains("inputinstance=\"input/facts.xbrl\""));

    let reimported = mfd::import(&exported_path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        reimported.project.source_options.xbrl.as_ref(),
        Some(&original)
    );
    Ok(())
}

#[test]
fn xbrl_target_boundary_regenerates_fact_metadata_and_simple_content() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&target_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let original = imported
        .project
        .target_options
        .xbrl
        .clone()
        .ok_or_else(|| std::io::Error::other("missing target XBRL options"))?;
    assert_eq!(original.mode(), XbrlBoundaryMode::ExternalTarget);
    assert_eq!(original.fact_bindings().len(), 1);

    let output = dir.0.join("roundtrip");
    std::fs::create_dir_all(output.join("presentation"))?;
    let exported_path = output.join("target-roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(output.join("presentation/table.sps").exists());

    let reimported = mfd::import(&exported_path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        reimported.project.target_options.xbrl.as_ref(),
        Some(&original)
    );
    let identifier = reimported
        .project
        .target
        .child("Report")
        .and_then(|report| report.child("identifier"))
        .ok_or_else(|| std::io::Error::other("missing identifier simple content"))?;
    assert!(identifier.text_child().is_some());
    assert!(
        identifier
            .child("scheme")
            .is_some_and(|child| child.attribute)
    );
    Ok(())
}

#[test]
fn unsafe_xbrl_presentation_path_rejects_without_publishing() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let mut imported = mfd::import(&target_fixture(&dir.0)?)?.project;
    let original = imported
        .target_options
        .xbrl
        .take()
        .ok_or_else(|| std::io::Error::other("missing target XBRL options"))?;
    imported.target_options.xbrl = Some(
        XbrlBoundaryOptions::external_target(original.taxonomy(), Some("../outside.sps"))?
            .with_namespace_bindings(original.namespace_bindings().to_vec())?
            .with_fact_bindings(original.fact_bindings().to_vec())?,
    );
    let design = dir.0.join("preserved.mfd");
    write(&design, "preserve")?;

    let result = mfd::export(&imported, &design);
    assert!(matches!(
        result,
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("not a bounded relative path")
    ));
    assert_eq!(std::fs::read_to_string(design)?, "preserve");
    assert!(!dir.0.join("outside.sps").exists());
    Ok(())
}
