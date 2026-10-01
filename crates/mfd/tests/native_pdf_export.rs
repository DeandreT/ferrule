use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture,
    PdfCommand, PdfCoordinate, PdfGroup, PdfLayout, PdfPageSelection, PdfReference, PdfRegion,
    Project, Scope, ScopeIteration,
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

fn grouped_project(layout: PdfLayout) -> Project {
    let mut project = project(layout);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Page", vec![SchemaNode::scalar("Text", ScalarType::String)])
                .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    )]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Page".into(),
            iteration: ScopeIteration::Source(vec!["Page".into()]),
            bindings: vec![Binding {
                target_field: "Text".into(),
                node: 0,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

fn two_page_pdf(first: &str, second: &str) -> Vec<u8> {
    let stream = |text: &str| format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET\n");
    let first_stream = stream(first);
    let second_stream = stream(second);
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>\n".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>\n".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>\n".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream\n",
            first_stream.len(),
            first_stream
        )
        .into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream\n",
            second_stream.len(),
            second_stream
        )
        .into_bytes(),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"endobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
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
fn one_named_page_group_maps_two_pages_through_two_strict_native_cycles()
-> Result<(), Box<dyn std::error::Error>> {
    const UNSTABLE: u64 = 0x3feffffffffffc19;
    let temp = TempDir::new()?;
    let layout = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![PdfCommand::GroupPerPage(PdfGroup {
            name: "Page".into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, f64::from_bits(UNSTABLE)),
                bottom: PdfCoordinate::new(PdfReference::Bottom, -0.0),
                ..PdfRegion::full()
            },
            children: vec![capture("Text", -0.0, -f64::from_bits(UNSTABLE))],
        })],
    )?;
    let pdf = two_page_pdf("Alpha", "Beta");
    let expected = Instance::Group(vec![(
        "Page".into(),
        Instance::Repeated(
            ["Alpha", "Beta"]
                .into_iter()
                .map(|text| {
                    Instance::Group(vec![(
                        "Text".into(),
                        Instance::Scalar(Value::String(text.into())),
                    )])
                })
                .collect(),
        ),
    )]);
    let mut current = grouped_project(layout);
    for cycle in 0..=2 {
        let source = format_pdf::from_bytes(&pdf, current.source_options.pdf.as_ref().unwrap())?;
        assert_eq!(engine::run(&current, &source)?, expected);
        if cycle == 2 {
            break;
        }
        let design = temp.0.join(format!("grouped-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd)?;
        let template = std::fs::read_to_string(temp.0.join(format!("grouped-{cycle}-source.pxt")))?;
        assert!(template.contains("<Template version=\"1\">"));
        assert!(template.contains("<Label>Page</Label>"));
        assert!(template.contains("<OneGroupPerPage/>"));
        assert!(!template.contains("FerruleLayout"));
        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        let restored_layout = restored.project.source_options.pdf.as_ref().unwrap();
        let [PdfCommand::GroupPerPage(group)] = restored_layout.commands() else {
            panic!("expected one page group after strict reimport");
        };
        assert_eq!(group.name, "Page");
        assert_eq!(group.region.left.offset.to_bits(), UNSTABLE);
        assert_eq!(group.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
        let [PdfCommand::Capture(capture)] = group.children.as_slice() else {
            panic!("expected one direct capture after strict reimport");
        };
        assert_eq!(capture.name, "Text");
        assert_eq!(capture.region.left.offset.to_bits(), (-0.0_f64).to_bits());
        assert_eq!(
            capture.region.right.offset.to_bits(),
            (-f64::from_bits(UNSTABLE)).to_bits()
        );
        current = restored.project;
    }
    Ok(())
}

#[test]
fn other_group_shapes_keep_extensions_and_reject_strict_export_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let group = |name: &str, children| {
        PdfCommand::GroupPerPage(PdfGroup {
            name: name.into(),
            region: PdfRegion::full(),
            children,
        })
    };
    let nested = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![group(
            "Page",
            vec![group("Inner", vec![capture("Text", 0.0, 0.0)])],
        )],
    )?;
    let mixed = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![
            group("Page", vec![capture("Text", 0.0, 0.0)]),
            capture("Other", 0.0, 0.0),
        ],
    )?;
    let anchored = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![group(
            "Page",
            vec![
                PdfCommand::Anchor(PdfAnchorAssignment {
                    name: "Start".into(),
                    axis: PdfAnchorAxis::Horizontal,
                    at: PdfCoordinate::edge(PdfReference::Left),
                }),
                PdfCommand::Capture(PdfCapture {
                    name: "Text".into(),
                    region: PdfRegion {
                        left: PdfCoordinate::edge(PdfReference::Anchor("Start".into())),
                        ..PdfRegion::full()
                    },
                    algorithm: Default::default(),
                }),
            ],
        )],
    )?;
    let padded_label = PdfLayout::new(
        "Receipt",
        PdfPageSelection::All,
        vec![group(" Page", vec![capture("Text", 0.0, 0.0)])],
    )?;
    let temp = TempDir::new()?;
    for (name, layout) in [
        ("nested", nested),
        ("mixed", mixed),
        ("anchored", anchored),
        ("padded", padded_label),
    ] {
        let design = temp.0.join(format!("{name}.mfd"));
        let sibling = temp.0.join(format!("{name}-source.pxt"));
        let project = project(layout);
        let report = mfd::preflight_export(&project, &design)?;
        assert!(report.issues.iter().any(|issue| {
            issue.feature == ExportCompatibilityFeature::PdfLayout && issue.component == "Receipt"
        }));
        assert!(mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd).is_err());
        assert!(!design.exists(), "{name} published a rejected MFD");
        assert!(!sibling.exists(), "{name} published a rejected template");
        mfd::export(&project, &design)?;
        let template = std::fs::read_to_string(sibling)?;
        assert!(template.contains("<FerruleLayout"), "{name}: {template}");
    }
    Ok(())
}
