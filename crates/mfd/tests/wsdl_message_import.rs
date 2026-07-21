use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_wsdl_message_{}_{}",
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

fn write_design(directory: &Path) -> Result<PathBuf, std::io::Error> {
    let design = directory.join("lookup.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="30"><component name="lookup-map">
<properties WSDLFile="inventory.wsdl" WSDLService="{urn:inventory}InventoryService" WSDLPort="InventoryPort" WSDLOperation="{urn:inventory}Lookup"/>
<structure><children>
  <component name="request" library="wsdl" kind="17"><data>
    <root><entry name="LookupRequest"><entry name="SearchText" outkey="10"/></entry></root>
    <wsdl previewRequestInstanceFile="lookup-request.xml"/>
  </data></component>
  <component name="response" library="wsdl" kind="17"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="LookupResponse"><entry name="MatchedText" inpkey="20"/></entry></root>
    <wsdl kind="output"/>
  </data></component>
  <component name="not-found" library="wsdl" kind="17"><data>
    <root><entry name="LookupFault"><entry name="Reason" inpkey="30"/></entry></root>
    <wsdl kind="fault" faultName="LookupFault"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn mapped_value<'a>(instance: &'a Instance, field: &str) -> Option<&'a Value> {
    instance.field(field).and_then(Instance::as_scalar)
}

#[test]
fn wsdl_messages_import_as_executable_xml_boundaries_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("lookup-request.xml")
    );
    assert!(imported.project.source_options.xml_document);
    assert!(imported.project.target_options.xml_document);
    assert_eq!(
        imported
            .project
            .source_options
            .wsdl
            .as_ref()
            .map(mapping::WsdlMessageOptions::contract_file),
        Some("inventory.wsdl")
    );
    assert_eq!(
        imported
            .project
            .target_options
            .wsdl
            .as_ref()
            .map(mapping::WsdlMessageOptions::role),
        Some(mapping::WsdlMessageRole::Response)
    );
    assert!(imported.project.extra_targets.is_empty());
    assert!(engine::validate(&imported.project).is_empty());

    let input = format_xml::from_str(
        "<LookupRequest><SearchText>copper</SearchText></LookupRequest>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &input)?;
    assert_eq!(
        mapped_value(&output, "MatchedText"),
        Some(&Value::String("copper".to_string()))
    );
    let output_xml = format_xml::to_string(&imported.project.target, &output)?;
    assert!(output_xml.contains("<LookupResponse>"), "{output_xml}");
    assert!(
        output_xml.contains("<MatchedText>copper</MatchedText>"),
        "{output_xml}"
    );

    let exported = directory.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported_text = std::fs::read_to_string(&exported)?;
    assert!(exported_text.contains("WSDLFile=\"inventory.wsdl\""));
    assert!(exported_text.contains("WSDLService=\"{urn:inventory}InventoryService\""));
    assert_eq!(exported_text.matches("library=\"wsdl\"").count(), 2);
    assert!(exported_text.contains("kind=\"output\""));
    assert!(exported_text.contains("previewRequestInstanceFile=\"lookup-request.xml\""));
    assert!(!directory.0.join("roundtrip-source.xsd").exists());
    assert!(!directory.0.join("roundtrip-target.xsd").exists());
    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        reimported.project.source_options.wsdl,
        imported.project.source_options.wsdl
    );
    assert_eq!(
        reimported.project.target_options.wsdl,
        imported.project.target_options.wsdl
    );
    assert_eq!(engine::run(&reimported.project, &input)?, output);

    let mut nonstandard_preview = imported.project.clone();
    nonstandard_preview.source_path = Some("lookup-request.json".to_string());
    let nonstandard_design = directory.0.join("nonstandard-preview.mfd");
    mfd::export(&nonstandard_preview, &nonstandard_design)?;
    let nonstandard_text = std::fs::read_to_string(nonstandard_design)?;
    assert!(nonstandard_text.contains("library=\"wsdl\""));
    assert!(nonstandard_text.contains("previewRequestInstanceFile=\"lookup-request.json\""));
    Ok(())
}
