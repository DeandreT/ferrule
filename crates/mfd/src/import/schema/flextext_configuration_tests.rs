use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::FlexLineEnding;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-flextext-configuration-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn standalone_flextext_configuration_derives_schema_without_opening_its_data_file()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let path = directory.0.join("layout.mft");
    let data_path = directory.0.join("not-present.txt");
    std::fs::write(
        &path,
        r#"<FlexText><Commands><Project FileName="not-present.txt" LineEnding="LF" ByteOrderMark="1">
<RootName Value="document"/><Connections><Connection><CSV>
<RecordName Value="rows"/><FieldSeparator Value="|"/><RecordSeparator Value="%0A"/>
<Fields><Field Type="string"><Name Value="text"/></Field><Field Type="integer"><Name Value="count"/></Field></Fields>
</CSV></Connection></Connections></Project></Commands></FlexText>"#,
    )?;
    let layout = crate::import_flextext_configuration(&path)?;
    assert_eq!(layout.output_line_ending(), FlexLineEnding::Lf);
    assert!(layout.write_bom());
    assert_eq!(
        layout.schema(),
        SchemaNode::group(
            "document",
            vec![
                SchemaNode::group(
                    "rows",
                    vec![
                        SchemaNode::scalar("text", ScalarType::String),
                        SchemaNode::scalar("count", ScalarType::Int),
                    ],
                )
                .repeating()
            ],
        )
    );
    assert!(!data_path.exists());
    Ok(())
}

#[test]
fn standalone_flextext_configuration_rejects_oversized_files_before_xml_parsing()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let path = directory.0.join("oversized.mft");
    let file = std::fs::File::create(&path)?;
    file.set_len((crate::MAX_FLEXTEXT_CONFIGURATION_BYTES * 16) as u64)?;
    let error = crate::import_flextext_configuration(&path).unwrap_err();
    assert!(error.to_string().contains("exceeds"));
    assert!(error.to_string().contains("byte limit"));
    assert!(!error.to_string().contains("could not parse"));
    Ok(())
}

#[test]
fn standalone_flextext_configuration_rejects_invalid_unsupported_and_excessive_xml()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let path = directory.0.join("invalid.mft");
    let cases = [
        ("<FlexText>", "could not parse"),
        (
            "<!DOCTYPE FlexText SYSTEM 'missing.dtd'><FlexText/>",
            "document type declaration",
        ),
        (
            "<FlexText><Commands><Project><RootName Value='document'/><Connections><Connection><LoadFile/></Connection></Connections></Project></Commands></FlexText>",
            "unsupported FlexText command",
        ),
        (
            "<FlexText><Commands><Project><RootName Value='document'/><Connections><Connection><Store><Name Value='bad%GG'/></Store></Connection></Connections></Project></Commands></FlexText>",
            "invalid percent escape",
        ),
    ];
    for (source, expected) in cases {
        std::fs::write(&path, source)?;
        let error = crate::import_flextext_configuration(&path).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    std::fs::write(&path, [0xff])?;
    assert!(
        crate::import_flextext_configuration(&path)
            .unwrap_err()
            .to_string()
            .contains("not valid UTF-8")
    );
    std::fs::write(
        &path,
        format!("<FlexText>{}</FlexText>", "<Version/>".repeat(16_384)),
    )?;
    assert!(
        crate::import_flextext_configuration(&path)
            .unwrap_err()
            .to_string()
            .contains("element limit")
    );
    Ok(())
}
