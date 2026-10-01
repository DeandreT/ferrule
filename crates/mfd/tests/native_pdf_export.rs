use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfCapture, PdfCommand, PdfCoordinate, PdfGroup,
    PdfLayout, PdfPageSelection, PdfReference, PdfRegion, Project, Scope,
};
use mfd::{ExportCompatibility, ExportCompatibilityFeature, ExportProfile};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-pdf-export-{}-{}",
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

fn capture(name: &str, left: f64, right: f64) -> PdfCommand {
    PdfCommand::Capture(PdfCapture {
        name: name.into(),
        region: PdfRegion {
            left: PdfCoordinate::new(PdfReference::Left, left),
            top: PdfCoordinate::new(PdfReference::Top, 2.5),
            right: PdfCoordinate::new(PdfReference::Right, right),
            bottom: PdfCoordinate::new(PdfReference::Bottom, -0.0),
        },
        algorithm: Default::default(),
    })
}

fn project(layout: PdfLayout) -> Project {
    Project {
        source: layout.schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Status", ScalarType::String)],
        ),
        source_path: Some("input.pdf".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            pdf: Some(layout),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: Value::String("ready".into()),
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Status".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

fn first_capture(layout: &PdfLayout) -> &PdfCapture {
    let Some(PdfCommand::Capture(capture)) = layout.commands().first() else {
        panic!("expected a direct PDF capture");
    };
    capture
}

#[test]
fn all_page_direct_captures_export_as_native_shaped_pxt_with_exact_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    const UNSTABLE: u64 = 0x3feffffffffffc19;
    let temp = TempDir::new()?;
    let design = temp.0.join("native.mfd");
    let layout = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![
            capture("Item", f64::from_bits(UNSTABLE), -0.0),
            capture("Price", 18.5, -3.0),
        ],
    )?;
    let project = project(layout);

    let report = mfd::preflight_export(&project, &design)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd)?;
    let template = std::fs::read_to_string(temp.0.join("native-source.pxt"))?;
    assert!(template.contains("<Template version=\"1\">"));
    assert!(template.contains("<OneGroupPerPage/>"));
    assert!(template.contains("<BasicVisual>"));
    assert!(!template.contains("FerruleLayout"));
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    let restored_layout = restored.project.source_options.pdf.as_ref().unwrap();
    assert_eq!(restored_layout.page_selection(), PdfPageSelection::All);
    assert_eq!(restored_layout.commands().len(), 2);
    let first = first_capture(restored_layout);
    assert_eq!(first.name, "Item");
    assert_eq!(first.region.left.offset.to_bits(), UNSTABLE);
    assert_eq!(first.region.right.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(first.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
    Ok(())
}

#[test]
fn native_pdf_labels_preserve_interior_xml_attribute_whitespace()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let design = temp.0.join("whitespace.mfd");
    let root_name = "Re\tceipt\n2026\rA";
    let capture_name = "It\tem\nA\rZ";
    let layout = PdfLayout::new(
        root_name,
        PdfPageSelection::All,
        vec![capture(capture_name, 0.0, 0.0)],
    )?;
    let project = project(layout);

    let report = mfd::preflight_export(&project, &design)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd)?;
    let xml = std::fs::read_to_string(&design)?;
    for reference in ["&#x9;", "&#xA;", "&#xD;"] {
        assert!(xml.contains(reference), "missing {reference} in MFD XML");
    }
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    let restored_layout = restored.project.source_options.pdf.as_ref().unwrap();
    assert_eq!(restored_layout.root_name(), root_name);
    assert_eq!(first_capture(restored_layout).name, capture_name);
    Ok(())
}

#[test]
fn first_page_capture_keeps_lossless_extension_and_native_export_rejects_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    const UNSTABLE: u64 = 0x3feffffffffffc19;
    let temp = TempDir::new()?;
    let design = temp.0.join("first.mfd");
    let layout = PdfLayout::new(
        "Receipt",
        PdfPageSelection::First,
        vec![capture("Item", f64::from_bits(UNSTABLE), -0.0)],
    )?;
    let project = project(layout);

    let report = mfd::preflight_export(&project, &design)?;
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
    assert!(report.issues.iter().any(|issue| {
        issue.feature == ExportCompatibilityFeature::PdfLayout && issue.component == "Receipt"
    }));
    assert!(mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd).is_err());
    assert!(!design.exists());
    assert!(!temp.0.join("first-source.pxt").exists());

    mfd::export(&project, &design)?;
    let template = std::fs::read_to_string(temp.0.join("first-source.pxt"))?;
    assert!(template.contains("<FerruleLayout version=\"2\">"));
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    let restored_layout = restored.project.source_options.pdf.as_ref().unwrap();
    assert_eq!(restored_layout.page_selection(), PdfPageSelection::First);
    assert_eq!(
        first_capture(restored_layout).region.left.offset.to_bits(),
        UNSTABLE
    );
    assert_eq!(
        first_capture(restored_layout).region.right.offset.to_bits(),
        (-0.0_f64).to_bits()
    );
    Ok(())
}

#[test]
fn grouped_layout_stays_an_extension_even_with_all_page_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let design = temp.0.join("grouped.mfd");
    let layout = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![PdfCommand::GroupPerPage(PdfGroup {
            name: "Row".into(),
            region: PdfRegion::full(),
            children: vec![capture("Item", 0.0, 0.0)],
        })],
    )?;
    let project = project(layout);
    let report = mfd::preflight_export(&project, &design)?;
    assert!(report.issues.iter().any(|issue| {
        issue.feature == ExportCompatibilityFeature::PdfLayout && issue.component == "Receipt"
    }));
    assert!(mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd).is_err());
    assert!(!design.exists());
    mfd::export(&project, &design)?;
    let template = std::fs::read_to_string(temp.0.join("grouped-source.pxt"))?;
    assert!(template.contains("<FerruleLayout"));
    Ok(())
}
