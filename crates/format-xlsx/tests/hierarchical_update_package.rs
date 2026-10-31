//! Package preflight of hierarchical XLSX updates: real-byte expansion, XML
//! budgets, and the relationship walk, exercised against self-authored
//! workbooks rewritten in each test.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use calamine::{Data, Reader, Xlsx};
use format_xlsx::{
    HierarchicalUpdateError, HierarchicalUpdateRefusal, XlsxFormatError, XlsxLimit,
    XlsxLimitOptions, XlsxLimits, update_hierarchical, update_hierarchical_bytes,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    XlsxCellKind, XlsxColumn, XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange,
    XlsxRangeStart, XlsxRow,
};
use rust_xlsxwriter::{Workbook, Worksheet};

/// A per-test directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-xlsx-hierarchical-package-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn workbook(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("report.xlsx");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn entries(&self) -> Vec<String> {
        let mut names = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Workbook",
        vec![
            SchemaNode::group(
                "Sheets",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::group(
                        "People",
                        vec![SchemaNode::scalar("Display", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

/// One open-ended band: a header in B5 and people from B6 down.
fn layout() -> XlsxHierarchicalLayout {
    XlsxHierarchicalLayout {
        worksheets_path: vec!["Sheets".into()],
        worksheet_name_path: vec!["Name".into()],
        ranges: vec![XlsxOutputRange {
            path: vec!["People".into()],
            start: XlsxRangeStart::Absolute {
                row: XlsxRow::new(5).unwrap(),
            },
            count: None,
            has_header: true,
            columns: vec![XlsxOutputColumn {
                path: vec!["Display".into()],
                column: XlsxColumn::new(2).unwrap(),
                header: Some("Person".into()),
                kind: XlsxCellKind::String,
            }],
        }],
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn people(count: usize) -> Vec<Instance> {
    (1..=count)
        .map(|rank| {
            group(vec![(
                "Display",
                Instance::Scalar(Value::String(format!("P{rank}"))),
            )])
        })
        .collect()
}

fn north_only(people: Vec<Instance>) -> Instance {
    group(vec![(
        "Sheets",
        Instance::Repeated(vec![group(vec![
            ("Name", Instance::Scalar(Value::String("North".into()))),
            ("People", Instance::Repeated(people)),
        ])]),
    )])
}

fn limits() -> XlsxLimits {
    XlsxLimits::default()
}

fn template_with(customize: impl FnOnce(&mut Worksheet)) -> Vec<u8> {
    let mut book = Workbook::new();
    let north = book.add_worksheet();
    north.set_name("North").unwrap();
    customize(north);
    book.add_worksheet().set_name("South").unwrap();
    book.save_to_buffer().unwrap()
}

/// Every string value as `(one-based row, one-based column) -> s:value`.
fn cells(bytes: &[u8], sheet: &str) -> BTreeMap<(u32, u32), String> {
    let mut workbook = Xlsx::new(Cursor::new(bytes)).unwrap();
    let range = workbook.worksheet_range(sheet).unwrap();
    let (first_row, first_column) = range.start().unwrap_or((0, 0));
    range
        .used_cells()
        .filter_map(|(row, column, data)| match data {
            Data::String(value) => Some((
                (first_row + row as u32 + 1, first_column + column as u32 + 1),
                format!("s:{value}"),
            )),
            _ => None,
        })
        .collect()
}

/// Runs a failing update through the filesystem API and checks that the
/// workbook bytes are unchanged and no staging file remains.
fn refuse(
    name: &str,
    template: &[u8],
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
    limits: &XlsxLimits,
) -> HierarchicalUpdateError {
    let scratch = Scratch::new(name);
    let path = scratch.workbook(template);
    let error = update_hierarchical(&path, &schema(), instance, layout, limits).unwrap_err();
    assert_eq!(std::fs::read(&path).unwrap(), template, "{name}");
    assert_eq!(scratch.entries(), ["report.xlsx"], "{name}");
    error
}

/// Rewrites a package: `edit` may rename or replace each entry, and `extra`
/// entries are appended.
fn rewrite_package(
    bytes: &[u8],
    edit: impl Fn(&str, String) -> (String, String),
    extra: &[(&str, &str)],
) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        let (name, content) = edit(entry.name(), content);
        output.start_file(name, options).unwrap();
        output.write_all(content.as_bytes()).unwrap();
    }
    for (name, content) in extra {
        output.start_file(*name, options).unwrap();
        output.write_all(content.as_bytes()).unwrap();
    }
    output.finish().unwrap().into_inner()
}

/// Appends one relationship to the relationships part named `part`.
fn add_relationship(content: String, relationship: &str, target: &str) -> String {
    content.replace(
        "</Relationships>",
        &format!(r#"<Relationship Id="rIdExtra" Type="{relationship}" Target="{target}"/></Relationships>"#),
    )
}

const CUSTOM_XML: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";

fn with_workbook_relationship(bytes: &[u8], relationship: &str) -> Vec<u8> {
    rewrite_package(
        bytes,
        |name, content| {
            let content = if name == "xl/_rels/workbook.xml.rels" {
                add_relationship(content, relationship, "../customXml/item1.xml")
            } else {
                content
            };
            (name.to_string(), content)
        },
        &[("customXml/item1.xml", "<item/>")],
    )
}

#[test]
fn refuses_packages_with_parts_the_writer_cannot_preserve() {
    let relationship =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
    let template = with_workbook_relationship(&template_with(|_| {}), relationship);

    let error = refuse(
        "unsupported-part",
        &template,
        &north_only(people(1)),
        &layout(),
        &limits(),
    );

    assert!(
        matches!(
            &error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::UnsupportedPart { part, relationship: found })
                if part == "xl/_rels/workbook.xml.rels" && found == relationship
        ),
        "{error}"
    );
}

fn assert_unsupported(error: &HierarchicalUpdateError, expected_part: &str) {
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::UnsupportedPart { part, .. })
                if part == expected_part
        ),
        "{error}"
    );
}

#[test]
fn relationship_checks_follow_the_readers_part_lookup() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));

    // A case variant of the workbook relationships is still what the reader loads.
    let case_variant = rewrite_package(
        &base,
        |name, content| {
            if name == "xl/_rels/workbook.xml.rels" {
                let content = add_relationship(content, CUSTOM_XML, "../customXml/item1.xml");
                ("XL/_rels/Workbook.xml.rels".to_string(), content)
            } else {
                (name.to_string(), content)
            }
        },
        &[("customXml/item1.xml", "<item/>")],
    );
    let error = refuse(
        "case-variant",
        &case_variant,
        &instance,
        &layout(),
        &limits(),
    );
    assert_unsupported(&error, "XL/_rels/Workbook.xml.rels");

    // Backslash names resolve in the reader too, so they are refused outright.
    let backslash = rewrite_package(
        &base,
        |name, content| (name.replace("xl/styles.xml", "xl\\styles.xml"), content),
        &[],
    );
    let error = refuse("backslash", &backslash, &instance, &layout(), &limits());
    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Update(message)) if message.contains("xl\\styles.xml")),
        "{error}"
    );

    // The reader drops the relationships of a worksheet outside xl/worksheets.
    let relocated = rewrite_package(
        &base,
        |name, content| {
            let content = content.replace("worksheets/sheet1.xml", "sheets/sheet1.xml");
            (
                name.replace("worksheets/sheet1.xml", "sheets/sheet1.xml"),
                content,
            )
        },
        &[(
            "xl/sheets/_rels/sheet1.xml.rels",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{CUSTOM_XML}" Target="../../customXml/item1.xml"/></Relationships>"#
            ),
        )],
    );
    let error = refuse(
        "relocated-sheet",
        &relocated,
        &instance,
        &layout(),
        &limits(),
    );
    assert_unloadable(&error, "xl/sheets/_rels/sheet1.xml.rels");
}

