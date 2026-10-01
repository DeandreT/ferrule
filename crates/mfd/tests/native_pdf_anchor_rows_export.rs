use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, Node, PdfAnchorAssignment, PdfAnchorAxis, PdfCapture,
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout, PdfPageSelection,
    PdfPages, PdfReference, PdfRegion, Project, Scope, ScopeIteration,
};
use mfd::{ExportCompatibilityFeature, ExportProfile};

const AWKWARD_FINITE: u64 = 0x3feffffffffffc19;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-native-pdf-anchor-rows-{}-{}",
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

// Entirely self-authored: two letter-size pages, each with the same painted
// table lines. The second page has decoy values to check Pages(First).
fn two_page_ruled_pdf() -> Vec<u8> {
    fn page_stream(header: &str, descriptions: [&str; 2], total: &str) -> String {
        let mut stream = String::new();
        let _ = writeln!(stream, "BT /F1 12 Tf 72 720 Td ({header}) Tj ET");
        for (index, description) in descriptions.iter().enumerate() {
            let y = 510 - index * 60;
            for (x, value) in [
                (75, *description),
                (300, if index == 0 { "2" } else { "3" }),
                (410, if index == 0 { "15" } else { "20" }),
                (490, if index == 0 { "30" } else { "60" }),
            ] {
                let _ = writeln!(stream, "BT /F1 12 Tf {x} {y} Td ({value}) Tj ET");
            }
        }
        let _ = writeln!(stream, "BT /F1 12 Tf 490 232 Td ({total}) Tj ET");
        for y in [542, 482, 422] {
            let _ = writeln!(stream, "50 {y} m 562 {y} l S");
        }
        stream
    }

    let first = page_stream("FIRST PAGE", ["Alpha", "Beta"], "90");
    let second = page_stream("SECOND PAGE", ["Decoy", "Ignore"], "999");
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

fn vertical_anchor(name: &str, offset: f64) -> PdfCommand {
    PdfCommand::Anchor(PdfAnchorAssignment {
        name: name.into(),
        axis: PdfAnchorAxis::Vertical,
        at: PdfCoordinate::new(PdfReference::Top, offset),
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
        "Statement",
        PdfPageSelection::All,
        vec![PdfCommand::Pages(PdfPages {
            selection: PdfPageSelection::First,
            children: vec![
                vertical_anchor("TableTop", 250.0),
                vertical_anchor("TableBottom", 370.0),
                capture(
                    "Header",
                    PdfRegion {
                        left: PdfCoordinate::new(PdfReference::Left, -0.0),
                        top: PdfCoordinate::new(PdfReference::Top, 40.0),
                        right: PdfCoordinate::new(
                            PdfReference::Right,
                            -f64::from_bits(AWKWARD_FINITE),
                        ),
                        bottom: PdfCoordinate::new(PdfReference::Top, 100.0),
                    },
                ),
                PdfCommand::EdgeRows(PdfEdgeRows {
                    region: PdfRegion {
                        left: PdfCoordinate::new(PdfReference::Left, 50.0),
                        top: PdfCoordinate::edge(PdfReference::Anchor("TableTop".into())),
                        right: PdfCoordinate::new(PdfReference::Left, 562.0),
                        bottom: PdfCoordinate::new(
                            PdfReference::Anchor("TableBottom".into()),
                            -0.0,
                        ),
                    },
                    find: PdfEdgeFind {
                        fill: f64::from_bits(AWKWARD_FINITE),
                        prominence: 100.0,
                    },
                    minimum_extent: None,
                    fallback_anchor: None,
                    children: vec![PdfCommand::GroupPerPage(PdfGroup {
                        name: "Service".into(),
                        region: PdfRegion::full(),
                        children: vec![
                            horizontal_anchor("Sep1", PdfReference::Left, 0.0),
                            horizontal_anchor("Sep2", PdfReference::Anchor("Sep1".into()), 220.0),
                            horizontal_anchor("Sep3", PdfReference::Anchor("Sep2".into()), 120.0),
                            horizontal_anchor("Sep4", PdfReference::Anchor("Sep3".into()), 80.0),
                            horizontal_anchor("Sep5", PdfReference::Anchor("Sep4".into()), 80.0),
                            column_capture("Description", "Sep1", "Sep2"),
                            column_capture("Hours", "Sep2", "Sep3"),
                            column_capture("Rate", "Sep3", "Sep4"),
                            column_capture("Amount", "Sep4", "Sep5"),
                        ],
                    })],
                }),
                capture(
                    "Total",
                    PdfRegion {
                        left: PdfCoordinate::new(PdfReference::Left, 450.0),
                        top: PdfCoordinate::new(PdfReference::Top, 530.0),
                        right: PdfCoordinate::new(PdfReference::Left, 560.0),
                        bottom: PdfCoordinate::new(PdfReference::Top, 590.0),
                    },
                ),
            ],
        })],
    )
    .unwrap()
}

fn project(layout: PdfLayout) -> Project {
    Project {
        source: layout.schema(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Header", ScalarType::String),
                SchemaNode::group(
                    "Service",
                    ["Description", "Hours", "Rate", "Amount"]
                        .into_iter()
                        .map(|name| SchemaNode::scalar(name, ScalarType::String))
                        .collect(),
                )
                .repeating(),
                SchemaNode::scalar("Total", ScalarType::String),
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
            nodes: ["Header", "Total", "Description", "Hours", "Rate", "Amount"]
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
            bindings: vec![
                Binding {
                    target_field: "Header".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Total".into(),
                    node: 1,
                },
            ],
            children: vec![Scope {
                target_field: "Service".into(),
                iteration: ScopeIteration::Source(vec!["Service".into()]),
                bindings: ["Description", "Hours", "Rate", "Amount"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| Binding {
                        target_field: name.into(),
                        node: index as u32 + 2,
                    })
                    .collect(),
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn assert_statement(instance: &Instance) {
    assert_eq!(
        instance.field("Header").and_then(Instance::as_scalar),
        Some(&Value::String("FIRST PAGE".into()))
    );
    assert_eq!(
        instance.field("Total").and_then(Instance::as_scalar),
        Some(&Value::String("90".into()))
    );
    let rows = instance
        .field("Service")
        .and_then(Instance::as_repeated)
        .expect("the ruled table must produce Service rows");
    assert_eq!(rows.len(), 2);
    for (row, expected) in rows
        .iter()
        .zip([["Alpha", "2", "15", "30"], ["Beta", "3", "20", "60"]])
    {
        for (name, value) in ["Description", "Hours", "Rate", "Amount"]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                row.field(name).and_then(Instance::as_scalar),
                Some(&Value::String(value.into())),
                "unexpected {name} in {row:?}"
            );
        }
    }
}

fn assert_exact_layout_bits(actual: &PdfLayout) {
    assert_eq!(actual, &layout());
    assert_eq!(actual.page_selection(), PdfPageSelection::All);
    let [PdfCommand::Pages(page)] = actual.commands() else {
        panic!("expected exactly one selected first-page block");
    };
    assert_eq!(page.selection, PdfPageSelection::First);
    let [
        PdfCommand::Anchor(top),
        PdfCommand::Anchor(bottom),
        PdfCommand::Capture(header),
        PdfCommand::EdgeRows(rows),
        PdfCommand::Capture(total),
    ] = page.children.as_slice()
    else {
        panic!("expected anchors, a header, ruled rows, and a footer");
    };
    assert_eq!(top.name, "TableTop");
    assert_eq!(top.axis, PdfAnchorAxis::Vertical);
    assert_eq!(top.at.offset.to_bits(), 250.0_f64.to_bits());
    assert_eq!(bottom.name, "TableBottom");
    assert_eq!(bottom.axis, PdfAnchorAxis::Vertical);
    assert_eq!(bottom.at.offset.to_bits(), 370.0_f64.to_bits());
    assert_eq!(header.region.left.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(
        header.region.right.offset.to_bits(),
        (-f64::from_bits(AWKWARD_FINITE)).to_bits()
    );
    assert_eq!(rows.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(rows.find.fill.to_bits(), AWKWARD_FINITE);
    assert!(rows.minimum_extent.is_none());
    assert!(rows.fallback_anchor.is_none());
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        panic!("expected one repeated Service group");
    };
    assert_eq!(group.name, "Service");
    assert_eq!(group.children.len(), 9);
    assert!(
        matches!(group.children.last(), Some(PdfCommand::Capture(capture)) if capture.name == "Amount")
    );
    assert_eq!(total.name, "Total");
}

fn changed_layout(edit: impl FnOnce(&mut Vec<PdfCommand>)) -> PdfLayout {
    let mut commands = layout().commands().to_vec();
    edit(&mut commands);
    PdfLayout::new("Statement", PdfPageSelection::All, commands).unwrap()
}

#[test]
fn first_page_anchored_rows_map_two_physical_pdf_rows_through_two_strict_cycles()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let pdf = two_page_ruled_pdf();
    let mut current = project(layout());
    let mut first_source = None;
    let mut first_output = None;

    for cycle in 0..=2 {
        let source_layout = current.source_options.pdf.as_ref().unwrap();
        assert_exact_layout_bits(source_layout);
        let source = format_pdf::from_bytes(&pdf, source_layout)?;
        assert_statement(&source);
        let output = engine::run(&current, &source)?;
        assert_statement(&output);
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

        let design = temp.0.join(format!("anchors-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &design)?;
        assert!(report.is_native_compatible(), "{report}");
        mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd)?;
        let template = std::fs::read_to_string(temp.0.join(format!("anchors-{cycle}-source.pxt")))?;
        assert!(template.contains("<VerticalAnchorAssignment"));
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
fn unsupported_anchor_row_shapes_reject_atomically() -> Result<(), Box<dyn std::error::Error>> {
    let second = NonZeroU32::new(2).unwrap();
    let non_first_page = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        page.selection = PdfPageSelection::Range {
            first: second,
            last: second,
        };
    });
    let misordered_anchor = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        page.children.swap(1, 2);
    });
    let narrowed_group = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut page.children[3] else {
            unreachable!()
        };
        let PdfCommand::GroupPerPage(group) = &mut rows.children[0] else {
            unreachable!()
        };
        group.region.left.offset = 1.0;
    });
    let changed_fallback = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut page.children[3] else {
            unreachable!()
        };
        rows.fallback_anchor = Some(PdfRegion::full());
    });
    let extra_row_group = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::EdgeRows(rows) = &mut page.children[3] else {
            unreachable!()
        };
        let PdfCommand::GroupPerPage(mut extra) = rows.children[0].clone() else {
            unreachable!()
        };
        extra.name = "Extra".into();
        rows.children.push(PdfCommand::GroupPerPage(extra));
    });
    let unsafe_anchor = changed_layout(|commands| {
        let PdfCommand::Pages(page) = &mut commands[0] else {
            unreachable!()
        };
        let PdfCommand::Anchor(anchor) = &mut page.children[0] else {
            unreachable!()
        };
        anchor.name = "Table Top".into();
        let PdfCommand::EdgeRows(rows) = &mut page.children[3] else {
            unreachable!()
        };
        rows.region.top.reference = PdfReference::Anchor("Table Top".into());
    });

    let temp = TempDir::new()?;
    for (name, layout) in [
        ("non-first-page", non_first_page),
        ("misordered-anchor", misordered_anchor),
        ("narrowed-group", narrowed_group),
        ("changed-fallback", changed_fallback),
        ("extra-row-group", extra_row_group),
        ("unsafe-anchor", unsafe_anchor),
    ] {
        let project = project(layout);
        let design = temp.0.join(format!("{name}.mfd"));
        let sibling = temp.0.join(format!("{name}-source.pxt"));
        let report = mfd::preflight_export(&project, &design)?;
        assert!(
            report.issues.iter().any(|issue| {
                issue.feature == ExportCompatibilityFeature::PdfLayout
                    && issue.component == "Statement"
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
