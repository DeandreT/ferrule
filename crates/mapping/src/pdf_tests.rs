use super::*;

fn capture(name: &str) -> PdfCommand {
    PdfCommand::Capture(PdfCapture {
        name: name.into(),
        region: PdfRegion::full(),
    })
}

#[test]
fn validated_layout_derives_repeating_group_schema_and_roundtrips() {
    let layout = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::GroupPerPage(PdfGroup {
            name: "Row".into(),
            region: PdfRegion::full(),
            children: vec![capture("Value")],
        })],
    )
    .unwrap();
    let schema = layout.schema();
    assert_eq!(schema.name, "Document");
    let encoded = serde_json::to_string(&layout).unwrap();
    assert_eq!(serde_json::from_str::<PdfLayout>(&encoded).unwrap(), layout);
}

#[test]
fn layout_rejects_forward_and_wrong_axis_anchor_references() {
    let unknown = PdfCoordinate::edge(PdfReference::Anchor("Later".into()));
    let layout = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion {
                left: unknown,
                ..PdfRegion::full()
            },
        })],
    );
    assert!(matches!(layout, Err(PdfLayoutError::UnknownAnchor(name)) if name == "Later"));
}

#[test]
fn layout_rejects_nonfinite_coordinates_and_reversed_pages() {
    let nonfinite = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion {
                left: PdfCoordinate::new(PdfReference::Left, f64::NAN),
                ..PdfRegion::full()
            },
        })],
    );
    assert!(matches!(nonfinite, Err(PdfLayoutError::InvalidCoordinate)));

    let invalid_extent = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::EdgeRows(PdfEdgeRows {
            region: PdfRegion::full(),
            find: PdfEdgeFind {
                fill: 1.0,
                prominence: 0.0,
            },
            minimum_extent: Some(0.0),
            fallback_anchor: None,
            children: vec![capture("Value")],
        })],
    );
    assert!(matches!(
        invalid_extent,
        Err(PdfLayoutError::InvalidMinimumExtent)
    ));

    let reversed = PdfPageSelection::Range {
        first: NonZeroU32::new(2).unwrap(),
        last: NonZeroU32::new(1).unwrap(),
    };
    assert!(matches!(
        PdfLayout::new("Document", reversed, vec![capture("Value")]),
        Err(PdfLayoutError::InvalidPageRange { .. })
    ));
}

#[test]
fn document_page_and_merge_commands_validate_and_roundtrip() {
    let Some(page_two) = NonZeroU32::new(2) else {
        panic!("two must be nonzero");
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![capture("Heading")],
            }),
            PdfCommand::Merge(PdfMerge {
                name: "Rows".into(),
                composition: PdfMergeComposition::Independent,
                sources: vec![
                    PdfMergeSource {
                        page_selection: PdfPageSelection::First,
                        region: PdfRegion::full(),
                    },
                    PdfMergeSource {
                        page_selection: PdfPageSelection::Range {
                            first: page_two,
                            last: page_two,
                        },
                        region: PdfRegion::full(),
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
                        children: vec![capture("Value")],
                    })],
                })],
            }),
        ],
    ) else {
        panic!("document-level page and merge commands must validate");
    };

    let schema = layout.schema();
    assert!(schema.child("Heading").is_some());
    assert!(schema.child("Row").is_some_and(|row| row.repeating));
    let Ok(encoded) = serde_json::to_string(&layout) else {
        panic!("validated PDF layout must serialize");
    };
    let Ok(decoded) = serde_json::from_str::<PdfLayout>(&encoded) else {
        panic!("serialized PDF layout must deserialize");
    };
    assert_eq!(decoded, layout);
}