fn chart_template() -> Vec<u8> {
    template_with(|north| {
        north.write_number(19, 7, 1.0).unwrap();
        north.write_number(20, 7, 2.0).unwrap();
        let mut chart = rust_xlsxwriter::Chart::new(rust_xlsxwriter::ChartType::Column);
        chart.add_series().set_values("North!$H$20:$H$21");
        north.insert_chart(22, 7, &chart).unwrap();
    })
}

#[test]
fn checks_drawing_relationships() {
    let instance = north_only(people(2));
    let (updated, _) = update_hierarchical_bytes(
        &chart_template(),
        &schema(),
        &instance,
        &layout(),
        &limits(),
    )
    .unwrap();
    assert_eq!(
        cells(&updated, "North").get(&(6, 2)),
        Some(&"s:P1".to_string())
    );

    let diagram = rewrite_package(
        &chart_template(),
        |name, content| {
            let content = if name == "xl/drawings/_rels/drawing1.xml.rels" {
                add_relationship(
                    content,
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/diagramData",
                    "../diagrams/data1.xml",
                )
            } else {
                content
            };
            (name.to_string(), content)
        },
        &[("xl/diagrams/data1.xml", "<data/>")],
    );
    let error = refuse(
        "drawing-relationship",
        &diagram,
        &instance,
        &layout(),
        &limits(),
    );
    assert_unsupported(&error, "xl/drawings/_rels/drawing1.xml.rels");
}

