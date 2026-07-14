use std::path::{Path, PathBuf};

use ir::ScalarType;
use mapping::{FlexCommand, FlexLineEnding, FlexTextLayout};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn flextext_options_reject_export_before_replacing_the_design() {
    let mut project = mfd::import(&fixture("people-csv.mfd")).unwrap().project;
    project.source_options.flextext = Some(
        FlexTextLayout::new(
            "document",
            FlexCommand::store("value", ScalarType::String, None),
            FlexLineEnding::Crlf,
            false,
        )
        .unwrap(),
    );
    let dir = std::env::temp_dir().join(format!(
        "ferrule_mfd_flextext_export_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let design = dir.join("mapping.mfd");
    std::fs::write(&design, "keep this design").unwrap();

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message))
            if message.contains("FlexText component export is not supported")
    ));
    assert_eq!(
        std::fs::read_to_string(&design).unwrap(),
        "keep this design"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
