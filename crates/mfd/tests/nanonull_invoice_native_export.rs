use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::Instance;
use mapping::Project;

const FIXED_TIME: &str = "2026-09-29T12:00:00-07:00";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_nanonull_invoice_native_{}_{}",
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

fn mapped_json(project: &Project, source: &Instance) -> Result<(Instance, String), Box<dyn Error>> {
    // Use one logical project identity and clock value across all export cycles.
    let execution = engine::ExecutionContext::new(Path::new("/maps/nanonull-invoice.mfd"))
        .with_current_datetime(FIXED_TIME);
    let output = engine::run_with_context(project, source, &execution)?;
    let json = format_json::to_string(&project.target, &output)?;
    Ok((output, json))
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn nanonull_invoice_native_roundtrip_preserves_pdf_and_json() -> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("Nanonull_InvoicePDFtoJSON.mfd");
    let template_path = samples.join("Nanonull_Invoice.pxt");
    let pdf = samples.join("Invoice_2023-1000354.pdf");
    for required in [&design, &template_path, &pdf] {
        if !required.is_file() {
            eprintln!(
                "skipping Nanonull invoice native regression: local sample file is unavailable: {}",
                required.display()
            );
            return Ok(());
        }
    }

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let baseline_source =
        format_pdf::read(&pdf, imported.project.source_options.pdf.as_ref().unwrap())?;
    let (baseline_output, baseline_json) = mapped_json(&imported.project, &baseline_source)?;

    let temp = TempDir::new()?;
    let mut current = imported.project;
    for cycle in 0..2 {
        let exported = temp.0.join(format!("nanonull-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &exported)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &exported, mfd::ExportProfile::NativeMfd)?;

        let template =
            std::fs::read_to_string(temp.0.join(format!("nanonull-{cycle}-source.pxt")))?;
        assert!(template.contains("<BoundaryFindVertical"));
        assert!(template.contains("<HorizontalAnchorAssignment"));
        assert!(template.contains("<Splitter"));
        assert!(!template.contains("FerruleLayout"));

        let restored = mfd::import(&exported)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        assert!(engine::validate(&restored.project).is_empty());
        let source = format_pdf::read(&pdf, restored.project.source_options.pdf.as_ref().unwrap())?;
        assert_eq!(
            source, baseline_source,
            "cycle {cycle} changed PDF extraction"
        );
        let (output, json) = mapped_json(&restored.project, &source)?;
        assert_eq!(
            output, baseline_output,
            "cycle {cycle} changed the mapped value"
        );
        assert_eq!(json, baseline_json, "cycle {cycle} changed serialized JSON");
        current = restored.project;
    }
    Ok(())
}
