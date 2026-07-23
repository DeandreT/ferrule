use std::num::NonZeroU32;

use ir::{Instance, Value};
use mapping::{
    PdfCapture, PdfCaptureAlgorithm, PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup,
    PdfLayout, PdfMerge, PdfMergeComposition, PdfMergeSource, PdfPageSelection, PdfPages,
    PdfReference, PdfRegion, PdfTextCase, PdfTextGroup, PdfTextGroupOutput, PdfTextGroups,
    PdfTextMatch, PdfTextRows, PdfWhitespaceMode, PdfWordSeparation,
};

use super::{OutputBudget, capture_text, evaluate, instance_has_content};
use crate::PdfError;
use crate::extract::{Glyph, HorizontalEdge, Page, Rect};

fn glyph(text: &str, left: f64, top: f64, right: f64, bottom: f64) -> Glyph {
    Glyph {
        text: text.into(),
        bounds: Rect {
            left,
            top,
            right,
            bottom,
        },
        font_face: None,
        cell_height: bottom - top,
        baseline_angle: 0.0,
    }
}

fn fixed_region(left: f64, top: f64, right: f64, bottom: f64) -> PdfRegion {
    PdfRegion {
        left: PdfCoordinate::new(PdfReference::Left, left),
        top: PdfCoordinate::new(PdfReference::Top, top),
        right: PdfCoordinate::new(PdfReference::Left, right),
        bottom: PdfCoordinate::new(PdfReference::Top, bottom),
    }
}

#[test]
fn basic_visual_capture_policy_inserts_word_and_line_separators() {
    let page = Page {
        number: 1,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 100.0,
        },
        glyphs: vec![
            glyph("A", 0.0, 0.0, 5.0, 10.0),
            glyph("B", 10.0, 0.0, 15.0, 10.0),
            glyph("C", 0.0, 20.0, 5.0, 30.0),
        ],
        horizontal_edges: Vec::new(),
    };
    let algorithm = PdfCaptureAlgorithm::BasicVisual {
        separate_words: PdfWordSeparation::InsertSpace,
        whitespace: PdfWhitespaceMode::Default,
    };
    let Ok(value) = capture_text(
        &page,
        page.bounds,
        algorithm,
        "Value",
        &mut OutputBudget::default(),
    ) else {
        panic!("bounded BasicVisual capture should evaluate");
    };
    assert_eq!(value, Value::String("A B\nC".into()));
}

#[test]
fn edge_rows_capture_repeated_columns_and_discard_empty_bands() {
    let page = Page {
        number: 1,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 200.0,
        },
        glyphs: vec![
            glyph("Ada", 10.0, 25.0, 30.0, 35.0),
            glyph("7", 120.0, 25.0, 126.0, 35.0),
            glyph("footer", 10.0, 55.0, 42.0, 65.0),
            glyph("Lin", 10.0, 80.0, 28.0, 90.0),
            glyph("9", 120.0, 80.0, 126.0, 90.0),
        ],
        horizontal_edges: [50.0, 70.0, 110.0]
            .into_iter()
            .map(|y| HorizontalEdge {
                left: 0.0,
                right: 200.0,
                y,
            })
            .collect(),
    };
    let capture = |name: &str, left: f64, right: f64| {
        PdfCommand::Capture(PdfCapture {
            name: name.into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, left),
                top: PdfCoordinate::edge(PdfReference::Top),
                right: PdfCoordinate::new(PdfReference::Left, right),
                bottom: PdfCoordinate::edge(PdfReference::Bottom),
            },
            algorithm: Default::default(),
        })
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::EdgeRows(PdfEdgeRows {
            region: PdfRegion::full(),
            find: PdfEdgeFind {
                fill: 2.0,
                prominence: 100.0,
            },
            minimum_extent: Some(30.0),
            fallback_anchor: None,
            children: vec![PdfCommand::GroupPerPage(PdfGroup {
                name: "Row".into(),
                region: PdfRegion::full(),
                children: vec![capture("Name", 0.0, 100.0), capture("Count", 100.0, 200.0)],
            })],
        })],
    ) else {
        panic!("synthetic ruled-row layout must be valid");
    };
    let Ok(instance) = evaluate(&[page], &layout) else {
        panic!("synthetic ruled-row page must evaluate");
    };
    let Some(rows) = instance.field("Row").and_then(Instance::as_repeated) else {
        panic!("synthetic ruled-row output must contain rows");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::String("9".into()))
    );
}

