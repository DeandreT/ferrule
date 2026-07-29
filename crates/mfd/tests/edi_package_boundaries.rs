use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::{EdiBoundaryKind, FormatOptions};

#[test]
fn package_manifest_resolves_all_edi_boundary_roles_after_relocation() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    let package = directory.path().join("original-package");
    write_package(&package)?;

    let original = import_package(&package)?;
    assert_compiled_boundaries(&original.project)?;

    let relocated = directory.path().join("relocated-package");
    std::fs::rename(&package, &relocated)?;

    let moved = import_package(&relocated)?;
    assert_compiled_boundaries(&moved.project)?;
    assert_eq!(moved.project.source, original.project.source);
    assert_eq!(moved.project.target, original.project.target);
    assert_eq!(
        moved.project.source_options,
        original.project.source_options
    );
    assert_eq!(
        moved.project.target_options,
        original.project.target_options
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn manifest_catalog_is_reconfined_when_options_are_reused() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new()?;
    let package = directory.path().join("package");
    write_package(&package)?;
    let manifest = package.join("ferrule-package.json");
    let options = mfd::ImportOptions::default().with_package_manifest(&manifest)?;
    let catalog = package.join("resources/edi");
    let outside = directory.path().join("outside");
    std::fs::rename(&catalog, &outside)?;
    symlink(&outside, &catalog)?;

    let error = match mfd::import_with_options(&package.join("maps/orders/mapping.mfd"), &options) {
        Ok(_) => return Err("manifest catalog symlink retargeting was accepted".into()),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("resolves outside package root"),
        "{error}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn selected_manifest_rejects_package_directory_retargeting() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new()?;
    let package = directory.path().join("package");
    write_package(&package)?;
    let manifest = package.join("ferrule-package.json");
    let options = mfd::ImportOptions::default().with_package_manifest(&manifest)?;
    let original = directory.path().join("original");
    std::fs::rename(&package, &original)?;
    let outside = directory.path().join("outside");
    write_package(&outside)?;
    symlink(&outside, &package)?;

    let error = match mfd::import_with_options(&package.join("maps/orders/mapping.mfd"), &options) {
        Ok(_) => return Err("retargeted package manifest identity was accepted".into()),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("package manifest identity changed"),
        "{error}"
    );
    Ok(())
}

fn import_package(package: &Path) -> Result<mfd::Imported, Box<dyn Error>> {
    let manifest = package.join("ferrule-package.json");
    let mapping = package.join("maps/orders/mapping.mfd");
    let options = mfd::ImportOptions::default().with_package_manifest(&manifest)?;
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.runtime_dependencies().is_empty());
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");
    Ok(imported)
}

fn assert_compiled_boundaries(project: &mapping::Project) -> Result<(), Box<dyn Error>> {
    assert_eq!(project.source_path.as_deref(), Some("orders.x12"));
    assert_eq!(project.target_path.as_deref(), Some("primary.x12"));
    assert_compiled_x12(&project.source_options);
    assert_schema_field(&project.source, "InboundValue")?;
    assert_compiled_x12(&project.target_options);
    assert_schema_field(&project.target, "PrimaryValue")?;

    let [acknowledgement] = project.extra_targets.as_slice() else {
        return Err("expected exactly one named EDI target".into());
    };
    assert_eq!(acknowledgement.name, "acknowledgement");
    assert_eq!(acknowledgement.path.as_deref(), Some("ack.x12"));
    assert_compiled_x12(&acknowledgement.options);
    assert_schema_field(&acknowledgement.schema, "AckValue")?;
    Ok(())
}

fn assert_compiled_x12(options: &FormatOptions) {
    assert_eq!(options.edi_kind, Some(EdiBoundaryKind::X12));
    assert!(
        options.edi_config_reference.is_none(),
        "compiled boundaries must not retain a runtime configuration dependency"
    );
}

fn assert_schema_field(schema: &ir::SchemaNode, field: &str) -> Result<(), Box<dyn Error>> {
    let exists = schema
        .child("Interchange")
        .and_then(|node| node.child("Group"))
        .and_then(|node| node.child("Message_850"))
        .and_then(|node| node.child("DAT"))
        .and_then(|node| node.child(field))
        .is_some();
    if !exists {
        return Err(format!("compiled EDI schema does not contain DAT/{field}").into());
    }
    Ok(())
}

