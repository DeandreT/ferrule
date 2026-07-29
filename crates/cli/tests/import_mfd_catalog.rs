use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn import_mfd_accepts_a_relocatable_package_manifest() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let maps = directory.path().join("maps");
    let catalog = directory.path().join("resources/edi");
    std::fs::create_dir_all(&maps)?;
    write_catalog(&catalog.join("PortableEdiX12"))?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping)?;
    let manifest = directory.path().join("ferrule-package.json");
    std::fs::write(
        &manifest,
        r#"{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [{"kind": "edi-config", "root": "resources/edi"}]
}"#,
    )?;
    let output = directory.path().join("mapping.json");

    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(&mapping)
        .args(["--out"])
        .arg(&output)
        .args(["--package-manifest"])
        .arg(&manifest)
        .output()?;

    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("(0 warning(s))"),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let serialized = std::fs::read_to_string(&output)?;
    assert!(!serialized.contains(&catalog.display().to_string()));
    assert!(!serialized.contains(&manifest.display().to_string()));
    std::fs::remove_dir_all(directory.path().join("resources"))?;
    let project: mapping::Project = serde_json::from_str(&serialized)?;
    assert_eq!(project.source_path.as_deref(), Some("maps/orders.x12"));
    assert!(project.runtime_dependencies().is_empty());
    assert!(engine::validate(&project).is_empty());
    Ok(())
}

#[test]
fn package_manifest_conflicts_with_an_explicit_package_root() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args([
            "import-mfd",
            "--mfd",
            "mapping.mfd",
            "--out",
            "mapping.json",
            "--package-root",
            ".",
            "--package-manifest",
            "ferrule-package.json",
        ])
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn import_mfd_accepts_repeated_trusted_edi_catalog_roots() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let maps = directory.path().join("maps");
    let empty_catalog = directory.path().join("empty");
    let catalog = directory.path().join("catalog");
    std::fs::create_dir_all(&maps)?;
    std::fs::create_dir_all(&empty_catalog)?;
    write_catalog(&catalog.join("PortableEdiX12"))?;
    let mapping = maps.join("mapping.mfd");
    write_mapping(&mapping)?;
    let output = directory.path().join("mapping.json");

    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(&mapping)
        .args(["--out"])
        .arg(&output)
        .args(["--edi-catalog-root"])
        .arg(&empty_catalog)
        .args(["--edi-catalog-root"])
        .arg(&catalog)
        .output()?;

    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("(0 warning(s))"),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let project: mapping::Project = serde_json::from_slice(&std::fs::read(output)?)?;
    assert!(project.runtime_dependencies().is_empty());
    assert!(engine::validate(&project).is_empty());
    Ok(())
}

fn write_catalog(root: &Path) -> Result<(), std::io::Error> {
    let catalog = root.join("Custom.X12");
    std::fs::create_dir_all(&catalog)?;
    std::fs::write(
        catalog.join("Defs.Segment"),
        r#"<Config><Elements>
  <Data name="F1" type="string"/><Data name="F143" type="string"/>
  <Data name="F373" type="string"/>
  <Segment name="ISA"><Data ref="F1"/></Segment>
  <Segment name="GS"><Data ref="F1"/></Segment>
  <Segment name="ST"><Data ref="F143"/></Segment>
  <Segment name="BEG"><Data ref="F373"/></Segment>
  <Segment name="SE"><Data ref="F1"/></Segment>
  <Segment name="GE"><Data ref="F1"/></Segment>
  <Segment name="IEA"><Data ref="F1"/></Segment>
</Elements></Config>"#,
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
        r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
  <Message><MessageType>850</MessageType>
    <Group name="Message_850" maxOccurs="unbounded">
      <Segment ref="ST"/><Segment ref="BEG"/><Segment ref="SE"/>
    </Group>
  </Message>
</Config>"#,
    )
}

fn write_mapping(path: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        path,
        r#"<mapping version="26"><resources/><component name="defaultmap" uid="1">
  <structure><children>
    <component name="orders" library="text" uid="2" kind="16"><properties/><data>
      <root><entry name="FileInstance"><file role="inputinstance" name="orders.x12"/>
        <entry name="document"><entry name="Envelope"><entry name="Interchange">
          <entry name="Group"><entry name="Message_850"><entry name="BEG">
            <entry name="F373" outkey="10"/>
          </entry></entry></entry>
        </entry></entry></entry>
      </entry></root>
      <text type="edi" kind="EDIX12"
            config="..\PortableEdiX12\Custom.X12\Envelope.Config">
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
</component></mapping>"#,
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_edi_catalog_{}_{}",
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