#[test]
fn unruled_rows_fold_wrapped_lines_around_a_trailing_column() {
    let page = Page {
        number: 1,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 100.0,
        },
        glyphs: vec![
            glyph("Long ", 10.0, 5.0, 34.0, 15.0),
            glyph("A", 180.0, 12.0, 190.0, 22.0),
            glyph("title", 10.0, 18.0, 36.0, 28.0),
            glyph("Second", 10.0, 45.0, 48.0, 55.0),
            glyph("B", 180.0, 45.0, 190.0, 55.0),
        ],
        horizontal_edges: Vec::new(),
    };
    let capture = |name: &str, left: f64, right: f64| {
        PdfCommand::Capture(PdfCapture {
            name: name.into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, left),
                top: PdfCoordinate::edge(PdfReference::Top),
                right: PdfCoordinate::new(PdfReference::Left, right),
                bottom: PdfCoordinate::edge(PdfReference::Bottom),
            },
            algorithm: Default::default(),
        })
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::EdgeRows(PdfEdgeRows {
            region: PdfRegion::full(),
            find: PdfEdgeFind {
                fill: 1.0,
                prominence: 100.0,
            },
            minimum_extent: None,
            fallback_anchor: Some(fixed_region(100.0, 0.0, 200.0, 100.0)),
            children: vec![PdfCommand::GroupPerPage(PdfGroup {
                name: "Row".into(),
                region: PdfRegion::full(),
                children: vec![capture("Title", 0.0, 100.0), capture("Key", 100.0, 200.0)],
            })],
        })],
    ) else {
        panic!("synthetic unruled-row layout must be valid");
    };

    let Ok(instance) = evaluate(&[page], &layout) else {
        panic!("synthetic unruled-row page must evaluate");
    };
    let Some(rows) = instance.field("Row").and_then(Instance::as_repeated) else {
        panic!("synthetic unruled-row output must contain rows");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Title").and_then(Instance::as_scalar),
        Some(&Value::String("Long\ntitle".into()))
    );
    assert_eq!(
        rows[1].field("Key").and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
}