/// Overwrites the central-directory expanded size of `entry`.
fn with_declared_size(bytes: &[u8], entry: &str, size: u32) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    let header = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"PK\x01\x02")
        .map(|(offset, _)| offset)
        .find(|offset| {
            let length = u16::from_le_bytes([bytes[offset + 28], bytes[offset + 29]]) as usize;
            &bytes[offset + 46..offset + 46 + length] == entry.as_bytes()
        })
        .unwrap();
    bytes[header + 24..header + 28].copy_from_slice(&size.to_le_bytes());
    bytes
}

#[test]
fn package_preflight_counts_real_bytes_and_validates_xml() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));

    let declared = with_declared_size(&base, "xl/styles.xml", 1_000_000);
    let error = refuse("declared-size", &declared, &instance, &layout(), &limits());
    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Update(message)) if message.contains("disagrees with the ZIP directory")),
        "{error}"
    );

    // Declared sizes stay small; the real expansion exceeds the budget.
    let small = XlsxLimits::new(XlsxLimitOptions {
        max_expanded_bytes: 2_000,
        ..XlsxLimitOptions::default()
    })
    .unwrap();
    let understated = with_declared_size(&base, "xl/styles.xml", 1);
    let error = refuse("understated", &understated, &instance, &layout(), &small);
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::ExpandedBytes,
                ..
            })
        ),
        "{error}"
    );

    let missing_target = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/_rels/workbook.xml.rels" {
                content.replace(
                    "</Relationships>",
                    r#"<Relationship Id="rIdExtra" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"/></Relationships>"#,
                )
            } else {
                content
            };
            (name.to_string(), content)
        },
        &[],
    );
    let error = refuse(
        "missing-target",
        &missing_target,
        &instance,
        &layout(),
        &limits(),
    );
    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Update(message)) if message.contains("Target")),
        "{error}"
    );

    let malformed = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/styles.xml" {
                content.replace("</styleSheet>", "")
            } else {
                content
            };
            (name.to_string(), content)
        },
        &[],
    );
    let error = refuse("malformed-xml", &malformed, &instance, &layout(), &limits());
    assert!(
        matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Update(message)) if message.contains("xl/styles.xml")),
        "{error}"
    );

    let few_nodes = XlsxLimits::new(XlsxLimitOptions {
        max_xml_nodes: 20,
        ..XlsxLimitOptions::default()
    })
    .unwrap();
    let error = refuse("xml-nodes", &base, &instance, &layout(), &few_nodes);
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::XmlNodes,
                ..
            })
        ),
        "{error}"
    );
}

const SHEET_RELATIONSHIPS: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const COMMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#;

fn office(kind: &str) -> String {
    format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}")
}

/// A relationships part holding `(type, target)` relationships in order.
fn relationships_part(entries: &[(&str, &str)]) -> String {
    let relationships = entries
        .iter()
        .enumerate()
        .map(|(index, (kind, target))| {
            format!(
                r#"<Relationship Id="rId{}" Type="{kind}" Target="{target}"/>"#,
                index + 1
            )
        })
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
    )
}

fn unchanged(name: &str, content: String) -> (String, String) {
    (name.to_string(), content)
}

fn assert_update_error(error: &HierarchicalUpdateError, expected: &[&str]) {
    assert!(
        matches!(error, HierarchicalUpdateError::Format(XlsxFormatError::Update(message))
            if expected.iter().all(|part| message.contains(part))),
        "{error}"
    );
}

fn assert_unloadable(error: &HierarchicalUpdateError, expected_part: &str) {
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::UnloadableRelationships { part })
                if part == expected_part
        ),
        "{error}"
    );
}

#[test]
fn refuses_relationship_cycles_and_repeated_loads() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));

    // The reader follows every relationship without a visited set.
    for (target, part) in [
        ("sheet1.xml", "xl/worksheets/sheet1.xml"),
        ("../workbook.xml", "xl/workbook.xml"),
    ] {
        let comments = office("comments");
        let cyclic = rewrite_package(
            &base,
            unchanged,
            &[(
                SHEET_RELATIONSHIPS,
                &relationships_part(&[(&comments, target)]),
            )],
        );
        let error = refuse("cycle", &cyclic, &instance, &layout(), &limits());
        assert!(
            matches!(
                &error,
                HierarchicalUpdateError::Refused(HierarchicalUpdateRefusal::RelationshipCycle { part: found })
                    if found == part
            ),
            "{error}"
        );
    }

    // Each relationship loads its own copy of the target. Comments need no
    // reference from the worksheet.
    let comments = office("comments");
    let padded = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><!--{}--></comments>"#,
        "x".repeat(20_000)
    );
    let repeated = |count: usize| {
        let entries = vec![(comments.as_str(), "../comments1.xml"); count];
        rewrite_package(
            &base,
            unchanged,
            &[
                (SHEET_RELATIONSHIPS, &relationships_part(&entries)),
                ("xl/comments1.xml", &padded),
            ],
        )
    };
    let budget = XlsxLimits::new(XlsxLimitOptions {
        max_expanded_bytes: 100_000,
        ..XlsxLimitOptions::default()
    })
    .unwrap();
    update_hierarchical_bytes(&repeated(1), &schema(), &instance, &layout(), &budget).unwrap();
    let error = refuse("repeated", &repeated(10), &instance, &layout(), &budget);
    assert!(
        matches!(
            error,
            HierarchicalUpdateError::Format(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::ExpandedBytes,
                ..
            })
        ),
        "{error}"
    );

    // Two sheets naming one worksheet relationship load it twice.
    let shared = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/workbook.xml" {
                content.replace(r#"r:id="rId2""#, r#"r:id="rId1""#)
            } else {
                content
            };
            unchanged(name, content)
        },
        &[],
    );
    let error = refuse("shared-sheet", &shared, &instance, &layout(), &limits());
    assert_update_error(&error, &["rId1", "more than one sheet"]);
}

