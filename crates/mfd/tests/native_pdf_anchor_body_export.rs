use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture,
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfPageSelection,
    PdfPages, PdfReference, PdfRegion, PdfVerticalBoundaryFind, Project, Scope, ScopeIteration,
};
use mfd::{ExportCompatibilityFeature, ExportProfile};

const AWKWARD_FINITE: u64 = 0x3feffffffffffc19;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-pdf-anchor-body-{}-{}",
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

// A self-authored two-page document. Both pages have two ruled rows, but only
// the first page's header should appear in the extracted instance.
fn two_page_ruled_pdf() -> Vec<u8> {
    fn page_stream(header: &str, reference: &str, labels: [&str; 2], counts: [&str; 2]) -> String {
        let mut stream = String::new();
        let _ = writeln!(stream, "BT /F1 12 Tf 72 720 Td ({header}) Tj ET");
        let _ = writeln!(stream, "BT /F1 12 Tf 370 680 Td ({reference}) Tj ET");
        for index in 0..2 {
            let y = 510 - index * 60;
            let _ = writeln!(stream, "BT /F1 12 Tf 75 {y} Td ({}) Tj ET", labels[index]);
            let _ = writeln!(stream, "BT /F1 12 Tf 400 {y} Td ({}) Tj ET", counts[index]);
        }
        for y in [542, 482, 422] {
            let _ = writeln!(stream, "50 {y} m 562 {y} l S");
        }
        stream
    }

    let first = page_stream("PRIMARY", "R-101", ["Alpha", "Beta"], ["1", "2"]);
    let second = page_stream("DECOY", "R-999", ["Gamma", "Delta"], ["3", "4"]);
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

fn capture(name: &str, region: PdfRegion) -> PdfCommand {
    PdfCommand::Capture(PdfCapture {
        name: name.into(),
        region,
        algorithm: Default::default(),
    })
}

fn horizontal_anchor(name: &str, reference: PdfReference, offset: f64) -> PdfCommand {
    PdfCommand::Anchor(PdfAnchorAssignment {
        name: name.into(),
        axis: PdfAnchorAxis::Horizontal,
        at: PdfCoordinate::new(reference, offset),
    })
}

fn column_capture(name: &str, left: &str, right: &str) -> PdfCommand {
    capture(
        name,
        PdfRegion {
            left: PdfCoordinate::edge(PdfReference::Anchor(left.into())),
            top: PdfCoordinate::edge(PdfReference::Top),
            right: PdfCoordinate::edge(PdfReference::Anchor(right.into())),
            bottom: PdfCoordinate::edge(PdfReference::Bottom),
        },
    )
}

fn layout() -> PdfLayout {
    PdfLayout::new(
        "Packet",
        PdfPageSelection::All,
        vec![
            PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Header".into(),
                    region: PdfRegion::full(),
                    children: vec![
                        capture(
                            "Batch",
                            PdfRegion {
                                left: PdfCoordinate::new(PdfReference::Left, -0.0),
                                top: PdfCoordinate::new(PdfReference::Top, 40.0),
                                right: PdfCoordinate::new(PdfReference::Left, 300.0),
                                bottom: PdfCoordinate::new(PdfReference::Top, 90.0),
                            },
                        ),
                        capture(
                            "Reference",
                            PdfRegion {
                                left: PdfCoordinate::new(PdfReference::Left, 350.0),
                                top: PdfCoordinate::new(PdfReference::Top, 90.0),
                                right: PdfCoordinate::new(PdfReference::Left, 550.0),
                                bottom: PdfCoordinate::new(PdfReference::Top, 140.0),
                            },
                        ),
                    ],
                })],
            }),
            PdfCommand::BoundaryFindVertical(PdfVerticalBoundaryFind {
                region: PdfRegion {
                    left: PdfCoordinate::new(PdfReference::Left, -0.0),
                    ..PdfRegion::full()
                },
                begin_anchor: "Begin".into(),
                end_anchor: "End".into(),
                find: PdfEdgeFind {
                    fill: f64::from_bits(AWKWARD_FINITE),
                    prominence: 100.0,
                },
            }),
            PdfCommand::EdgeRows(PdfEdgeRows {
                region: PdfRegion {
                    left: PdfCoordinate::new(PdfReference::Left, 50.0),
                    top: PdfCoordinate::edge(PdfReference::Anchor("Begin".into())),
                    right: PdfCoordinate::new(PdfReference::Left, 562.0),
                    bottom: PdfCoordinate::new(PdfReference::Anchor("End".into()), -0.0),
                },
                find: PdfEdgeFind {
                    fill: 2.0,
                    prominence: 100.0,
                },
                minimum_extent: Some(f64::from_bits(AWKWARD_FINITE)),
                fallback_anchor: None,
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Item".into(),
                    region: PdfRegion::full(),
                    children: vec![
                        horizontal_anchor("ColStart", PdfReference::Left, 0.0),
                        horizontal_anchor(
                            "ColBreak",
                            PdfReference::Anchor("ColStart".into()),
                            250.0,
                        ),
                        horizontal_anchor("ColEnd", PdfReference::Anchor("ColBreak".into()), 250.0),
                        column_capture("Label", "ColStart", "ColBreak"),
                        column_capture("Count", "ColBreak", "ColEnd"),
                    ],
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
                SchemaNode::group(
                    "Header",
                    vec![
                        SchemaNode::scalar("Batch", ScalarType::String),
                        SchemaNode::scalar("Reference", ScalarType::String),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Label", ScalarType::String),
                        SchemaNode::scalar("Count", ScalarType::String),
                    ],
                )
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
            nodes: ["Batch", "Reference", "Label", "Count"]
                .into_iter()
                .enumerate()
                .map(|(id, name)| {
                    (
                        id as u32,
                        Node::SourceField {
                            path: vec![name.into()],
                            frame: None,
                        },
                    )
                })
                .collect(),
        },
        root: Scope {
            children: vec![
                Scope {
                    target_field: "Header".into(),
                    iteration: ScopeIteration::Source(vec!["Header".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Batch".into(),
                            node: 0,
                        },
                        Binding {
                            target_field: "Reference".into(),
                            node: 1,
                        },
                    ],
                    ..Scope::default()
                },
                Scope {
                    target_field: "Item".into(),
                    iteration: ScopeIteration::Source(vec!["Item".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Label".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "Count".into(),
                            node: 3,
                        },
                    ],
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        },
    }
}

fn assert_packet(instance: &Instance) {
    let header = instance
        .field("Header")
        .and_then(Instance::as_repeated)
        .expect("first-page header group must be present");
    assert_eq!(header.len(), 1);
    assert_eq!(
        header[0].field("Batch").and_then(Instance::as_scalar),
        Some(&Value::String("PRIMARY".into()))
    );
    assert_eq!(
        header[0].field("Reference").and_then(Instance::as_scalar),
        Some(&Value::String("R-101".into()))
    );

    let items = instance
        .field("Item")
        .and_then(Instance::as_repeated)
        .expect("each page must contribute two ruled rows");
    assert_eq!(items.len(), 4);
    for (item, (label, count)) in items.iter().zip([
        ("Alpha", "1"),
        ("Beta", "2"),
        ("Gamma", "3"),
        ("Delta", "4"),
    ]) {
        assert_eq!(
            item.field("Label").and_then(Instance::as_scalar),
            Some(&Value::String(label.into()))
        );
        assert_eq!(
            item.field("Count").and_then(Instance::as_scalar),
            Some(&Value::String(count.into()))
        );
    }
}

fn assert_exact_layout_bits(actual: &PdfLayout) {
    assert_eq!(actual, &layout());
    assert_eq!(actual.page_selection(), PdfPageSelection::All);
    let [
        PdfCommand::Pages(page),
        PdfCommand::BoundaryFindVertical(boundary),
        PdfCommand::EdgeRows(rows),
    ] = actual.commands()
    else {
        panic!("expected first-page header followed by all-page anchored rows");
    };
    assert_eq!(page.selection, PdfPageSelection::First);
    let [PdfCommand::GroupPerPage(header)] = page.children.as_slice() else {
        panic!("expected one Header group");
    };
    assert_eq!(header.name, "Header");
    assert_eq!(header.children.len(), 2);
    let PdfCommand::Capture(first_capture) = &header.children[0] else {
        panic!("expected direct Header capture");
    };
    assert_eq!(
        first_capture.region.left.offset.to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(boundary.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(boundary.find.fill.to_bits(), AWKWARD_FINITE);
    assert_eq!(rows.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(rows.minimum_extent.unwrap().to_bits(), AWKWARD_FINITE);
    assert!(rows.fallback_anchor.is_none());
    let [PdfCommand::GroupPerPage(item)] = rows.children.as_slice() else {
        panic!("expected one Item group");
    };
    assert_eq!(item.name, "Item");
    assert_eq!(item.children.len(), 5);
    assert!(
        matches!(item.children.last(), Some(PdfCommand::Capture(capture)) if capture.name == "Count")
    );
}

fn changed_layout(edit: impl FnOnce(&mut Vec<PdfCommand>)) -> PdfLayout {
    let mut commands = layout().commands().to_vec();
    edit(&mut commands);
    PdfLayout::new("Packet", PdfPageSelection::All, commands).unwrap()
}

#[test]
fn first_page_header_and_all_page_rows_survive_two_strict_native_cycles()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let pdf = two_page_ruled_pdf();
    let mut current = project(layout());
    let expected_source_schema = current.source.clone();
    let expected_target_schema = current.target.clone();
    let mut first_source = None;
    let mut first_output = None;

    for cycle in 0..=2 {
        assert_eq!(current.source, expected_source_schema);
        assert_eq!(current.target, expected_target_schema);
        let source_layout = current.source_options.pdf.as_ref().unwrap();
        assert_exact_layout_bits(source_layout);
        let source = format_pdf::from_bytes(&pdf, source_layout)?;
        assert_packet(&source);
        let output = engine::run(&current, &source)?;
        assert_packet(&output);
        if let Some(first) = &first_source {
            assert_eq!(&source, first);
        } else {
            first_source = Some(source);
        }
        if let Some(first) = &first_output {
            assert_eq!(&output, first);
        } else {
            first_output = Some(output);
        }
        if cycle == 2 {
            break;
        }

        let design = temp.0.join(format!("anchor-body-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd)?;
        let template =
            std::fs::read_to_string(temp.0.join(format!("anchor-body-{cycle}-source.pxt")))?;
        assert!(template.contains("<BoundaryFindVertical"));
        assert!(template.contains("<HorizontalAnchorAssignment"));
        assert!(template.contains("<Filter>1</Filter>"));
        assert!(template.contains("<Splitter"));
        assert!(!template.contains("FerruleLayout"));
        let restored = mfd::import(&design)?;
        assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
        current = restored.project;
    }
    Ok(())
}

#[test]
fn unsupported_anchor_body_shapes_reject_atomically() -> Result<(), Box<dyn std::error::Error>> {
    let second = NonZeroU32::new(2).unwrap();
    let non_first_header = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        page.selection = PdfPageSelection::Range {
            first: second,
            last: second,
        };
    });
    let extra_root = changed_layout(|commands| {
        commands.push(horizontal_anchor("Extra", PdfReference::Left, 0.0));
    });
    let misordered_root = changed_layout(|commands| {
        commands.swap(0, 1);
    });
    let nested_header_anchor = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::GroupPerPage(header) = &mut page.children[0] else {
            unreachable!()
        };
        header
            .children
            .insert(0, horizontal_anchor("HeaderX", PdfReference::Left, 0.0));
    });
    let narrowed_header = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::GroupPerPage(header) = &mut page.children[0] else {
            unreachable!()
        };
        header.region.left.offset = 1.0;
    });
    let changed_fallback = changed_layout(|commands| {
        let PdfCommand::EdgeRows(rows) = &mut commands[2] else {
            unreachable!()
        };
        rows.fallback_anchor = Some(PdfRegion::full());
    });
    let unsafe_boundary_name = changed_layout(|commands| {
        let PdfCommand::BoundaryFindVertical(boundary) = &mut commands[1] else {
            unreachable!()
        };
        boundary.begin_anchor = "Begin Name".into();
        let PdfCommand::EdgeRows(rows) = &mut commands[2] else {
            unreachable!()
        };
        rows.region.top.reference = PdfReference::Anchor("Begin Name".into());
    });
    let wrong_row_anchor = changed_layout(|commands| {
        let PdfCommand::EdgeRows(rows) = &mut commands[2] else {
            unreachable!()
        };
        rows.region.top.reference = PdfReference::Anchor("End".into());
    });

    let temp = TempDir::new()?;
    for (name, layout) in [
        ("non-first-header", non_first_header),
        ("extra-root", extra_root),
        ("misordered-root", misordered_root),
        ("nested-header-anchor", nested_header_anchor),
        ("narrowed-header", narrowed_header),
        ("changed-fallback", changed_fallback),
        ("unsafe-boundary-name", unsafe_boundary_name),
        ("wrong-row-anchor", wrong_row_anchor),
    ] {
        let project = project(layout);
        let design = temp.0.join(format!("{name}.mfd"));
        let sibling = temp.0.join(format!("{name}-source.pxt"));
        let report = mfd::preflight_export(&project, &design)?;
        assert!(
            report.issues.iter().any(|issue| {
                issue.feature == ExportCompatibilityFeature::PdfLayout
                    && issue.component == "Packet"
            }),
            "{name}: {report}"
        );
        assert!(mfd::export_with_profile(&project, &design, ExportProfile::NativeMfd).is_err());
        assert!(!design.exists(), "{name} published a rejected MFD");
        assert!(!sibling.exists(), "{name} published a rejected template");
        mfd::export(&project, &design)?;
        let template = std::fs::read_to_string(sibling)?;
        assert!(template.contains("<FerruleLayout"), "{name}: {template}");
    }
    Ok(())
}