#[test]
fn page_blocks_and_merge_sources_preserve_page_and_source_order() {
    let page = |number, company, row| Page {
        number,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 120.0,
        },
        glyphs: vec![
            glyph(company, 10.0, 10.0, 60.0, 20.0),
            glyph(row, 10.0, 65.0, 30.0, 75.0),
        ],
        horizontal_edges: Vec::new(),
    };
    let pages = [page(1, "Acme", "A"), page(2, "ignored", "B")];
    let Some(page_two) = NonZeroU32::new(2) else {
        panic!("two must be nonzero");
    };
    let second_page = PdfPageSelection::Range {
        first: page_two,
        last: page_two,
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![PdfCommand::Capture(PdfCapture {
                    name: "Company".into(),
                    region: fixed_region(0.0, 0.0, 100.0, 40.0),
                    algorithm: Default::default(),
                })],
            }),
            PdfCommand::Merge(PdfMerge {
                name: "Table".into(),
                composition: PdfMergeComposition::Independent,
                sources: vec![
                    PdfMergeSource {
                        page_selection: PdfPageSelection::First,
                        region: fixed_region(0.0, 50.0, 200.0, 100.0),
                    },
                    PdfMergeSource {
                        page_selection: second_page,
                        region: fixed_region(0.0, 50.0, 200.0, 100.0),
                    },
                ],
                children: vec![PdfCommand::EdgeRows(PdfEdgeRows {
                    region: PdfRegion::full(),
                    find: PdfEdgeFind {
                        fill: 1.0,
                        prominence: 0.0,
                    },
                    minimum_extent: None,
                    fallback_anchor: None,
                    children: vec![PdfCommand::GroupPerPage(PdfGroup {
                        name: "Row".into(),
                        region: PdfRegion::full(),
                        children: vec![PdfCommand::Capture(PdfCapture {
                            name: "Value".into(),
                            region: PdfRegion::full(),
                            algorithm: Default::default(),
                        })],
                    })],
                })],
            }),
        ],
    ) else {
        panic!("synthetic page merge layout must be valid");
    };

    let Ok(instance) = evaluate(&pages, &layout) else {
        panic!("synthetic page merge must evaluate");
    };
    assert_eq!(
        instance.field("Company").and_then(Instance::as_scalar),
        Some(&Value::String("Acme".into()))
    );
    let Some(rows) = instance.field("Row").and_then(Instance::as_repeated) else {
        panic!("synthetic page merge output must contain rows");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(
        rows[1].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
}

#[test]
fn root_anchors_survive_intervening_page_blocks_per_physical_page() {
    let page = |number, value| Page {
        number,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 100.0,
        },
        glyphs: vec![
            glyph("heading", 10.0, 10.0, 40.0, 20.0),
            glyph(value, 60.0, 60.0, 80.0, 70.0),
        ],
        horizontal_edges: Vec::new(),
    };
    let pages = [page(1, "A"), page(2, "B")];
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::Anchor(mapping::PdfAnchorAssignment {
                name: "Column".into(),
                axis: mapping::PdfAnchorAxis::Horizontal,
                at: PdfCoordinate::new(PdfReference::Left, 50.0),
            }),
            PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![PdfCommand::Capture(PdfCapture {
                    name: "Heading".into(),
                    region: fixed_region(0.0, 0.0, 50.0, 30.0),
                    algorithm: Default::default(),
                })],
            }),
            PdfCommand::GroupPerPage(PdfGroup {
                name: "Row".into(),
                region: PdfRegion {
                    left: PdfCoordinate::edge(PdfReference::Anchor("Column".into())),
                    top: PdfCoordinate::new(PdfReference::Top, 50.0),
                    right: PdfCoordinate::edge(PdfReference::Right),
                    bottom: PdfCoordinate::edge(PdfReference::Bottom),
                },
                children: vec![PdfCommand::Capture(PdfCapture {
                    name: "Value".into(),
                    region: PdfRegion::full(),
                    algorithm: Default::default(),
                })],
            }),
        ],
    ) else {
        panic!("root anchor page-block layout must validate");
    };

    let Ok(instance) = evaluate(&pages, &layout) else {
        panic!("root anchor page-block layout must evaluate");
    };
    let Some(rows) = instance.field("Row").and_then(Instance::as_repeated) else {
        panic!("root anchor page-block output must contain rows");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(
        rows[1].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
}

#[test]
fn vertical_collage_text_groups_keep_nested_records_open_across_pages() {
    let page = |number, glyphs| Page {
        number,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 120.0,
            bottom: 100.0,
        },
        glyphs,
        horizontal_edges: Vec::new(),
    };
    let pages = [
        page(
            1,
            vec![
                glyph("header", 10.0, 1.0, 30.0, 6.0),
                glyph("ITEM   CODE:", 10.0, 15.0, 42.0, 20.0),
                glyph("100", 60.0, 15.0, 70.0, 20.0),
                glyph("Warehouse:", 10.0, 45.0, 45.0, 50.0),
                glyph("North", 60.0, 45.0, 75.0, 50.0),
                glyph("XS", 10.0, 65.0, 20.0, 70.0),
                glyph("2", 80.0, 65.0, 85.0, 70.0),
                glyph("unmapped note", 55.0, 75.0, 65.0, 80.0),
            ],
        ),
        page(
            2,
            vec![
                glyph("S", 10.0, 15.0, 20.0, 20.0),
                glyph("3", 80.0, 15.0, 85.0, 20.0),
                glyph("Item Code:", 10.0, 35.0, 42.0, 40.0),
                glyph("200", 60.0, 35.0, 70.0, 40.0),
                glyph("Warehouse:", 10.0, 55.0, 45.0, 60.0),
                glyph("South", 60.0, 55.0, 75.0, 60.0),
                glyph("M", 10.0, 75.0, 20.0, 80.0),
                glyph("4", 80.0, 75.0, 85.0, 80.0),
            ],
        ),
    ];
    let capture = |name: &str, left: f64, top: f64, right: f64, bottom: f64| {
        PdfCommand::Capture(PdfCapture {
            name: name.into(),
            region: fixed_region(left, top, right, bottom),
            algorithm: Default::default(),
        })
    };
    let row_capture = |name: &str, left: f64, right: f64| {
        PdfCommand::Capture(PdfCapture {
            name: name.into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, left),
                top: PdfCoordinate::edge(PdfReference::Top),
                right: PdfCoordinate::new(PdfReference::Left, right),
                bottom: PdfCoordinate::edge(PdfReference::Bottom),
            },
            algorithm: Default::default(),
        })
    };
    let stock = PdfCommand::GroupPerPage(PdfGroup {
        name: "Stock".into(),
        region: PdfRegion::full(),
        children: vec![
            row_capture("Size", 0.0, 40.0),
            row_capture("InStock", 60.0, 100.0),
        ],
    });
    let warehouses = PdfCommand::TextGroups(PdfTextGroups {
        region: PdfRegion::full(),
        groups: vec![PdfTextGroup {
            output: PdfTextGroupOutput::Repeated {
                name: "Warehouse".into(),
            },
            matcher: PdfTextMatch {
                needle: "Warehouse:".into(),
                case: PdfTextCase::Sensitive,
                flexible_whitespace: true,
                properties: Default::default(),
            },
            children: vec![
                capture("Name", 40.0, 0.0, 100.0, 12.0),
                PdfCommand::TextRows(PdfTextRows {
                    region: PdfRegion {
                        left: PdfCoordinate::edge(PdfReference::Left),
                        top: PdfCoordinate::new(PdfReference::Top, 20.0),
                        right: PdfCoordinate::edge(PdfReference::Right),
                        bottom: PdfCoordinate::edge(PdfReference::Bottom),
                    },
                    minimum_extent: None,
                    children: vec![stock],
                }),
            ],
        }],
    });
    let items = PdfCommand::TextGroups(PdfTextGroups {
        region: PdfRegion::full(),
        groups: vec![PdfTextGroup {
            output: PdfTextGroupOutput::Repeated {
                name: "Item".into(),
            },
            matcher: PdfTextMatch {
                needle: "item code:".into(),
                case: PdfTextCase::AsciiInsensitive,
                flexible_whitespace: true,
                properties: Default::default(),
            },
            children: vec![capture("Code", 40.0, 0.0, 100.0, 12.0), warehouses],
        }],
    });
    let Some(first) = NonZeroU32::new(1) else {
        panic!("one must be nonzero");
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::Merge(PdfMerge {
            name: "Inventory".into(),
            composition: PdfMergeComposition::VerticalCollage,
            sources: vec![PdfMergeSource {
                page_selection: PdfPageSelection::From { first },
                region: fixed_region(10.0, 10.0, 110.0, 90.0),
            }],
            children: vec![items],
        })],
    ) else {
        panic!("vertical collage marker layout must validate");
    };

    let Ok(instance) = evaluate(&pages, &layout) else {
        panic!("vertical collage marker layout must evaluate");
    };
    let Some(items) = instance.field("Item").and_then(Instance::as_repeated) else {
        panic!("vertical collage output must contain items");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("100".into()))
    );
    let Some(first_warehouse) = items[0]
        .field("Warehouse")
        .and_then(Instance::as_repeated)
        .and_then(|warehouses| warehouses.first())
    else {
        panic!("first item must contain a warehouse");
    };
    let Some(stock) = first_warehouse
        .field("Stock")
        .and_then(Instance::as_repeated)
    else {
        panic!("first warehouse must contain stock rows");
    };
    assert_eq!(stock.len(), 2);
    assert_eq!(
        stock[1].field("Size").and_then(Instance::as_scalar),
        Some(&Value::String("S".into()))
    );
    assert_eq!(
        items[1].field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("200".into()))
    );
}