#[test]
fn refuses_relationships_the_reader_would_not_load() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));
    let comments = office("comments");
    let with_comments = relationships_part(&[(&comments, "../comments1.xml")]);

    // The reader uses a worksheet's relationships only below
    // `xl/worksheets/_rels/sheet`, case-sensitively, by the resolved target.
    for (sheet, target) in [
        ("xl/worksheets/Sheet1.xml", "worksheets/Sheet1.xml"),
        ("xl/worksheets/data.xml", "worksheets/data.xml"),
    ] {
        let (directory, file) = sheet.rsplit_once('/').unwrap();
        let relationships = format!("{directory}/_rels/{file}.rels");
        let renamed = rewrite_package(
            &base,
            |name, content| {
                let content = content.replace("worksheets/sheet1.xml", target);
                (name.replace("xl/worksheets/sheet1.xml", sheet), content)
            },
            &[
                (&relationships, &with_comments),
                ("xl/comments1.xml", COMMENTS),
            ],
        );
        let error = refuse("renamed-sheet", &renamed, &instance, &layout(), &limits());
        assert_unloadable(&error, &relationships);
    }
    let upper_target = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/_rels/workbook.xml.rels" {
                content.replace(
                    r#"Target="worksheets/sheet1.xml""#,
                    r#"Target="/XL/worksheets/sheet1.xml""#,
                )
            } else {
                content
            };
            unchanged(name, content)
        },
        &[
            (SHEET_RELATIONSHIPS, &with_comments),
            ("xl/comments1.xml", COMMENTS),
        ],
    );
    let error = refuse(
        "upper-target",
        &upper_target,
        &instance,
        &layout(),
        &limits(),
    );
    assert_unloadable(&error, SHEET_RELATIONSHIPS);

    // Likewise drawings below `xl/drawings/_rels/drawing`.
    let drawing = rewrite_package(
        &chart_template(),
        |name, content| {
            (
                name.replace("drawing1.xml", "Drawing1.xml"),
                content.replace("drawings/drawing1.xml", "drawings/Drawing1.xml"),
            )
        },
        &[],
    );
    let error = refuse("renamed-drawing", &drawing, &instance, &layout(), &limits());
    assert_unloadable(&error, "xl/drawings/_rels/Drawing1.xml.rels");

    // The writer never re-emits relationships of a comments part.
    let leaf = rewrite_package(
        &base,
        unchanged,
        &[
            (SHEET_RELATIONSHIPS, &with_comments),
            ("xl/comments1.xml", COMMENTS),
            ("xl/_rels/comments1.xml.rels", &relationships_part(&[])),
        ],
    );
    let error = refuse("leaf", &leaf, &instance, &layout(), &limits());
    assert_unloadable(&error, "xl/_rels/comments1.xml.rels");

    // The reader keeps only the first VML drawing's relationships.
    let vml = office("vmlDrawing");
    let image = relationships_part(&[(&office("image"), "../media/image1.png")]);
    let two_vml = rewrite_package(
        &base,
        with_sheet_elements(r#"<legacyDrawing r:id="rId1"/><legacyDrawing r:id="rId2"/>"#),
        &[
            (
                SHEET_RELATIONSHIPS,
                &relationships_part(&[
                    (&vml, "../drawings/vmlDrawing1.vml"),
                    (&vml, "../drawings/vmlDrawing2.vml"),
                    (&comments, "../comments1.xml"),
                ]),
            ),
            ("xl/comments1.xml", COMMENTS),
            ("xl/drawings/vmlDrawing1.vml", &vml_drawing(NOTE_SHAPE)),
            ("xl/drawings/vmlDrawing2.vml", &vml_drawing(NOTE_SHAPE)),
            ("xl/drawings/_rels/vmlDrawing1.vml.rels", &image),
            ("xl/drawings/_rels/vmlDrawing2.vml.rels", &image),
            ("xl/media/image1.png", "png"),
        ],
    );
    let error = refuse("two-vml", &two_vml, &instance, &layout(), &limits());
    assert_unloadable(&error, "xl/drawings/_rels/vmlDrawing2.vml.rels");
}