#[test]
fn text_groups_rows_and_open_page_ranges_validate_and_roundtrip() {
    let Some(page_two) = NonZeroU32::new(2) else {
        panic!("two must be nonzero");
    };
    let selection = PdfPageSelection::From { first: page_two };
    assert!(!selection.includes(1));
    assert!(selection.includes(2));
    assert!(selection.includes(u32::MAX));

    let matcher = |needle: &str| PdfTextMatch {
        needle: needle.into(),
        case: PdfTextCase::AsciiInsensitive,
        flexible_whitespace: true,
    };
    let Ok(layout) = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::TextGroups(PdfTextGroups {
            region: PdfRegion::full(),
            groups: vec![PdfTextGroup {
                output: PdfTextGroupOutput::Repeated {
                    name: "Record".into(),
                },
                matcher: matcher("Item code:"),
                children: vec![
                    capture("Code"),
                    PdfCommand::TextGroups(PdfTextGroups {
                        region: PdfRegion::full(),
                        groups: vec![
                            PdfTextGroup {
                                output: PdfTextGroupOutput::Flatten,
                                matcher: matcher("Details"),
                                children: vec![capture("Description")],
                            },
                            PdfTextGroup {
                                output: PdfTextGroupOutput::Repeated {
                                    name: "Location".into(),
                                },
                                matcher: matcher("Location:"),
                                children: vec![PdfCommand::TextRows(PdfTextRows {
                                    region: PdfRegion::full(),
                                    minimum_extent: Some(2.0),
                                    children: vec![PdfCommand::GroupPerPage(PdfGroup {
                                        name: "Count".into(),
                                        region: PdfRegion::full(),
                                        children: vec![capture("Quantity")],
                                    })],
                                })],
                            },
                        ],
                    }),
                ],
            }],
        })],
    ) else {
        panic!("text grouping layout must validate");
    };
    let schema = layout.schema();
    let Some(record) = schema.child("Record") else {
        panic!("text grouping schema must expose Record");
    };
    assert!(record.repeating);
    assert!(record.child("Code").is_some());
    assert!(record.child("Description").is_some());
    assert!(
        record
            .child("Location")
            .is_some_and(|location| location.repeating)
    );
    let Ok(encoded) = serde_json::to_string(&layout) else {
        panic!("text grouping layout must serialize");
    };
    let Ok(decoded) = serde_json::from_str::<PdfLayout>(&encoded) else {
        panic!("text grouping layout must deserialize");
    };
    assert_eq!(decoded, layout);
}

#[test]
fn text_group_validation_rejects_empty_matchers_and_nonrepeating_rows() {
    let empty_matcher = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::TextGroups(PdfTextGroups {
            region: PdfRegion::full(),
            groups: vec![PdfTextGroup {
                output: PdfTextGroupOutput::Repeated { name: "Row".into() },
                matcher: PdfTextMatch {
                    needle: String::new(),
                    case: PdfTextCase::Sensitive,
                    flexible_whitespace: false,
                },
                children: vec![capture("Value")],
            }],
        })],
    );
    assert!(matches!(
        empty_matcher,
        Err(PdfLayoutError::EmptyTextNeedle)
    ));

    let whitespace_matcher = PdfLayout {
        root_name: "Document".into(),
        page_selection: PdfPageSelection::First,
        commands: vec![PdfCommand::TextGroups(PdfTextGroups {
            region: PdfRegion::full(),
            groups: vec![PdfTextGroup {
                output: PdfTextGroupOutput::Repeated { name: "Row".into() },
                matcher: PdfTextMatch {
                    needle: " \t\n".into(),
                    case: PdfTextCase::Sensitive,
                    flexible_whitespace: true,
                },
                children: vec![capture("Value")],
            }],
        })],
    };
    let Ok(encoded) = serde_json::to_string(&whitespace_matcher) else {
        panic!("unchecked whitespace matcher must serialize");
    };
    assert!(matches!(
        serde_json::from_str::<PdfLayout>(&encoded),
            Err(error) if error.to_string().contains("matcher must not normalize to empty")
    ));

    let literal_whitespace = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::TextGroups(PdfTextGroups {
            region: PdfRegion::full(),
            groups: vec![PdfTextGroup {
                output: PdfTextGroupOutput::Repeated { name: "Row".into() },
                matcher: PdfTextMatch {
                    needle: " \t".into(),
                    case: PdfTextCase::Sensitive,
                    flexible_whitespace: false,
                },
                children: vec![capture("Value")],
            }],
        })],
    );
    assert!(literal_whitespace.is_ok());

    let scalar_rows = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::TextRows(PdfTextRows {
            region: PdfRegion::full(),
            minimum_extent: None,
            children: vec![capture("Value")],
        })],
    );
    assert!(matches!(
        scalar_rows,
        Err(PdfLayoutError::NonRepeatingRowOutput(name)) if name == "Value"
    ));
}

