use std::collections::BTreeMap;

use super::{
    collect_merge_sources, last_capture_region, parse_coordinate, parse_groups_from_list,
    parse_native_template_text, parse_page_filter, parse_region,
};
use mapping::{PdfCapture, PdfCommand, PdfCoordinate, PdfGroup, PdfReference, PdfRegion};

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
