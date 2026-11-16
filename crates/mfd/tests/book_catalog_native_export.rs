#[path = "support/private_corpus.rs"]
mod private_corpus;

use std::path::Path;

use ir::Instance;
use mfd::ExportProfile;

use test_support::TempDir;

#[test]
#[ignore = "needs the local ReferenceSamples corpus; informational only"]
fn local_book_catalog_keeps_extraction_and_xml_through_two_strict_native_cycles()
-> Result<(), Box<dyn std::error::Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("BookCatalogPDFToXML.mfd");
    let pdf = samples.join("BookCatalog.pdf");
    private_corpus::require(&[&design, &pdf]);

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut current = imported.project;
    let original_layout = current
        .source_options
        .pdf
        .as_ref()
        .expect("local BookCatalog import must retain its PDF layout");
    let original_source = format_pdf::read(&pdf, original_layout)?;
    let original_output = engine::run(&current, &original_source)?;
    let books = original_output
        .field("Book")
        .and_then(Instance::as_repeated)
        .expect("local BookCatalog mapping must produce repeated books");
    assert_eq!(books.len(), 52);
    let original_xml = format_xml::to_string(&current.target, &original_output)?;

    let temp = TempDir::new("book-catalog-native")?;
    for cycle in 0..2 {
        let exported_design = temp.path().join(format!("book-catalog-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &exported_design)?;
        assert!(report.is_native_compatible(), "{report}");
        let report =
            mfd::export_with_profile(&current, &exported_design, ExportProfile::NativeMfd)?;
        assert!(report.is_native_compatible(), "{report}");
        let template =
            std::fs::read_to_string(temp.path().join(format!("book-catalog-{cycle}-source.pxt")))?;
        assert!(template.contains("<Template version=\"1\">"));
        assert!(template.contains("<MergeTarget"));
        assert!(!template.contains("FerruleLayout"));

        let restored = mfd::import(&exported_design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        let restored_layout = restored
            .project
            .source_options
            .pdf
            .as_ref()
            .expect("strict reimport must retain its PDF layout");
        let restored_source = format_pdf::read(&pdf, restored_layout)?;
        assert!(
            restored_source == original_source,
            "PDF extraction changed after strict cycle {cycle}"
        );
        let restored_output = engine::run(&restored.project, &restored_source)?;
        let books = restored_output
            .field("Book")
            .and_then(Instance::as_repeated)
            .expect("strict reimport must produce repeated books");
        assert_eq!(books.len(), 52);
        let restored_xml = format_xml::to_string(&restored.project.target, &restored_output)?;
        assert!(
            restored_xml == original_xml,
            "serialized XML changed after strict cycle {cycle}"
        );
        current = restored.project;
    }
    Ok(())
}
