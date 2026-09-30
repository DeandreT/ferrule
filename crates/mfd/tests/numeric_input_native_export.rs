use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_numeric_input_{}_{}",
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

fn compact_xml(
    project: &mapping::Project,
    instance: &ir::Instance,
) -> Result<String, Box<dyn Error>> {
    Ok(format_xml::to_string_with_options(
        &project.target,
        instance,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )?)
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn finite_numeric_input_preview_strictly_roundtrips_top_ten_xml() -> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let mapping = samples.join("FindHighestTemperatures.mfd");
    let imported = mfd::import_with_options(
        &mapping,
        &mfd::ImportOptions::default().with_package_root(&samples),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, mapping::Node::Call { function, .. } if function == "to_number"))
            .count(),
        0,
        "a finite numeric input preview is already a number"
    );

    let input_path = samples.join("Temperatures.xml");
    let input = format_xml::read(&input_path, &imported.project.source)?;
    let expected = engine::run(&imported.project, &input)?;
    let expected_json: serde_json::Value = serde_json::from_str(&format_json::to_string(
        &imported.project.target,
        &expected,
    )?)?;
    let rows = expected_json["data"].as_array().expect("top temperatures");
    assert_eq!(rows.len(), 10);
    assert_eq!(
        rows.iter()
            .map(|row| row["temp"].as_f64().expect("numeric temperature"))
            .collect::<Vec<_>>(),
        vec![24.0, 23.8, 23.2, 22.7, 22.3, 22.3, 21.5, 21.4, 21.1, 20.7]
    );
    assert_eq!(rows[0]["month"], "2008-07");
    assert_eq!(rows[9]["month"], "2007-08");

    let directory = TempDir::new()?;
    let exported = directory.0.join("highest.mfd");
    let report = mfd::preflight_export(&imported.project, &exported)?;
    assert!(report.is_native_compatible(), "{report}");
    let report =
        mfd::export_with_profile(&imported.project, &exported, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report}");
    let xml = std::fs::read_to_string(&exported)?;
    assert!(xml.contains("name=\"first-items\" library=\"core\""));
    assert!(!xml.contains("library=\"ferrule\""));

    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let roundtrip_input = format_xml::read(&input_path, &reimported.project.source)?;
    let actual = engine::run(&reimported.project, &roundtrip_input)?;
    assert_eq!(
        format_json::to_string(&reimported.project.target, &actual)?,
        format_json::to_string(&imported.project.target, &expected)?,
        "strict reimport changes ordered top-ten values"
    );
    assert_eq!(
        compact_xml(&reimported.project, &actual)?,
        compact_xml(&imported.project, &expected)?,
        "strict reimport changes ordered top-ten XML"
    );
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn integer_input_preview_clears_input_is_sequence_strict_preflight() -> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let imported = mfd::import_with_options(
        &samples.join("InputIsSequence.mfd"),
        &mfd::ImportOptions::default().with_package_root(&samples),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, mapping::Node::Call { function, .. } if function == "to_number"))
            .count(),
        0,
        "integer input preview needs no conversion"
    );
    let directory = TempDir::new()?;
    let report = mfd::preflight_export(&imported.project, &directory.0.join("sequence.mfd"))?;
    assert!(report.is_native_compatible(), "{report}");
    Ok(())
}
