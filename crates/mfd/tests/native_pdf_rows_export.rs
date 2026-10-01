use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture,
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfPageSelection,
    PdfReference, PdfRegion, PdfVerticalBoundaryFind, Project, Scope, ScopeIteration,
};
use mfd::{ExportCompatibilityFeature, ExportProfile};

const UNSTABLE: u64 = 0x3feffffffffffc19;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-pdf-rows-{}-{}",
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

fn two_page_pdf() -> Vec<u8> {
    let stream = |first: &str, second: &str| {
        format!(
            "BT /F1 12 Tf 72 720 Td ({first}) Tj 0 -30 Td ({second}) Tj ET\n\
             0 734 m 612 734 l S\n\
             0 704 m 612 704 l S\n\
             0 674 m 612 674 l S\n"
        )
    };
    let first = stream("Alpha", "Beta");
    let second = stream("Gamma", "Delta");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>\n".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>\n".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>\n".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\n".to_vec(),
        format!("<< /Length {} >>\nstream\n{}endstream\n", first.len(), first).into_bytes(),
        format!("<< /Length {} >>\nstream\n{}endstream\n", second.len(), second).into_bytes(),
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

fn layout() -> PdfLayout {
    let capture_region = PdfRegion {
        left: PdfCoordinate::new(PdfReference::Left, -0.0),
        right: PdfCoordinate::new(PdfReference::Right, -f64::from_bits(UNSTABLE)),
        ..PdfRegion::full()
    };
    PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::BoundaryFindVertical(PdfVerticalBoundaryFind {
                region: PdfRegion {
                    left: PdfCoordinate::new(PdfReference::Left, -0.0),
                    ..PdfRegion::full()
                },
                begin_anchor: "Begin".into(),
                end_anchor: "End".into(),
                find: PdfEdgeFind {
                    fill: f64::from_bits(UNSTABLE),
                    prominence: 30.0,
                },
            }),
            PdfCommand::EdgeRows(PdfEdgeRows {
                region: PdfRegion {
                    top: PdfCoordinate::edge(PdfReference::Anchor("Begin".into())),
                    bottom: PdfCoordinate::new(PdfReference::Anchor("End".into()), -0.0),
                    ..PdfRegion::full()
                },
                find: PdfEdgeFind {
                    fill: 2.0,
                    prominence: 30.0,
                },
                minimum_extent: Some(f64::from_bits(UNSTABLE)),
                fallback_anchor: Some(capture_region.clone()),
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Row".into(),
                    region: PdfRegion {
                        left: PdfCoordinate::new(PdfReference::Left, -0.0),
                        ..PdfRegion::full()
                    },
                    children: vec![PdfCommand::Capture(PdfCapture {
                        name: "Text".into(),
                        region: capture_region,
                        algorithm: Default::default(),
                    })],
                })],
            }),
        ],
    )
    .unwrap()
}