#[test]
fn recursive_content_keeps_non_null_scalar_values() {
    let empty = Instance::Group(vec![(
        "Nested".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::Null),
        )])]),
    )]);
    assert!(!instance_has_content(&empty));

    for value in [
        Value::Bool(false),
        Value::Int(0),
        Value::String(String::new()),
        Value::XmlNil(ir::XmlNil),
    ] {
        assert!(instance_has_content(&Instance::Scalar(value)));
    }
}

#[test]
fn text_rows_reject_dense_sort_preprocessing() {
    let glyphs = (0..100_000)
        .map(|index| {
            let top = index as f64 * 2.0;
            glyph("x", 0.0, top, 1.0, top + 1.0)
        })
        .collect::<Vec<_>>();
    let page = Page {
        number: 1,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 10.0,
            bottom: 200_000.0,
        },
        glyphs,
        horizontal_edges: Vec::new(),
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::TextRows(PdfTextRows {
            region: PdfRegion::full(),
            minimum_extent: None,
            children: vec![PdfCommand::GroupPerPage(PdfGroup {
                name: "Row".into(),
                region: PdfRegion::full(),
                children: vec![PdfCommand::Capture(PdfCapture {
                    name: "Value".into(),
                    region: PdfRegion::full(),
                    algorithm: Default::default(),
                })],
            })],
        })],
    ) else {
        panic!("dense text-row layout must validate");
    };

    assert!(matches!(
        evaluate(&[page], &layout),
        Err(PdfError::TooManyEvents)
    ));
}