#[test]
fn resolves_relationship_targets_exactly_as_the_reader() {
    let instance = north_only(people(1));
    let sheet_relationships = |edit: &dyn Fn(String) -> String| {
        rewrite_package(
            &chart_template(),
            |name, content| {
                let content = if name == SHEET_RELATIONSHIPS {
                    edit(content)
                } else {
                    content
                };
                unchanged(name, content)
            },
            &[],
        )
    };

    // The reader takes attribute values raw, so references would not resolve.
    for (from, to) in [
        ("drawing1.xml", "drawing&#49;.xml"),
        ("relationships/drawing", "relationships/dr&#97;wing"),
    ] {
        let escaped = sheet_relationships(&|content| content.replace(from, to));
        let error = refuse("escaped", &escaped, &instance, &layout(), &limits());
        assert_update_error(&error, &[SHEET_RELATIONSHIPS, "escaped"]);
    }

    let backslash = sheet_relationships(&|content| {
        content.replace("../drawings/drawing1.xml", r"..\drawings\drawing1.xml")
    });
    let error = refuse("backslash", &backslash, &instance, &layout(), &limits());
    assert_update_error(&error, &[SHEET_RELATIONSHIPS, "backslash"]);

    let comments = office("comments");
    let unresolved =
        sheet_relationships(&|content| add_relationship(content, &comments, "../comments9.xml"));
    let error = refuse("unresolved", &unresolved, &instance, &layout(), &limits());
    assert_update_error(
        &error,
        &["../comments9.xml", "does not name a package part"],
    );

    // The reader ignores prefixed elements, dropping their parts.
    let prefixed = sheet_relationships(&|content| {
        content.replace(
            "<Relationship ",
            r#"<r:Relationship xmlns:r="http://schemas.openxmlformats.org/package/2006/relationships" "#,
        )
    });
    let error = refuse("prefixed", &prefixed, &instance, &layout(), &limits());
    assert_update_error(&error, &[SHEET_RELATIONSHIPS, "unprefixed"]);
}

#[test]
fn validates_reached_parts_as_xml_whatever_their_names() {
    let renamed = rewrite_package(
        &template_with(|_| {}),
        |name, content| {
            let content = if name == "xl/worksheets/sheet1.xml" {
                content.replace("</worksheet>", "")
            } else {
                content.replace("worksheets/sheet1.xml", "worksheets/sheet1.bin")
            };
            (
                name.replace("worksheets/sheet1.xml", "worksheets/sheet1.bin"),
                content,
            )
        },
        &[],
    );

    let error = refuse(
        "binary-name",
        &renamed,
        &north_only(people(1)),
        &layout(),
        &limits(),
    );

    assert_update_error(&error, &["xl/worksheets/sheet1.bin", "well-formed"]);
}

const NOTE_SHAPE: &str = r##"<v:shape id="_x0000_s1025" type="#_x0000_t202"><v:textbox/><x:ClientData ObjectType="Note"><x:Row>0</x:Row><x:Column>0</x:Column></x:ClientData></v:shape>"##;

fn vml_drawing(shapes: &str) -> String {
    format!(
        r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">{shapes}</xml>"#
    )
}

/// An `edit` for [`rewrite_package`] that appends `elements` to North's
/// worksheet XML.
fn with_sheet_elements(elements: &str) -> impl Fn(&str, String) -> (String, String) + '_ {
    move |name, content| {
        let content = if name == "xl/worksheets/sheet1.xml" {
            content.replace("</worksheet>", &format!("{elements}</worksheet>"))
        } else {
            content
        };
        unchanged(name, content)
    }
}

fn assert_refused(error: &HierarchicalUpdateError, expected: &HierarchicalUpdateRefusal) {
    assert!(
        matches!(error, HierarchicalUpdateError::Refused(found) if found == expected),
        "{error}"
    );
}

