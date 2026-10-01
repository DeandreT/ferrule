use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture,
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfMerge,
    PdfMergeComposition, PdfMergeSource, PdfPageSelection, PdfPages, PdfReference, PdfRegion,
    Project, Scope, ScopeIteration,
};
use mfd::{ExportCompatibilityFeature, ExportProfile};

const UNSTABLE: u64 = 0x3feffffffffffc19;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-pdf-merge-{}-{}",
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
    let stream = |company: Option<&str>, first: &str, second: &str| {
        let heading = company.map_or(String::new(), |company| {
            format!("BT /F1 12 Tf 72 750 Td ({company}) Tj ET\n")
        });
        format!(
            "{heading}BT /F1 12 Tf 72 720 Td ({first}) Tj 0 -30 Td ({second}) Tj ET\n\
             0 734 m 612 734 l S\n\
             0 704 m 612 704 l S\n\
             0 674 m 612 674 l S\n"
        )
    };
    let first = stream(Some("Acme"), "Alpha", "Beta");
    let second = stream(None, "Gamma", "Delta");
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

fn page_two() -> NonZeroU32 {
    NonZeroU32::new(2).unwrap()
}

fn row_capture_region() -> PdfRegion {
    PdfRegion {
        left: PdfCoordinate::new(PdfReference::Left, -0.0),
        right: PdfCoordinate::new(PdfReference::Right, -f64::from_bits(UNSTABLE)),
        ..PdfRegion::full()
    }
}

fn page_row_region() -> PdfRegion {
    PdfRegion {
        left: PdfCoordinate::new(PdfReference::Left, -0.0),
        top: PdfCoordinate::new(PdfReference::Top, 55.0),
        right: PdfCoordinate::new(PdfReference::Right, -0.0),
        bottom: PdfCoordinate::new(PdfReference::Top, 130.0),
    }
}