#[test]
fn layout_rejects_nested_document_commands_and_nonrepeating_row_outputs() {
    let nested = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::GroupPerPage(PdfGroup {
            name: "Outer".into(),
            region: PdfRegion::full(),
            children: vec![PdfCommand::Pages(PdfPages {
                selection: PdfPageSelection::First,
                children: vec![capture("Value")],
            })],
        })],
    );
    assert!(matches!(
        nested,
        Err(PdfLayoutError::NestedDocumentCommand("pages"))
    ));

    let scalar_rows = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::EdgeRows(PdfEdgeRows {
            region: PdfRegion::full(),
            find: PdfEdgeFind {
                fill: 1.0,
                prominence: 0.0,
            },
            minimum_extent: None,
            fallback_anchor: None,
            children: vec![capture("Value")],
        })],
    );
    assert!(matches!(
        scalar_rows,
        Err(PdfLayoutError::NonRepeatingRowOutput(name)) if name == "Value"
    ));

    let multipage_scalar = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::Pages(PdfPages {
            selection: PdfPageSelection::All,
            children: vec![capture("Value")],
        })],
    );
    assert!(matches!(
        multipage_scalar,
        Err(PdfLayoutError::NonRepeatingDocumentOutput {
            command: "page selection",
            name,
        }) if name == "Value"
    ));

    let single_merged_scalar = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::Merge(PdfMerge {
            name: "Merged".into(),
            composition: PdfMergeComposition::Independent,
            sources: vec![PdfMergeSource {
                page_selection: PdfPageSelection::First,
                region: PdfRegion::full(),
            }],
            children: vec![capture("Value")],
        })],
    );
    assert!(single_merged_scalar.is_ok());

    let merged_scalar = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::Merge(PdfMerge {
            name: "Merged".into(),
            composition: PdfMergeComposition::Independent,
            sources: vec![PdfMergeSource {
                page_selection: PdfPageSelection::All,
                region: PdfRegion::full(),
            }],
            children: vec![capture("Value")],
        })],
    );
    assert!(matches!(
        merged_scalar,
        Err(PdfLayoutError::NonRepeatingDocumentOutput {
            command: "merge",
            name,
        }) if name == "Value"
    ));

    let vertical_collage_scalar = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![PdfCommand::Merge(PdfMerge {
            name: "Merged".into(),
            composition: PdfMergeComposition::VerticalCollage,
            sources: vec![
                PdfMergeSource {
                    page_selection: PdfPageSelection::All,
                    region: PdfRegion::full(),
                },
                PdfMergeSource {
                    page_selection: PdfPageSelection::First,
                    region: PdfRegion::full(),
                },
            ],
            children: vec![capture("Value")],
        })],
    );
    assert!(vertical_collage_scalar.is_ok());
}

#[test]
fn merge_sources_and_children_cannot_use_outer_anchors() {
    let anchor = PdfCommand::Anchor(PdfAnchorAssignment {
        name: "Outer".into(),
        axis: PdfAnchorAxis::Horizontal,
        at: PdfCoordinate::edge(PdfReference::Left),
    });
    let anchored_region = PdfRegion {
        left: PdfCoordinate::edge(PdfReference::Anchor("Outer".into())),
        ..PdfRegion::full()
    };
    let source_layout = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            anchor,
            PdfCommand::Merge(PdfMerge {
                name: "Rows".into(),
                composition: PdfMergeComposition::Independent,
                sources: vec![PdfMergeSource {
                    page_selection: PdfPageSelection::All,
                    region: anchored_region,
                }],
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Row".into(),
                    region: PdfRegion::full(),
                    children: vec![capture("Value")],
                })],
            }),
        ],
    );
    assert!(matches!(
        source_layout,
        Err(PdfLayoutError::UnknownAnchor(name)) if name == "Outer"
    ));

    let child_layout = PdfLayout::new(
        "Document",
        PdfPageSelection::All,
        vec![
            PdfCommand::Anchor(PdfAnchorAssignment {
                name: "Outer".into(),
                axis: PdfAnchorAxis::Horizontal,
                at: PdfCoordinate::edge(PdfReference::Left),
            }),
            PdfCommand::Merge(PdfMerge {
                name: "Rows".into(),
                composition: PdfMergeComposition::Independent,
                sources: vec![PdfMergeSource {
                    page_selection: PdfPageSelection::All,
                    region: PdfRegion::full(),
                }],
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Row".into(),
                    region: PdfRegion {
                        left: PdfCoordinate::edge(PdfReference::Anchor("Outer".into())),
                        ..PdfRegion::full()
                    },
                    children: vec![capture("Value")],
                })],
            }),
        ],
    );
    assert!(matches!(
        child_layout,
        Err(PdfLayoutError::UnknownAnchor(name)) if name == "Outer"
    ));
}
