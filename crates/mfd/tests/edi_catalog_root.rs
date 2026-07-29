use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::EdiBoundaryKind;

#[test]
fn trusted_catalog_roots_resolve_in_order_with_portable_paths() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("ordered")?;
    let mapping_directory = directory.path().join("maps");
    let empty_catalog = directory.path().join("empty-catalog");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&mapping_directory)?;
    std::fs::create_dir_all(&empty_catalog)?;
    write_catalog(&catalog, VALID_FIELD)?;
    let mapping = mapping_directory.join("mapping.mfd");
    write_mapping(&mapping, r"altova://edi_config/Custom.X12\Envelope.Config")?;

    let options = mfd::ImportOptions::default()
        .with_edi_catalog_root(&empty_catalog)
        .with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.runtime_dependencies().is_empty());
    assert_eq!(
        imported.project.source_options.edi_kind,
        Some(EdiBoundaryKind::X12)
    );
    assert!(
        imported
            .project
            .source
            .child("Interchange")
            .and_then(|node| node.child("Group"))
            .and_then(|node| node.child("Message_850"))
            .and_then(|node| node.child("BEG"))
            .and_then(|node| node.child(VALID_FIELD))
            .is_some()
    );
    assert!(engine::validate(&imported.project).is_empty());
    Ok(())
}

#[test]
fn package_configuration_precedes_catalog_configuration() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("package-first")?;
    let package = directory.path().join("package");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&package)?;
    write_catalog(&package, VALID_FIELD)?;
    write_broken_catalog(&catalog)?;
    let mapping = package.join("mapping.mfd");
    write_mapping(&mapping, r"Custom.X12\Envelope.Config")?;

    let options = mfd::ImportOptions::default()
        .with_package_root(&package)
        .with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported
            .project
            .source
            .child("Interchange")
            .and_then(|node| node.child("Group"))
            .and_then(|node| node.child("Message_850"))
            .and_then(|node| node.child("BEG"))
            .and_then(|node| node.child(VALID_FIELD))
            .is_some()
    );
    Ok(())
}

#[test]
fn catalog_lookup_clamps_leading_installation_parent_components() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("leading-parent")?;
    let mapping_directory = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&mapping_directory)?;
    write_catalog(&catalog.join("PortableEdiX12_lib"), VALID_FIELD)?;
    let mapping = mapping_directory.join("mapping.mfd");
    write_mapping(
        &mapping,
        r"..\PortableEdiX12_lib\Custom.X12\Envelope.Config",
    )?;

    let options = mfd::ImportOptions::default().with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.runtime_dependencies().is_empty());
    Ok(())
}

#[test]
fn catalog_lookup_rejects_virtual_root_escape() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("traversal")?;
    let mapping_directory = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&mapping_directory)?;
    write_catalog(&catalog, VALID_FIELD)?;
    let mapping = mapping_directory.join("mapping.mfd");
    write_mapping(&mapping, "../nested/../../Custom.X12/Envelope.Config")?;

    let options = mfd::ImportOptions::default().with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert_eq!(imported.project.runtime_dependencies().len(), 1);
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("could not compile external configuration")
            && warning.contains("parent traversal escapes the virtual catalog root")
    }));
    Ok(())
}

#[test]
fn trusted_catalog_resolves_adjacent_zip_package() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("archive")?;
    let mapping_directory = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&mapping_directory)?;
    write_catalog_zip(&catalog)?;
    let mapping = mapping_directory.join("mapping.mfd");
    write_mapping(&mapping, "Custom.X12/Envelope.Config")?;

    let options = mfd::ImportOptions::default().with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.runtime_dependencies().is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn catalog_lookup_rejects_symlink_escape() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = TempDir::new("symlink")?;
    let mapping_directory = directory.path().join("maps");
    let catalog = directory.path().join("catalog");
    let outside = directory.path().join("outside");
    std::fs::create_dir_all(&mapping_directory)?;
    std::fs::create_dir_all(&catalog)?;
    write_catalog(&outside, VALID_FIELD)?;
    symlink(outside.join("Custom.X12"), catalog.join("Custom.X12"))?;
    let mapping = mapping_directory.join("mapping.mfd");
    write_mapping(&mapping, "Custom.X12/Envelope.Config")?;

    let options = mfd::ImportOptions::default().with_edi_catalog_root(&catalog);
    let imported = mfd::import_with_options(&mapping, &options)?;

    assert_eq!(imported.project.runtime_dependencies().len(), 1);
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("could not compile external configuration")
            && warning.contains("resolves outside trusted catalog root")
    }));
    Ok(())
}