#[test]
fn refuses_references_and_vml_shapes_the_writer_drops() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));
    let vml = office("vmlDrawing");
    let comments = office("comments");
    let image = relationships_part(&[(&office("image"), "../media/image1.png")]);
    let header_picture = vml_drawing(
        r#"<v:shape id="LH" style="position:absolute"><v:imagedata o:relid="rId1" o:title="logo"/></v:shape>"#,
    );
    let with_vml = |elements: &str, shapes: &str, with_comments: bool| {
        let mut relationships = vec![(vml.as_str(), "../drawings/vmlDrawing1.vml")];
        if with_comments {
            relationships.push((comments.as_str(), "../comments1.xml"));
        }
        rewrite_package(
            &base,
            with_sheet_elements(elements),
            &[
                (SHEET_RELATIONSHIPS, &relationships_part(&relationships)),
                ("xl/comments1.xml", COMMENTS),
                ("xl/drawings/vmlDrawing1.vml", shapes),
                ("xl/drawings/_rels/vmlDrawing1.vml.rels", &image),
                ("xl/media/image1.png", "png"),
            ],
        )
    };
    let unsupported_vml = HierarchicalUpdateRefusal::UnsupportedVmlShape {
        part: "xl/drawings/vmlDrawing1.vml".into(),
    };

    // A print header/footer picture: the reader never reads legacyDrawingHF.
    let footer = with_vml(r#"<legacyDrawingHF r:id="rId1"/>"#, &header_picture, false);
    let error = refuse("header-footer", &footer, &instance, &layout(), &limits());
    assert_refused(
        &error,
        &HierarchicalUpdateRefusal::UnsupportedReference {
            part: "xl/worksheets/sheet1.xml".into(),
            element: "legacyDrawingHF".into(),
        },
    );

    // VML shapes survive only as comment notes of a worksheet with comments.
    let button = vml_drawing(&NOTE_SHAPE.replace("Note", "Button"));
    for (name, shapes, with_comments) in [
        ("picture-shape", header_picture.as_str(), true),
        ("button-shape", button.as_str(), true),
        ("note-without-comments", &vml_drawing(NOTE_SHAPE), false),
    ] {
        let template = with_vml(r#"<legacyDrawing r:id="rId1"/>"#, shapes, with_comments);
        let error = refuse(name, &template, &instance, &layout(), &limits());
        assert_refused(&error, &unsupported_vml);
    }

    // The reader loads printer settings only through pageSetup.
    let printer = rewrite_package(
        &base,
        unchanged,
        &[
            (
                SHEET_RELATIONSHIPS,
                &relationships_part(&[(
                    &office("printerSettings"),
                    "../printerSettings/printerSettings1.bin",
                )]),
            ),
            ("xl/printerSettings/printerSettings1.bin", "settings"),
        ],
    );
    let error = refuse("unreferenced", &printer, &instance, &layout(), &limits());
    assert_refused(
        &error,
        &HierarchicalUpdateRefusal::UnreferencedRelationship {
            part: SHEET_RELATIONSHIPS.into(),
            id: "rId1".into(),
        },
    );

    // A reference must name a relationship of its element's type.
    let wrong_type = rewrite_package(
        &base,
        with_sheet_elements(r#"<drawing r:id="rId1"/>"#),
        &[
            (
                SHEET_RELATIONSHIPS,
                &relationships_part(&[(&office("table"), "../tables/table1.xml")]),
            ),
            ("xl/tables/table1.xml", "<table/>"),
        ],
    );
    let error = refuse("wrong-type", &wrong_type, &instance, &layout(), &limits());
    assert_refused(
        &error,
        &HierarchicalUpdateRefusal::UnsupportedReference {
            part: "xl/worksheets/sheet1.xml".into(),
            element: "drawing".into(),
        },
    );
}

/// The names of the package entries that start with `prefix`.
fn entries_with(bytes: &[u8], prefix: &str) -> Vec<String> {
    let archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    archive
        .file_names()
        .filter(|name| name.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

fn entry(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut content = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    content
}

#[test]
fn comment_drawings_survive_an_update() {
    let template = template_with(|north| {
        let note = rust_xlsxwriter::Note::new("Keep this note").set_author("Author");
        north.insert_note(0, 7, &note).unwrap();
    });

    let (updated, _) = update_hierarchical_bytes(
        &template,
        &schema(),
        &north_only(people(2)),
        &layout(),
        &limits(),
    )
    .unwrap();

    assert_eq!(
        cells(&updated, "North").get(&(7, 2)),
        Some(&"s:P2".to_string())
    );
    let comments = entries_with(&updated, "xl/comments");
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(entry(&updated, &comments[0]).contains("Keep this note"));
    let drawings = entries_with(&updated, "xl/drawings/vmlDrawing");
    assert_eq!(drawings.len(), 1, "{drawings:?}");
    let drawing = entry(&updated, &drawings[0]);
    assert!(drawing.contains(r#"ObjectType="Note""#), "{drawing}");
    assert!(drawing.contains("<x:Column>7</x:Column>"), "{drawing}");
    assert!(entry(&updated, "xl/worksheets/sheet1.xml").contains("<legacyDrawing "));
    // The updated workbook passes the same checks again.
    update_hierarchical_bytes(
        &updated,
        &schema(),
        &north_only(people(1)),
        &layout(),
        &limits(),
    )
    .unwrap();
}

#[test]
fn refuses_repeated_and_unnamed_relationship_ids() {
    let base = template_with(|_| {});
    let instance = north_only(people(1));
    let comments = office("comments");

    let repeated = rewrite_package(
        &base,
        unchanged,
        &[
            (
                SHEET_RELATIONSHIPS,
                &relationships_part(&[
                    (&comments, "../comments1.xml"),
                    (&comments, "../comments1.xml"),
                ])
                .replace("rId2", "rId1"),
            ),
            ("xl/comments1.xml", COMMENTS),
        ],
    );
    let error = refuse("repeated-id", &repeated, &instance, &layout(), &limits());
    assert_update_error(&error, &[SHEET_RELATIONSHIPS, "`rId1` is repeated"]);

    // The reader loads only worksheets a sheet names, so another is dropped.
    let unnamed = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/_rels/workbook.xml.rels" {
                add_relationship(content, &office("worksheet"), "worksheets/sheet2.xml")
            } else {
                content
            };
            unchanged(name, content)
        },
        &[],
    );
    let error = refuse("unnamed-sheet", &unnamed, &instance, &layout(), &limits());
    assert_refused(
        &error,
        &HierarchicalUpdateRefusal::UnreferencedRelationship {
            part: "xl/_rels/workbook.xml.rels".into(),
            id: "rIdExtra".into(),
        },
    );

    let missing = rewrite_package(
        &base,
        |name, content| {
            let content = if name == "xl/workbook.xml" {
                content.replace(
                    "</sheets>",
                    r#"<sheet name="East" sheetId="3" r:id="rId42"/></sheets>"#,
                )
            } else {
                content
            };
            unchanged(name, content)
        },
        &[],
    );
    let error = refuse("missing-sheet", &missing, &instance, &layout(), &limits());
    assert_update_error(&error, &["rId42", "names no internal worksheet"]);
}

#[cfg(unix)]
#[test]
fn refuses_targets_that_are_not_regular_files() {
    let scratch = Scratch::new("special");
    let socket = scratch.0.join("s");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let link = scratch.0.join("link");
    std::os::unix::fs::symlink(&socket, &link).unwrap();
    let directory = scratch.0.join("d");
    std::fs::create_dir(&directory).unwrap();
    let mut targets = vec![socket, link, directory];
    // Opening a FIFO for reading or writing would block.
    let fifo = scratch.0.join("f");
    if std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|status| status.success())
    {
        targets.push(fifo);
    }

    for target in &targets {
        let error = update_hierarchical(
            target,
            &schema(),
            &north_only(people(1)),
            &layout(),
            &limits(),
        )
        .unwrap_err();
        assert!(
            matches!(&error, HierarchicalUpdateError::Format(XlsxFormatError::Io(io)) if io.kind() == std::io::ErrorKind::InvalidInput),
            "{}: {error}",
            target.display()
        );
    }
    let mut expected = vec!["d", "link", "s"];
    if targets.len() == 4 {
        expected.insert(1, "f");
    }
    assert_eq!(scratch.entries(), expected);
}

/// [`chart_template`] with `edit` applied to the named part.
fn chart_template_with(part: &str, edit: impl Fn(String) -> String) -> Vec<u8> {
    rewrite_package(
        &chart_template(),
        |name, content| {
            let content = if name == part { edit(content) } else { content };
            unchanged(name, content)
        },
        &[("xl/media/image1.png", "png")],
    )
}

/// A second anchor holding `content`, appended to the chart drawing.
fn with_anchor(content: &str) -> impl Fn(String) -> String + '_ {
    move |drawing| {
        drawing.replace(
            "</xdr:wsDr>",
            &format!(
                r#"<xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>40</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>44</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>{content}<xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#
            ),
        )
    }
}

fn group_shape(content: &str) -> String {
    format!(
        r#"<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="9" name="Group"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr/>{content}</xdr:grpSp>"#
    )
}

const DRAWING: &str = "xl/drawings/drawing1.xml";
const CHART: &str = "xl/charts/chart1.xml";

#[test]
fn refuses_drawing_content_the_reader_drops() {
    let instance = north_only(people(1));
    let frame = {
        let drawing = entry(&chart_template(), DRAWING);
        let start = drawing.find("<xdr:graphicFrame").unwrap();
        let end = drawing.find("</xdr:graphicFrame>").unwrap() + "</xdr:graphicFrame>".len();
        drawing[start..end].to_string()
    };
    let alternate = |drawing: String| {
        drawing
            .replace(
                "<xdr:twoCellAnchor>",
                r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="a14"><xdr:twoCellAnchor>"#,
            )
            .replace(
                "</xdr:twoCellAnchor>",
                "</xdr:twoCellAnchor></mc:Choice></mc:AlternateContent>",
            )
    };
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "absolute-anchor",
            chart_template_with(DRAWING, |drawing| {
                drawing.replace("xdr:twoCellAnchor>", "xdr:absoluteAnchor>")
            }),
            "xdr:absoluteAnchor",
        ),
        (
            "alternate-content",
            chart_template_with(DRAWING, alternate),
            "mc:AlternateContent",
        ),
        (
            "chart-in-group",
            chart_template_with(DRAWING, with_anchor(&group_shape(&frame))),
            "xdr:graphicFrame",
        ),
        (
            "nested-group",
            chart_template_with(DRAWING, with_anchor(&group_shape(&group_shape("")))),
            "xdr:grpSp",
        ),
        (
            "frame-without-chart",
            chart_template_with(DRAWING, |drawing| {
                drawing.replace(r#" r:id="rId1"/>"#, r#"/>"#)
            }),
            "xdr:graphicFrame",
        ),
    ];
    for (name, template, element) in cases {
        let error = refuse(name, &template, &instance, &layout(), &limits());
        assert_refused(
            &error,
            &HierarchicalUpdateRefusal::UnsupportedDrawingContent {
                part: DRAWING.into(),
                element: element.into(),
            },
        );
    }

    // The writer rebuilds a drawing's relationships from what it kept.
    let unreferenced = chart_template_with("xl/drawings/_rels/drawing1.xml.rels", |content| {
        add_relationship(content, &office("image"), "../media/image1.png")
    });
    let error = refuse(
        "unreferenced-image",
        &unreferenced,
        &instance,
        &layout(),
        &limits(),
    );
    assert_refused(
        &error,
        &HierarchicalUpdateRefusal::UnreferencedRelationship {
            part: "xl/drawings/_rels/drawing1.xml.rels".into(),
            id: "rIdExtra".into(),
        },
    );
}

