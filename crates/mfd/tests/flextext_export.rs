use std::collections::BTreeMap;
use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::ScalarType;
use mapping::{
    Binding, FlexCommand, FlexLineEnding, FlexTextLayout, FormatOptions, Graph, ManySplitter, Node,
    OnceSplitter, Project, Scope,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_flextext_export_{}_{}",
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

#[test]
fn source_layout_roundtrips_and_executes_identically() -> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("flextext-source.mfd"))?;
    let temp = TempDir::new()?;
    let design = temp.0.join("source-map.mfd");

    assert!(mfd::export(&imported.project, &design)?.is_empty());
    assert!(temp.0.join("source-map-source.mft").is_file());
    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source, imported.project.source);
    assert_eq!(
        roundtrip.project.source_options.flextext,
        imported.project.source_options.flextext
    );
    assert!(engine::validate(&roundtrip.project).is_empty());

    let layout = imported
        .project
        .source_options
        .flextext
        .as_ref()
        .ok_or("fixture has no FlexText source layout")?;
    let source = format_flextext::read(
        &fixture("flextext/source.flex"),
        &imported.project.source,
        layout,
    )?;
    assert_eq!(
        engine::run(&roundtrip.project, &source)?,
        engine::run(&imported.project, &source)?
    );
    Ok(())
}

#[test]
fn target_layout_roundtrips_and_renders_identically() -> Result<(), Box<dyn Error>> {
    let imported = mfd::import(&fixture("flextext-target.mfd"))?;
    let temp = TempDir::new()?;
    let design = temp.0.join("target-map.mfd");

    assert!(mfd::export(&imported.project, &design)?.is_empty());
    assert!(temp.0.join("target-map-target.mft").is_file());
    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.target, imported.project.target);
    assert_eq!(
        roundtrip.project.target_options.flextext,
        imported.project.target_options.flextext
    );
    assert!(engine::validate(&roundtrip.project).is_empty());

    let source = format_xml::read(
        &fixture("flextext/target-source.xml"),
        &imported.project.source,
    )?;
    let original = engine::run(&imported.project, &source)?;
    let exported = engine::run(&roundtrip.project, &source)?;
    let original_layout = imported
        .project
        .target_options
        .flextext
        .as_ref()
        .ok_or("fixture has no FlexText target layout")?;
    let exported_layout = roundtrip
        .project
        .target_options
        .flextext
        .as_ref()
        .ok_or("roundtrip has no FlexText target layout")?;
    assert_eq!(
        format_flextext::to_string(&roundtrip.project.target, &exported, exported_layout)?,
        format_flextext::to_string(&imported.project.target, &original, original_layout)?
    );
    Ok(())
}

#[test]
fn delimiter_and_line_content_splits_roundtrip_losslessly() -> Result<(), Box<dyn Error>> {
    let one = NonZeroU32::new(1).ok_or("one is nonzero")?;
    let layout = FlexTextLayout::new(
        "Root",
        FlexCommand::SplitOnce {
            name: "Sections".into(),
            splitter: OnceSplitter::LineContaining("MARK %\r\n".into()),
            first: Box::new(FlexCommand::SplitOnce {
                name: "Pair".into(),
                splitter: OnceSplitter::Delimiter("=%".into()),
                first: Box::new(FlexCommand::store("Key", ScalarType::String, None)),
                second: Box::new(FlexCommand::store("Value", ScalarType::String, None)),
            }),
            second: Box::new(FlexCommand::SplitMany {
                name: "Lines".into(),
                splitter: ManySplitter::FixedLines(one),
                child: Box::new(FlexCommand::store("Line", ScalarType::String, None)),
            }),
        },
        FlexLineEnding::Lf,
        true,
    )?;
    let project = Project {
        source: layout.schema(),
        target: ir::SchemaNode::group(
            "Result",
            vec![ir::SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("inputs/data%20.txt".into()),
        target_path: Some("result.xml".into()),
        source_options: FormatOptions {
            flextext: Some(layout.clone()),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Sections".into(), "Pair".into(), "Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };
    let temp = TempDir::new()?;
    let design = temp.0.join("splitters.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let roundtrip = mfd::import(&design)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source_options.flextext, Some(layout));
    assert_eq!(roundtrip.project.source_path, project.source_path);
    assert!(engine::validate(&roundtrip.project).is_empty());
    Ok(())
}

#[test]
fn unsupported_layout_rejects_before_replacing_any_artifact() -> Result<(), Box<dyn Error>> {
    let mut project = mfd::import(&fixture("people-csv.mfd"))?.project;
    let layout = FlexTextLayout::new(
        "Document",
        FlexCommand::SplitOnce {
            name: "Parts".into(),
            splitter: OnceSplitter::LineStartingWith("ITEM".into()),
            first: Box::new(FlexCommand::store("Header", ir::ScalarType::String, None)),
            second: Box::new(FlexCommand::store("Body", ir::ScalarType::String, None)),
        },
        FlexLineEnding::Crlf,
        false,
    )?;
    project.source = layout.schema();
    project.source_path = Some("input.txt".into());
    project.source_options = FormatOptions {
        flextext: Some(layout),
        ..FormatOptions::default()
    };

    let temp = TempDir::new()?;
    let design = temp.0.join("mapping.mfd");
    let config = temp.0.join("mapping-source.mft");
    std::fs::write(&design, "keep design")?;
    std::fs::write(&config, "keep config")?;

    let error = mfd::export(&project, &design).expect_err("unsupported splitter must fail");
    assert!(error.to_string().contains("line-starting single split"));
    assert_eq!(std::fs::read_to_string(design)?, "keep design");
    assert_eq!(std::fs::read_to_string(config)?, "keep config");
    Ok(())
}

#[test]
fn mismatched_layout_schema_is_rejected_atomically() -> Result<(), Box<dyn Error>> {
    let mut project = mfd::import(&fixture("people-csv.mfd"))?.project;
    let layout = FlexTextLayout::new(
        "Document",
        FlexCommand::SplitMany {
            name: "Lines".into(),
            splitter: mapping::ManySplitter::FixedLines(
                NonZeroU32::new(1).ok_or("one is nonzero")?,
            ),
            child: Box::new(FlexCommand::store("Value", ir::ScalarType::String, None)),
        },
        FlexLineEnding::Lf,
        false,
    )?;
    project.source_options = FormatOptions {
        flextext: Some(layout),
        ..FormatOptions::default()
    };

    let temp = TempDir::new()?;
    let design = temp.0.join("mapping.mfd");
    std::fs::write(&design, "keep design")?;
    let error = mfd::export(&project, &design).expect_err("schema mismatch must fail");
    assert!(error.to_string().contains("does not exactly match"));
    assert_eq!(std::fs::read_to_string(design)?, "keep design");
    assert!(!temp.0.join("mapping-source.mft").exists());
    Ok(())
}