fn layout() -> PdfLayout {
    PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![PdfCommand::Capture(PdfCapture {
                    name: "Company".into(),
                    region: PdfRegion {
                        left: PdfCoordinate::new(PdfReference::Left, -0.0),
                        top: PdfCoordinate::new(PdfReference::Top, 20.0),
                        right: PdfCoordinate::new(PdfReference::Right, -f64::from_bits(UNSTABLE)),
                        bottom: PdfCoordinate::new(PdfReference::Top, 55.0),
                    },
                    algorithm: Default::default(),
                })],
            }),
            PdfCommand::Merge(PdfMerge {
                name: "Records".into(),
                composition: PdfMergeComposition::Independent,
                sources: vec![
                    PdfMergeSource {
                        page_selection: PdfPageSelection::First,
                        region: page_row_region(),
                    },
                    PdfMergeSource {
                        page_selection: PdfPageSelection::Range {
                            first: page_two(),
                            last: page_two(),
                        },
                        region: page_row_region(),
                    },
                ],
                children: vec![PdfCommand::EdgeRows(PdfEdgeRows {
                    region: PdfRegion::full(),
                    find: PdfEdgeFind {
                        fill: 2.0,
                        prominence: 30.0,
                    },
                    minimum_extent: Some(f64::from_bits(UNSTABLE)),
                    fallback_anchor: Some(row_capture_region()),
                    children: vec![PdfCommand::GroupPerPage(PdfGroup {
                        name: "Row".into(),
                        region: PdfRegion::full(),
                        children: vec![PdfCommand::Capture(PdfCapture {
                            name: "Text".into(),
                            region: row_capture_region(),
                            algorithm: Default::default(),
                        })],
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
                SchemaNode::scalar("Company", ScalarType::String),
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
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Company".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Company".into(),
                node: 0,
            }],
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![Binding {
                    target_field: "Text".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn expected_output() -> Instance {
    Instance::Group(vec![
        (
            "Company".into(),
            Instance::Scalar(Value::String("Acme".into())),
        ),
        (
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
        ),
    ])
}

fn assert_exact_layout_bits(layout: &PdfLayout) {
    assert_eq!(layout.page_selection(), PdfPageSelection::All);
    let [PdfCommand::Pages(pages), PdfCommand::Merge(merge)] = layout.commands() else {
        panic!("expected first-page capture followed by an independent merge");
    };
    assert_eq!(pages.selection, PdfPageSelection::First);
    let [PdfCommand::Capture(company)] = pages.children.as_slice() else {
        panic!("expected one first-page capture");
    };
    assert_eq!(company.name, "Company");
    assert_eq!(company.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(
        company.region.right.offset.to_bits(),
        (-f64::from_bits(UNSTABLE)).to_bits()
    );

    assert_eq!(merge.name, "Records");
    assert_eq!(merge.composition, PdfMergeComposition::Independent);
    let [first, second] = merge.sources.as_slice() else {
        panic!("expected two ordered merge sources");
    };
    assert_eq!(first.page_selection, PdfPageSelection::First);
    assert_eq!(
        second.page_selection,
        PdfPageSelection::Range {
            first: page_two(),
            last: page_two(),
        }
    );
    assert_eq!(first.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(second.region.right.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(first.region.top.offset.to_bits(), 55.0_f64.to_bits());
    assert_eq!(second.region.bottom.offset.to_bits(), 130.0_f64.to_bits());

    let [PdfCommand::EdgeRows(rows)] = merge.children.as_slice() else {
        panic!("expected one edge-row splitter");
    };
    assert_eq!(rows.minimum_extent.unwrap().to_bits(), UNSTABLE);
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        panic!("expected one named row group");
    };
    assert_eq!(group.name, "Row");
    let [PdfCommand::Capture(capture)] = group.children.as_slice() else {
        panic!("expected one direct row capture");
    };
    assert_eq!(capture.name, "Text");
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
fn first_page_heading_and_independent_merge_map_physical_rows_through_two_strict_cycles()
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
        let design = temp.0.join(format!("merge-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd)?;
        let template = std::fs::read_to_string(temp.0.join(format!("merge-{cycle}-source.pxt")))?;
        assert!(template.contains("<MergeSource id="));
        assert!(template.contains("<MergeTarget id="));
        assert!(template.contains("<Splitter id="));
        assert!(template.contains("<Filter>1</Filter>"));
        assert!(template.contains("<Filter>2</Filter>"));
        assert!(!template.contains("FerruleLayout"));
        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        current = restored.project;
    }
    Ok(())
}

#[test]
fn incompatible_merge_controls_reject_native_export_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let second_page_capture = changed_layout(|commands| {
        let PdfCommand::Pages(pages) = &mut commands[0] else {
            unreachable!()
        };
        pages.selection = PdfPageSelection::Range {
            first: page_two(),
            last: page_two(),
        };
    });
    let reversed_merge_sources = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        merge.sources.reverse();
    });
    let open_second_source = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        merge.sources[1].page_selection = PdfPageSelection::From { first: page_two() };
    });
    let reversed_commands = changed_layout(|commands| commands.reverse());
    let vertical_collage = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        merge.composition = PdfMergeComposition::VerticalCollage;
    });
    let no_fallback = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut merge.children[0] else {
            unreachable!()
        };
        rows.fallback_anchor = None;
    });
    let wrong_fallback = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut merge.children[0] else {
            unreachable!()
        };
        rows.fallback_anchor.as_mut().unwrap().left.offset = 3.0;
    });
    let extra_group_child = changed_layout(|commands| {
        let PdfCommand::Merge(merge) = &mut commands[1] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut merge.children[0] else {
            unreachable!()
        };
        let PdfCommand::GroupPerPage(group) = &mut rows.children[0] else {
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

    let temp = TempDir::new()?;
    for (name, layout) in [
        ("second-page-capture", second_page_capture),
        ("reversed-merge-sources", reversed_merge_sources),
        ("open-second-source", open_second_source),
        ("reversed-commands", reversed_commands),
        ("vertical-collage", vertical_collage),
        ("no-fallback", no_fallback),
        ("wrong-fallback", wrong_fallback),
        ("extra-group-child", extra_group_child),
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