fn project(layout: PdfLayout) -> Project {
    Project {
        source: layout.schema(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Text", ScalarType::String)])
                    .repeating(),
            ],
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
                Node::SourceField {
                    path: vec!["Text".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![Binding {
                    target_field: "Text".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn expected_output() -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            ["Alpha", "Beta", "Gamma", "Delta"]
                .into_iter()
                .map(|text| {
                    Instance::Group(vec![(
                        "Text".into(),
                        Instance::Scalar(Value::String(text.into())),
                    )])
                })
                .collect(),
        ),
    )])
}

fn assert_exact_layout_bits(layout: &PdfLayout) {
    let [
        PdfCommand::BoundaryFindVertical(boundary),
        PdfCommand::EdgeRows(rows),
    ] = layout.commands()
    else {
        panic!("expected the boundary and edge-row commands");
    };
    assert_eq!(boundary.begin_anchor, "Begin");
    assert_eq!(boundary.end_anchor, "End");
    assert_eq!(boundary.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(boundary.find.fill.to_bits(), UNSTABLE);
    assert_eq!(rows.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(rows.minimum_extent.unwrap().to_bits(), UNSTABLE);
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        panic!("expected one named row group");
    };
    assert_eq!(group.name, "Row");
    assert_eq!(group.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    let [PdfCommand::Capture(capture)] = group.children.as_slice() else {
        panic!("expected one direct capture");
    };
    assert_eq!(capture.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(
        capture.region.right.offset.to_bits(),
        (-f64::from_bits(UNSTABLE)).to_bits()
    );
    let fallback = rows.fallback_anchor.as_ref().unwrap();
    assert_eq!(fallback.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(
        fallback.right.offset.to_bits(),
        (-f64::from_bits(UNSTABLE)).to_bits()
    );
}

fn changed_layout(edit: impl FnOnce(&mut Vec<PdfCommand>)) -> PdfLayout {
    let mut commands = layout().commands().to_vec();
    edit(&mut commands);
    PdfLayout::new("Document", PdfPageSelection::All, commands).unwrap()
}

#[test]
fn boundary_edge_rows_map_four_physical_pdf_rows_through_two_strict_cycles()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let pdf = two_page_pdf();
    let expected = expected_output();
    let mut current = project(layout());
    for cycle in 0..=2 {
        assert_exact_layout_bits(current.source_options.pdf.as_ref().unwrap());
        let source = format_pdf::from_bytes(&pdf, current.source_options.pdf.as_ref().unwrap())?;
        assert_eq!(engine::run(&current, &source)?, expected);
        if cycle == 2 {
            break;
        }
        let design = temp.0.join(format!("rows-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd)?;
        let template = std::fs::read_to_string(temp.0.join(format!("rows-{cycle}-source.pxt")))?;
        assert!(template.contains("<BoundaryFindVertical id=\"2\">"));
        assert!(template.contains("<Splitter id=\"3\">"));
        assert!(template.contains("<NameBegin>Begin</NameBegin>"));
        assert!(template.contains("<SkipInitial>0</SkipInitial>"));
        assert!(template.contains("<SkipFinal>0</SkipFinal>"));
        assert!(!template.contains("FerruleLayout"));
        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        current = restored.project;
    }
    Ok(())
}

#[test]
fn changed_inferred_fallback_and_other_row_shapes_reject_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let no_fallback = changed_layout(|commands| {
        let [_, PdfCommand::EdgeRows(rows)] = commands.as_mut_slice() else {
            unreachable!()
        };
        rows.fallback_anchor = None;
    });
    let wrong_fallback = changed_layout(|commands| {
        let [_, PdfCommand::EdgeRows(rows)] = commands.as_mut_slice() else {
            unreachable!()
        };
        rows.fallback_anchor.as_mut().unwrap().left.offset = 2.0;
    });
    let unsafe_anchor = changed_layout(|commands| {
        let [
            PdfCommand::BoundaryFindVertical(boundary),
            PdfCommand::EdgeRows(rows),
        ] = commands.as_mut_slice()
        else {
            unreachable!()
        };
        boundary.begin_anchor = "Begin Name".into();
        rows.region.top.reference = PdfReference::Anchor("Begin Name".into());
    });
    let wrong_row_anchor = changed_layout(|commands| {
        let [_, PdfCommand::EdgeRows(rows)] = commands.as_mut_slice() else {
            unreachable!()
        };
        rows.region.top.reference = PdfReference::Anchor("End".into());
    });
    let extra_child = changed_layout(|commands| {
        let [_, PdfCommand::EdgeRows(rows)] = commands.as_mut_slice() else {
            unreachable!()
        };
        let [PdfCommand::GroupPerPage(group)] = rows.children.as_mut_slice() else {
            unreachable!()
        };
        group.children.insert(
            0,
            PdfCommand::Anchor(PdfAnchorAssignment {
                name: "Unused".into(),
                axis: PdfAnchorAxis::Horizontal,
                at: PdfCoordinate::edge(PdfReference::Left),
            }),
        );
    });
    let first_page = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        layout().commands().to_vec(),
    )?;

    let temp = TempDir::new()?;
    for (name, layout) in [
        ("no-fallback", no_fallback),
        ("wrong-fallback", wrong_fallback),
        ("unsafe-anchor", unsafe_anchor),
        ("wrong-row-anchor", wrong_row_anchor),
        ("extra-child", extra_child),
        ("first-page", first_page),
    ] {
        let project = project(layout);
        let design = temp.0.join(format!("{name}.mfd"));
        let sibling = temp.0.join(format!("{name}-source.pxt"));
        let report = mfd::preflight_export(&project, &design)?;
        assert!(report.issues.iter().any(|issue| {
            issue.feature == ExportCompatibilityFeature::PdfLayout && issue.component == "Document"
        }));
        assert!(mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd).is_err());
        assert!(!design.exists(), "{name} published a rejected MFD");
        assert!(!sibling.exists(), "{name} published a rejected template");
        mfd::export(&project, &design)?;
        assert!(std::fs::read_to_string(sibling)?.contains("<FerruleLayout"));
    }
    Ok(())
}
