use std::io::Write;
use std::path::{Path, PathBuf};

use mapping::EdiBoundaryKind;
use zip::write::SimpleFileOptions;

#[test]
fn adjacent_zip_package_compiles_to_a_portable_executable_schema() {
    let directory = TempDir::new("packaged_config");
    let archive_path = directory.path().join("Custom.X12.zip");
    write_package(
        &archive_path,
        &[
            (
                "Custom.X12/Defs.Segment",
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
            ),
            (
                "Custom.X12/Envelope.Config",
                r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
                  <Group name="Envelope"><Group name="Interchange" maxOccurs="unbounded">
                    <Segment ref="ISA"/><Group name="Group" maxOccurs="unbounded">
                      <Segment ref="GS"/><Select field="ST/F143" maxOccurs="unbounded"/>
                      <Segment ref="GE" minOccurs="0"/>
                    </Group><Segment ref="IEA" minOccurs="0"/>
                  </Group></Group></Config>"#,
            ),
            (
                "Custom.X12/850.Config",
                r#"<Config><Format standard="X12"/><Include href="Defs.Segment"/>
                  <Message><MessageType>850</MessageType>
                    <Group name="Message_850" maxOccurs="unbounded">
                      <Segment ref="ST"/><Segment ref="BEG"/><Segment ref="SE"/>
                    </Group>
                  </Message></Config>"#,
            ),
        ],
    );
    let mapping_path = directory.path().join("mapping.mfd");
    std::fs::write(
        &mapping_path,
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1">
          <structure><children>
            <component name="orders" library="text" uid="2" kind="16"><properties/><data>
              <root><entry name="FileInstance"><file role="inputinstance" name="orders.x12"/>
                <entry name="document"><entry name="Envelope"><entry name="Interchange">
                  <entry name="Group"><entry name="Message_850"><entry name="BEG">
                    <entry name="F373" outkey="10"/>
                  </entry></entry></entry>
                </entry></entry></entry>
              </entry></root>
              <text type="edi" kind="EDIX12" config="Custom.X12/Envelope.Config">
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
    .unwrap();

    let imported = mfd::import(&mapping_path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
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
            .and_then(|node| node.child("F373"))
            .is_some()
    );
    assert!(engine::validate(&imported.project).is_empty());

    std::fs::remove_file(&archive_path).unwrap();
    let exported_path = directory.path().join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let reimported = mfd::import(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, imported.project.source);
}

fn write_package(path: &Path, entries: &[(&str, &str)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, contents) in entries {
        archive.start_file(*name, options).unwrap();
        archive.write_all(contents.as_bytes()).unwrap();
    }
    archive.finish().unwrap();
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_edi_{label}_{}_{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
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
