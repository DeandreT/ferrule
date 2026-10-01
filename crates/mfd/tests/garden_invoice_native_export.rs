use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::Instance;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_garden_invoice_native_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mapped_xml(
    project: &mapping::Project,
    source: &Instance,
    path: &Path,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mapped = engine::run(project, source)?;
    format_xml::write(path, &project.target, &mapped)?;
    Ok(std::fs::read(path)?)
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn garden_invoice_native_roundtrip_preserves_ten_services_and_xml() -> Result<(), Box<dyn Error>> {
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/Tutorial");
    let pdf = samples.join("GardenInvoice.pdf");
    let imported = mfd::import(&samples.join("GardenInvoice.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let dir = TempDir::new()?;
    let baseline_source =
        format_pdf::read(&pdf, imported.project.source_options.pdf.as_ref().unwrap())?;
    assert_eq!(
        baseline_source
            .field("Service")
            .and_then(Instance::as_repeated)
            .map(|items| items.len()),
        Some(10),
        "the local invoice should yield ten service rows"
    );
    let baseline_xml = mapped_xml(
        &imported.project,
        &baseline_source,
        &dir.0.join("baseline.xml"),
    )?;

    let mut current = imported.project;
    for cycle in 0..2 {
        let design = dir.0.join(format!("garden-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, mfd::ExportProfile::NativeMfd)?;

        let template = std::fs::read_to_string(dir.0.join(format!("garden-{cycle}-source.pxt")))?;
        assert!(template.contains("<VerticalAnchorAssignment"));
        assert!(template.contains("<HorizontalAnchorAssignment"));
        assert!(template.contains("<Splitter"));
        assert!(!template.contains("FerruleLayout"));

        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        assert!(engine::validate(&restored.project).is_empty());
        let source = format_pdf::read(&pdf, restored.project.source_options.pdf.as_ref().unwrap())?;
        assert_eq!(
            source, baseline_source,
            "cycle {cycle} changed PDF extraction"
        );
        let xml = mapped_xml(
            &restored.project,
            &source,
            &dir.0.join(format!("garden-{cycle}.xml")),
        )?;
        assert_eq!(xml, baseline_xml, "cycle {cycle} changed mapped XML");
        current = restored.project;
    }
    Ok(())
}