#[test]
fn invalid_explicit_catalog_root_is_an_import_error() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("invalid-root")?;
    let mapping = directory.path().join("mapping.mfd");
    write_mapping(&mapping, "Custom.X12/Envelope.Config")?;
    let missing = directory.path().join("missing");
    let options = mfd::ImportOptions::default().with_edi_catalog_root(&missing);

    let error = match mfd::import_with_options(&mapping, &options) {
        Ok(_) => return Err("import unexpectedly accepted a missing catalog root".into()),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("could not canonicalize trusted EDI catalog root")
    );
    Ok(())
}

const VALID_FIELD: &str = "F373";

fn write_catalog(root: &Path, field: &str) -> Result<(), std::io::Error> {
    let catalog = root.join("Custom.X12");
    std::fs::create_dir_all(&catalog)?;
    std::fs::write(
        catalog.join("Defs.Segment"),
        format!(
            r#"<Config><Elements>
  <Data name="F1" type="string"/><Data name="F143" type="string"/>
  <Data name="{field}" type="string"/>
  <Segment name="ISA"><Data ref="F1"/></Segment>
  <Segment name="GS"><Data ref="F1"/></Segment>
  <Segment name="ST"><Data ref="F143"/></Segment>
  <Segment name="BEG"><Data ref="{field}"/></Segment>
  <Segment name="SE"><Data ref="F1"/></Segment>
  <Segment name="GE"><Data ref="F1"/></Segment>
  <Segment name="IEA"><Data ref="F1"/></Segment>
</Elements></Config>"#
        ),
    )?;
    std::fs::write(
        catalog.join("Envelope.Config"),
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
        catalog.join("850.Config"),
        format!(
            r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
  <Message><MessageType>850</MessageType>
    <Group name="Message_850" maxOccurs="unbounded">
      <Segment ref="ST"/><Segment ref="BEG"/><Segment ref="SE"/>
    </Group>
  </Message>
</Config><!-- {field} -->"#
        ),
    )?;
    Ok(())
}

fn write_broken_catalog(root: &Path) -> Result<(), std::io::Error> {
    let catalog = root.join("Custom.X12");
    std::fs::create_dir_all(&catalog)?;
    std::fs::write(catalog.join("Envelope.Config"), "<not-edi/>")
}

fn write_catalog_zip(root: &Path) -> Result<(), Box<dyn Error>> {
    write_catalog(root, VALID_FIELD)?;
    let directory = root.join("Custom.X12");
    let archive = std::fs::File::create(root.join("Custom.X12.zip"))?;
    let mut archive = zip::ZipWriter::new(archive);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in ["Defs.Segment", "Envelope.Config", "850.Config"] {
        archive.start_file(format!("Custom.X12/{name}"), options)?;
        archive.write_all(&std::fs::read(directory.join(name))?)?;
    }
    archive.finish()?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn write_mapping(path: &Path, config: &str) -> Result<(), std::io::Error> {
    std::fs::write(
        path,
        format!(
            r#"<mapping version="26"><resources/><component name="defaultmap" uid="1">
  <structure><children>
    <component name="orders" library="text" uid="2" kind="16"><properties/><data>
      <root><entry name="FileInstance"><file role="inputinstance" name="orders.x12"/>
        <entry name="document"><entry name="Envelope"><entry name="Interchange">
          <entry name="Group"><entry name="Message_850"><entry name="BEG">
            <entry name="{VALID_FIELD}" outkey="10"/>
          </entry></entry></entry>
        </entry></entry></entry>
      </entry></root>
      <text type="edi" kind="EDIX12" config="{config}">
        <messages><message type="850"/></messages>
      </text>
    </data></component>
    <component name="output" library="xml" uid="3" kind="14">
      <properties XSLTDefaultOutput="1"/><data><root><entry name="Outputs">
        <entry name="Date" inpkey="20"/>
      </entry></root></data>
    </component>
  </children><graph directed="1"><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  </vertices></graph></structure>
</component></mapping>"#
        ),
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_catalog_{label}_{}_{}",
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