fn write_package(package: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = package.join("resources/edi/SyntheticEdi");
    let maps = package.join("maps/orders");
    std::fs::create_dir_all(&catalog)?;
    std::fs::create_dir_all(&maps)?;
    write_x12_configuration(&catalog.join("Inbound"), "InboundValue")?;
    write_x12_configuration(&catalog.join("Primary"), "PrimaryValue")?;
    write_x12_configuration(&catalog.join("Ack"), "AckValue")?;
    std::fs::write(
        package.join("ferrule-package.json"),
        r#"{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [
    {"kind": "edi-config", "root": "resources\\edi"}
  ]
}"#,
    )?;
    std::fs::write(maps.join("mapping.mfd"), mapping_document())?;
    Ok(())
}

fn write_x12_configuration(directory: &Path, field: &str) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(directory)?;
    std::fs::write(
        directory.join("Defs.Segment"),
        format!(
            r#"<Config><Elements>
  <Data name="F1" type="string"/>
  <Data name="F143" type="string"/>
  <Data name="{field}" type="string"/>
  <Segment name="ISA"><Data ref="F1"/></Segment>
  <Segment name="GS"><Data ref="F1"/></Segment>
  <Segment name="ST"><Data ref="F143"/></Segment>
  <Segment name="DAT"><Data ref="{field}"/></Segment>
  <Segment name="SE"><Data ref="F1"/></Segment>
  <Segment name="GE"><Data ref="F1"/></Segment>
  <Segment name="IEA"><Data ref="F1"/></Segment>
</Elements></Config>"#
        ),
    )?;
    std::fs::write(
        directory.join("Envelope.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
  <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
    <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
      <Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/>
      <Segment ref="GE" minOccurs="0"/>
    </Group><Segment ref="IEA" minOccurs="0"/>
  </Group></Group>
</Config>"#,
    )?;
    std::fs::write(
        directory.join("850.Config"),
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
  <Message><MessageType>850</MessageType>
    <Group name="Message_850" maxOccurs="unbounded">
      <Segment ref="ST"/><Segment ref="DAT"/><Segment ref="SE"/>
    </Group>
  </Message>
</Config>"#,
    )
}

fn mapping_document() -> &'static str {
    r#"<mapping version="26"><resources/><component name="defaultmap" uid="1">
  <structure><children>
    <component name="orders" library="text" uid="2" kind="16"><properties/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
        <entry name="Interchange"><entry name="Group"><entry name="Message_850">
          <entry name="DAT"><entry name="InboundValue" outkey="10"/></entry>
        </entry></entry></entry>
      </entry></entry></entry></root>
      <text type="edi" kind="EDIX12"
            config="..\SyntheticEdi\Inbound\Envelope.Config"
            inputinstance="orders.x12">
        <messages><message type="850"/></messages>
      </text>
    </data></component>
    <component name="primary" library="text" uid="3" kind="16">
      <properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
        <entry name="Interchange"><entry name="Group"><entry name="Message_850">
          <entry name="DAT"><entry name="PrimaryValue" inpkey="20"/></entry>
        </entry></entry></entry>
      </entry></entry></entry></root>
      <text type="edi" kind="EDIX12"
            config="SyntheticEdi/Primary/Envelope.Config"
            outputinstance="primary.x12">
        <messages><message type="850"/></messages>
      </text>
    </data></component>
    <component name="acknowledgement" library="text" uid="4" kind="16"><properties/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
        <entry name="Interchange"><entry name="Group"><entry name="Message_850">
          <entry name="DAT"><entry name="AckValue" inpkey="30"/></entry>
        </entry></entry></entry>
      </entry></entry></entry></root>
      <text type="edi" kind="EDIX12"
            config="..\SyntheticEdi\Ack\Envelope.Config"
            outputinstance="ack.x12">
        <messages><message type="850"/></messages>
      </text>
    </data></component>
  </children><graph directed="1"><vertices>
    <vertex vertexkey="10"><edges>
      <edge vertexkey="20"/><edge vertexkey="30"/>
    </edges></vertex>
  </vertices></graph></structure>
</component></mapping>"#
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-edi-package-boundaries-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
