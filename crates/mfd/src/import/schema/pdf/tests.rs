use std::collections::BTreeMap;

use super::{
    collect_merge_sources, last_capture_region, parse_coordinate, parse_groups_from_list,
    parse_native_template_text, parse_page_filter, parse_region,
};
use mapping::{PdfCapture, PdfCommand, PdfCoordinate, PdfGroup, PdfReference, PdfRegion};

const FULL_REGION: &str = "{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }";
const ZERO_REGION: &str =
    "{ Left: Left - 0pt, Top: Top + 0pt, Right: Right - 0pt, Bottom: Bottom - 0pt }";
const NARROW_REGION: &str = "{ Left: Left + 5pt, Top: Top, Right: Right, Bottom: Bottom }";

fn template(commands: &str) -> String {
    format!(
        "<Document><Template><Model><Root><Label>Document</Label><Children>{commands}\
         </Children></Root></Model></Template></Document>"
    )
}

fn capture() -> String {
    format!("<Capture><Label>Value</Label><Region>{FULL_REGION}</Region></Capture>")
}

fn grouping(label: &str, region: &str, filter: &str, children: &str) -> String {
    format!(
        "<Grouping><Label>{label}</Label>{region}<Kind><OneGroupPerPage/></Kind>\
         <Filter>{filter}</Filter><Children>{children}</Children></Grouping>"
    )
}