#[test]
fn refuses_charts_the_reader_does_not_model() {
    let instance = north_only(people(1));
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        // A combined chart would lose its stock series.
        (
            "stock-series",
            chart_template_with(CHART, |chart| {
                chart.replace(
                    "</c:barChart>",
                    r#"</c:barChart><c:stockChart><c:axId val="50010001"/><c:axId val="50010002"/></c:stockChart>"#,
                )
            }),
            "c:stockChart",
        ),
        // A chart with no modeled type would lose its whole anchor.
        (
            "surface-only",
            chart_template_with(CHART, |chart| chart.replace("c:barChart", "c:surfaceChart")),
            "c:surfaceChart",
        ),
        (
            "chart-text",
            chart_template_with(CHART, |chart| {
                chart.replace(
                    "<c:printSettings>",
                    "<c:txPr><a:bodyPr/><a:p><a:endParaRPr/></a:p></c:txPr><c:printSettings>",
                )
            }),
            "c:txPr",
        ),
        (
            "no-plot-area",
            chart_template_with(CHART, |chart| {
                let start = chart.find("<c:plotArea>").unwrap();
                let end = chart.find("</c:plotArea>").unwrap() + "</c:plotArea>".len();
                format!("{}{}", &chart[..start], &chart[end..])
            }),
            "c:plotArea",
        ),
    ];
    for (name, template, element) in cases {
        let error = refuse(name, &template, &instance, &layout(), &limits());
        assert_refused(
            &error,
            &HierarchicalUpdateRefusal::UnsupportedChart {
                part: CHART.into(),
                element: element.into(),
            },
        );
    }

    // Every modeled chart type is accepted, alone or combined.
    let line = chart_template_with(CHART, |chart| {
        chart.replace(
            "</c:barChart>",
            r#"</c:barChart><c:lineChart><c:grouping val="standard"/><c:axId val="50010001"/><c:axId val="50010002"/></c:lineChart>"#,
        )
    });
    update_hierarchical_bytes(&line, &schema(), &instance, &layout(), &limits()).unwrap();
}

#[test]
fn pictures_survive_an_update() {
    // A 1x1 PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let template = template_with(|north| {
        let image = rust_xlsxwriter::Image::new_from_buffer(PNG).unwrap();
        north.insert_image(0, 7, &image).unwrap();
    });

    let (updated, _) = update_hierarchical_bytes(
        &template,
        &schema(),
        &north_only(people(2)),
        &layout(),
        &limits(),
    )
    .unwrap();

    assert_eq!(entries_with(&updated, "xl/media/").len(), 1);
    let drawings = entries_with(&updated, "xl/drawings/drawing");
    assert_eq!(drawings.len(), 1, "{drawings:?}");
    assert!(entry(&updated, &drawings[0]).contains("<xdr:pic>"));
}