#[test]
fn nested_text_groups_charge_each_parent_page_rescan() {
    let glyphs = (0..1_024)
        .map(|index| {
            let top = index as f64 * 2.0;
            glyph("row", 0.0, top, 10.0, top + 1.0)
        })
        .collect::<Vec<_>>();
    let page = Page {
        number: 1,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 20.0,
            bottom: 2_048.0,
        },
        glyphs,
        horizontal_edges: Vec::new(),
    };
    let matcher = || PdfTextMatch {
        needle: "row".into(),
        case: PdfTextCase::Sensitive,
        flexible_whitespace: false,
        properties: Default::default(),
    };
    let inner = PdfCommand::TextGroups(PdfTextGroups {
        region: PdfRegion::full(),
        groups: vec![PdfTextGroup {
            output: PdfTextGroupOutput::Flatten,
            matcher: matcher(),
            children: vec![PdfCommand::Capture(PdfCapture {
                name: "Value".into(),
                region: PdfRegion::full(),
                algorithm: Default::default(),
            })],
        }],
    });
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::TextGroups(PdfTextGroups {
            region: PdfRegion::full(),
            groups: vec![PdfTextGroup {
                output: PdfTextGroupOutput::Repeated { name: "Row".into() },
                matcher: matcher(),
                children: vec![inner],
            }],
        })],
    ) else {
        panic!("nested dense text-group layout must validate");
    };

    assert!(matches!(
        evaluate(&[page], &layout),
        Err(PdfError::TooManyEvents)
    ));
}