#[test]
fn transparent_group_defaults_keep_direct_children_and_exact_page_selection() {
    for filter in ["", "1", "2"] {
        let expected = parse_native_template_text(
            &template(&grouping("", "", filter, &capture())),
            "Document",
        )
        .unwrap();
        for region in [
            "".to_string(),
            "<Region/>".to_string(),
            format!("<Region>{FULL_REGION}</Region>"),
            format!("<Region>{ZERO_REGION}</Region>"),
        ] {
            let layout = parse_native_template_text(
                &template(&grouping("", &region, filter, &capture())),
                "Document",
            )
            .unwrap();
            assert_eq!(layout, expected);
            match layout.commands() {
                [PdfCommand::Capture(_)] => assert_eq!(filter, ""),
                [PdfCommand::Pages(pages)] => {
                    assert!(matches!(
                        pages.children.as_slice(),
                        [PdfCommand::Capture(_)]
                    ));
                    assert_eq!(
                        pages.selection,
                        if filter == "1" {
                            mapping::PdfPageSelection::First
                        } else {
                            let page = std::num::NonZeroU32::new(2).unwrap();
                            mapping::PdfPageSelection::Range {
                                first: page,
                                last: page,
                            }
                        }
                    );
                }
                commands => panic!("unexpected transparent commands: {commands:?}"),
            }
        }
    }
    assert_eq!(
        parse_region(ZERO_REGION).unwrap().left.offset.to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn transparent_groups_reject_narrowed_or_malformed_regions_without_echoing_payloads() {
    let huge_invalid = "x".repeat(65_536);
    for region in [
        format!("<Region>{NARROW_REGION}</Region>"),
        "<Region>{ Left: Top, Top: Top, Right: Right, Bottom: Bottom }</Region>".to_string(),
        format!("<Region>{huge_invalid}</Region>"),
        "<Region><Unknown/></Region>".to_string(),
        format!("<Region/><Region>{NARROW_REGION}</Region>"),
        format!("<Region>{FULL_REGION}<!-- separator -->invalid</Region>"),
    ] {
        for filter in ["", "1", "2"] {
            let error = parse_native_template_text(
                &template(&grouping("", &region, filter, &capture())),
                "Document",
            )
            .unwrap_err();
            assert!(error.contains("unnamed Grouping Region"), "{error}");
            assert!(error.contains("cannot be retained"));
            assert!(error.len() < 200);
        }
        let nested = grouping("Outer", "", "", &grouping("", &region, "", &capture()));
        assert!(parse_native_template_text(&template(&nested), "Document").is_err());
    }
}

#[test]
fn named_groups_retain_narrowed_regions_and_signed_zero() {
    for filter in ["", "1", "2"] {
        let region = "{ Left: Left + 5pt, Top: Top, Right: Right - 0pt, Bottom: Bottom - 0pt }";
        let layout = parse_native_template_text(
            &template(&grouping(
                "Rows",
                &format!("<Region>{region}</Region>"),
                filter,
                &capture(),
            )),
            "Document",
        )
        .unwrap();
        let commands = match layout.commands() {
            [PdfCommand::Pages(pages)] => pages.children.as_slice(),
            commands => commands,
        };
        let [PdfCommand::GroupPerPage(group)] = commands else {
            panic!("named group must retain its command");
        };
        assert_eq!(group.region.left.offset, 5.0);
        assert_eq!(group.region.right.offset.to_bits(), (-0.0_f64).to_bits());
        assert_eq!(group.region.bottom.offset.to_bits(), (-0.0_f64).to_bits());
    }
}

#[test]
fn transparent_merge_wrappers_accept_only_default_regions() {
    let merge_source =
        format!("<MergeSource><Target>Rows</Target><Region>{FULL_REGION}</Region></MergeSource>");
    let merge_target = format!(
        "<MergeTarget><Name>Rows</Name><Children>{}</Children></MergeTarget>",
        grouping("Rows", "", "", &capture())
    );
    for (kind, filter, merge_inside) in [
        ("<OneGroupPerPage/>", "", false),
        ("<OneGroupPerPage/>", "1", false),
        ("<OneGroupPerPage/>", "2", false),
        (
            "<GroupsFromList><Pages>2-</Pages></GroupsFromList>",
            "",
            true,
        ),
    ] {
        let wrapper = |region: &str| {
            let (children, trailing) = if merge_inside {
                (format!("{merge_source}{merge_target}"), String::new())
            } else {
                (merge_source.clone(), merge_target.clone())
            };
            format!(
                "<Grouping><Label/>{region}<Kind>{kind}</Kind><Filter>{filter}</Filter>\
             <Children>{children}</Children></Grouping>{trailing}"
            )
        };
        let expected = parse_native_template_text(&template(&wrapper("")), "Document").unwrap();
        for region in [FULL_REGION, ZERO_REGION] {
            let actual = parse_native_template_text(
                &template(&wrapper(&format!("<Region>{region}</Region>"))),
                "Document",
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
        assert!(matches!(expected.commands(), [PdfCommand::Merge(_)]));
        let error = parse_native_template_text(
            &template(&wrapper(&format!("<Region>{NARROW_REGION}</Region>"))),
            "Document",
        )
        .unwrap_err();
        assert!(error.contains("unnamed Grouping Region"));
    }
}

#[test]
fn native_template_self_check_enforces_byte_element_and_depth_limits() {
    let oversized = format!("<Document>{}</Document>", "x".repeat(1024 * 1024));
    assert!(
        parse_native_template_text(&oversized, "Document")
            .unwrap_err()
            .contains("1048576-byte limit")
    );

    let many_elements = format!("<Document>{}</Document>", "<Extra/>".repeat(4096));
    assert!(
        parse_native_template_text(&many_elements, "Document")
            .unwrap_err()
            .contains("4096-element limit")
    );

    let deeply_nested = format!(
        "<Document>{}{}</Document>",
        "<Extra>".repeat(64),
        "</Extra>".repeat(64)
    );
    assert!(
        parse_native_template_text(&deeply_nested, "Document")
            .unwrap_err()
            .contains("64-level depth limit")
    );
}

fn edge_splitter_template(controls: &str) -> String {
    format!(
        "<Document><Template version=\"1\"><Model><Root><Label>Document</Label><Children>\
         <Splitter>{controls}<Region/><FeatureFind><EdgeFind><Fill>2pt</Fill>\
         <Prominence>15</Prominence><Resolution><Standard/></Resolution></EdgeFind></FeatureFind>\
         <Children><Grouping><Label>Row</Label><Kind><OneGroupPerPage/></Kind><Filter/>\
         <Children><Capture><Label>Value</Label>\
         <Region>{{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }}</Region>\
         </Capture></Children></Grouping></Children></Splitter>\
         </Children></Root></Model></Template></Document>"
    )
}

#[test]
fn native_splitter_accepts_only_represented_default_controls() {
    let expected = parse_native_template_text(&edge_splitter_template(""), "Document").unwrap();
    assert!(matches!(expected.commands(), [PdfCommand::EdgeRows(rows)]
        if rows.find.fill == 2.0 && rows.find.prominence == 15.0));
    for controls in [
        "<Search/><SkipInitial/><SkipFinal/>",
        "<Search/><SkipInitial>0</SkipInitial><SkipFinal>0</SkipFinal>",
        "<Search> \n </Search><SkipInitial> 0 </SkipInitial><SkipFinal> 0 </SkipFinal>",
    ] {
        assert_eq!(
            parse_native_template_text(&edge_splitter_template(controls), "Document").unwrap(),
            expected
        );
    }
}

#[test]
fn native_splitter_rejects_nondefault_or_ambiguous_controls() {
    for (controls, diagnostic) in [
        ("<Search>needle</Search>", "Search must be empty"),
        (
            "<Search><Unknown/></Search>",
            "Search must contain only scalar text",
        ),
        ("<Search/><Search>needle</Search>", "duplicate Search"),
        ("<SkipInitial>1</SkipInitial>", "SkipInitial supports only"),
        ("<SkipFinal>2</SkipFinal>", "SkipFinal supports only"),
        (
            "<SkipInitial>invalid</SkipInitial>",
            "SkipInitial supports only",
        ),
        ("<SkipFinal>-1</SkipFinal>", "SkipFinal supports only"),
        ("<SkipFinal>0.0</SkipFinal>", "SkipFinal supports only"),
        (
            "<SkipInitial>999999999999999999999</SkipInitial>",
            "SkipInitial supports only",
        ),
        (
            "<SkipInitial><Unknown/></SkipInitial>",
            "SkipInitial must contain only scalar text",
        ),
        (
            "<SkipFinal>0</SkipFinal><SkipFinal>1</SkipFinal>",
            "duplicate SkipFinal",
        ),
        (
            "<SkipInitial>0<!-- separator -->1</SkipInitial>",
            "SkipInitial supports only",
        ),
    ] {
        let error =
            parse_native_template_text(&edge_splitter_template(controls), "Document").unwrap_err();
        assert!(error.contains(diagnostic), "{controls}: {error}");
    }
}

#[test]
fn parses_absolute_and_anchor_coordinates() {
    let coordinate = parse_coordinate("(Left + 18.5pt)").unwrap();
    assert_eq!(coordinate.reference, PdfReference::Left);
    assert_eq!(coordinate.offset, 18.5);

    let coordinate = parse_coordinate("[Column] - 2pt").unwrap();
    assert_eq!(coordinate.reference, PdfReference::Anchor("Column".into()));
    assert_eq!(coordinate.offset, -2.0);
}

#[test]
fn parses_complete_regions_in_any_edge_order() {
    let region =
        parse_region("{ Top: Top + 2pt, Left: Left, Bottom: [End], Right: Right - 3pt }").unwrap();
    assert_eq!(region.left.reference, PdfReference::Left);
    assert_eq!(region.top.offset, 2.0);
    assert_eq!(region.right.offset, -3.0);
    assert_eq!(region.bottom.reference, PdfReference::Anchor("End".into()));
}

#[test]
fn page_filters_require_one_exact_positive_page() {
    let Ok(valid) = roxmltree::Document::parse(
        "<Grouping><Filter>2</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
    ) else {
        panic!("valid page filter XML must parse");
    };
    assert!(matches!(
        parse_page_filter(&valid.root_element()),
        Ok(Some(mapping::PdfPageSelection::Range { first, last }))
            if first.get() == 2 && last.get() == 2
    ));

    let Ok(open) = roxmltree::Document::parse(
        "<Grouping><Filter>2-</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
    ) else {
        panic!("open page filter XML must parse");
    };
    assert!(matches!(
        parse_page_filter(&open.root_element()),
        Err(message) if message.contains("not an exact page number")
    ));

    let Ok(zero) = roxmltree::Document::parse(
        "<Grouping><Filter>0</Filter><Kind><OneGroupPerPage/></Kind></Grouping>",
    ) else {
        panic!("zero page filter XML must parse");
    };
    assert!(parse_page_filter(&zero.root_element()).is_err());
}

#[test]
fn groups_from_list_accepts_only_open_positive_ranges() {
    let Ok(valid) = roxmltree::Document::parse(
        "<Grouping><Kind><GroupsFromList><Pages>2-</Pages></GroupsFromList></Kind><Filter/></Grouping>",
    ) else {
        panic!("valid page-list XML must parse");
    };
    assert!(matches!(
        parse_groups_from_list(&valid.root_element()),
        Ok(Some(mapping::PdfPageSelection::From { first })) if first.get() == 2
    ));

    let Ok(disjoint) = roxmltree::Document::parse(
        "<Grouping><Kind><GroupsFromList><Pages>2,4</Pages></GroupsFromList></Kind><Filter/></Grouping>",
    ) else {
        panic!("unsupported page-list XML must still parse");
    };
    assert!(matches!(
        parse_groups_from_list(&disjoint.root_element()),
        Err(message) if message.contains("expected N-")
    ));
}

#[test]
fn merge_sources_must_remain_in_page_relative_document_groups() {
    let Ok(document) = roxmltree::Document::parse(
        "<Children><Splitter><Children><MergeSource><Region>{ Left: Left, Top: Top, Right: Right, Bottom: Bottom }</Region><Target>Rows</Target></MergeSource></Children></Splitter></Children>",
    ) else {
        panic!("nested merge-source XML must parse");
    };
    let mut sources = BTreeMap::new();
    assert!(matches!(
        collect_merge_sources(&document.root_element(), &mut sources),
        Err(message) if message.contains("page-relative")
    ));
    assert!(sources.is_empty());
}

#[test]
fn row_anchor_inference_does_not_cross_a_narrowed_group_region() {
    let command = PdfCommand::GroupPerPage(PdfGroup {
        name: "Row".into(),
        region: PdfRegion {
            left: PdfCoordinate::new(PdfReference::Left, 20.0),
            ..PdfRegion::full()
        },
        children: vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    });
    assert!(last_capture_region(&[command]).is_none());
}
