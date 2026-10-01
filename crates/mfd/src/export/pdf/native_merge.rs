use std::fmt::Write as _;

use mapping::{
    PdfCapture, PdfCaptureAlgorithm, PdfCommand, PdfEdgeFind, PdfLayout, PdfMergeComposition,
    PdfPageSelection, PdfRegion, PdfWhitespaceMode, PdfWordSeparation,
};

use super::{
    MAX_PXT_BYTES, native_label, native_region, same_capture, same_capture_commands, same_region,
    xml_escape,
};

/// One capture from the first page, followed by
/// two independently evaluated physical page regions in their declared order.
/// The merge contains one painted-edge row splitter and one named group of
/// direct default visual captures. The splitter's fallback anchor is inferred
/// by the native importer, so the final comparison must include it.
pub(super) fn template(layout: &PdfLayout) -> Option<String> {
    if layout.page_selection() != PdfPageSelection::All || !native_label(layout.root_name()) {
        return None;
    }
    let [PdfCommand::Pages(first_page), PdfCommand::Merge(merge)] = layout.commands() else {
        return None;
    };
    let [PdfCommand::Capture(company)] = first_page.children.as_slice() else {
        return None;
    };
    let [first_source, second_source] = merge.sources.as_slice() else {
        return None;
    };
    let [PdfCommand::EdgeRows(rows)] = merge.children.as_slice() else {
        return None;
    };
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        return None;
    };
    if first_page.selection != PdfPageSelection::First
        || merge.composition != PdfMergeComposition::Independent
        || !native_label(&merge.name)
        || first_source.page_selection != PdfPageSelection::First
        || !matches!(
            second_source.page_selection,
            PdfPageSelection::Range { first, last } if first.get() == 2 && last.get() == 2
        )
        || !native_label(&group.name)
        || group.children.is_empty()
    {
        return None;
    }

    let first_source_region = native_region(&first_source.region)?;
    let second_source_region = native_region(&second_source.region)?;
    let rows_region = native_region(&rows.region)?;
    let group_region = native_region(&group.region)?;
    let edge_find = native_edge_find(rows.find)?;
    let minimum_extent = match rows.minimum_extent {
        Some(value) if value.is_finite() && value > 0.0 => format!("{value}pt"),
        None => String::new(),
        _ => return None,
    };

    let mut rendered = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document><Template version=\"1\"><Model><Root id=\"1\"><Children>\n\
         <Grouping id=\"2\"><Children>\n",
    );
    render_capture(&mut rendered, company, 3)?;
    let _ = write!(
        rendered,
        "<MergeSource id=\"4\"><Region>{}</Region><Target>{}</Target></MergeSource>\n\
         </Children><Label/><Kind><OneGroupPerPage/></Kind><Filter>1</Filter></Grouping>\n\
         <Grouping id=\"5\"><Children>\
         <MergeSource id=\"6\"><Region>{}</Region><Target>{}</Target></MergeSource>\
         </Children><Label/><Kind><OneGroupPerPage/></Kind><Filter>2</Filter></Grouping>\n\
         <MergeTarget id=\"7\"><Children>\
         <Splitter id=\"8\"><Children>\
         <Grouping id=\"9\"><Children>\n",
        xml_escape(&first_source_region),
        xml_escape(&merge.name),
        xml_escape(&second_source_region),
        xml_escape(&merge.name),
    );
    if rendered.len() > MAX_PXT_BYTES {
        return None;
    }
    for (index, command) in group.children.iter().enumerate() {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        render_capture(&mut rendered, capture, index + 10)?;
    }
    let _ = write!(
        rendered,
        "</Children><Label>{}</Label><Region>{}</Region>\
         <Kind><OneGroupPerPage/></Kind><Filter/></Grouping>\
         </Children><Region>{}</Region><Search/><SkipInitial>0</SkipInitial><SkipFinal>0</SkipFinal>\
         {}<PostProcess><MinimumExtent>{}</MinimumExtent><Behavior><discard/></Behavior></PostProcess>\
         </Splitter></Children><Name>{}</Name></MergeTarget>\n\
         </Children><Label>{}</Label></Root></Model></Template></Document>\n",
        xml_escape(&group.name),
        xml_escape(&group_region),
        xml_escape(&rows_region),
        edge_find,
        xml_escape(&minimum_extent),
        xml_escape(&merge.name),
        xml_escape(layout.root_name()),
    );
    if rendered.len() > MAX_PXT_BYTES {
        return None;
    }
    let restored =
        crate::import::parse_native_pdf_template_text(&rendered, layout.root_name()).ok()?;
    same_merge_layout(layout, &restored).then_some(rendered)
}

fn render_capture(rendered: &mut String, capture: &PdfCapture, id: usize) -> Option<()> {
    if !native_label(&capture.name)
        || !matches!(
            capture.algorithm,
            PdfCaptureAlgorithm::BasicVisual {
                separate_words: PdfWordSeparation::InsertSpace,
                whitespace: PdfWhitespaceMode::Default,
            }
        )
    {
        return None;
    }
    let region = native_region(&capture.region)?;
    let _ = writeln!(
        rendered,
        "<Capture id=\"{id}\"><Label>{}</Label><Region>{}</Region>\
         <Algorithm><BasicVisual><BaselineCapture/><ParagraphSpacing/><BaselineAngle/>\
         <AngleDeviation/><SeparateWords><InsertSpace/></SeparateWords>\
         <WhitespaceMode><Default/></WhitespaceMode></BasicVisual></Algorithm></Capture>",
        xml_escape(&capture.name),
        xml_escape(&region),
    );
    (rendered.len() <= MAX_PXT_BYTES).then_some(())
}

fn native_edge_find(find: PdfEdgeFind) -> Option<String> {
    if !find.fill.is_finite()
        || find.fill <= 0.0
        || !find.prominence.is_finite()
        || find.prominence < 0.0
    {
        return None;
    }
    Some(format!(
        "<FeatureFind><EdgeFind><Fill>{}pt</Fill><Prominence>{}</Prominence>\
         <Resolution><Standard/></Resolution></EdgeFind></FeatureFind>",
        find.fill, find.prominence,
    ))
}

fn same_merge_layout(expected: &PdfLayout, actual: &PdfLayout) -> bool {
    if expected.root_name() != actual.root_name()
        || expected.page_selection() != actual.page_selection()
    {
        return false;
    }
    let (
        [
            PdfCommand::Pages(expected_page),
            PdfCommand::Merge(expected_merge),
        ],
        [
            PdfCommand::Pages(actual_page),
            PdfCommand::Merge(actual_merge),
        ],
    ) = (expected.commands(), actual.commands())
    else {
        return false;
    };
    let ([PdfCommand::Capture(expected_company)], [PdfCommand::Capture(actual_company)]) = (
        expected_page.children.as_slice(),
        actual_page.children.as_slice(),
    ) else {
        return false;
    };
    if expected_page.selection != actual_page.selection
        || !same_capture(expected_company, actual_company)
        || expected_merge.name != actual_merge.name
        || expected_merge.composition != actual_merge.composition
        || expected_merge.sources.len() != actual_merge.sources.len()
        || !expected_merge
            .sources
            .iter()
            .zip(&actual_merge.sources)
            .all(|(expected, actual)| {
                expected.page_selection == actual.page_selection
                    && same_region(&expected.region, &actual.region)
            })
    {
        return false;
    }
    let ([PdfCommand::EdgeRows(expected_rows)], [PdfCommand::EdgeRows(actual_rows)]) = (
        expected_merge.children.as_slice(),
        actual_merge.children.as_slice(),
    ) else {
        return false;
    };
    if !same_region(&expected_rows.region, &actual_rows.region)
        || !same_edge_find(expected_rows.find, actual_rows.find)
        || !same_optional_float(expected_rows.minimum_extent, actual_rows.minimum_extent)
        || !same_optional_region(
            expected_rows.fallback_anchor.as_ref(),
            actual_rows.fallback_anchor.as_ref(),
        )
    {
        return false;
    }
    let ([PdfCommand::GroupPerPage(expected_group)], [PdfCommand::GroupPerPage(actual_group)]) = (
        expected_rows.children.as_slice(),
        actual_rows.children.as_slice(),
    ) else {
        return false;
    };
    expected_group.name == actual_group.name
        && same_region(&expected_group.region, &actual_group.region)
        && same_capture_commands(&expected_group.children, &actual_group.children)
}

fn same_edge_find(expected: PdfEdgeFind, actual: PdfEdgeFind) -> bool {
    expected.fill.to_bits() == actual.fill.to_bits()
        && expected.prominence.to_bits() == actual.prominence.to_bits()
}

fn same_optional_float(expected: Option<f64>, actual: Option<f64>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => expected.to_bits() == actual.to_bits(),
        (None, None) => true,
        _ => false,
    }
}

fn same_optional_region(expected: Option<&PdfRegion>, actual: Option<&PdfRegion>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => same_region(expected, actual),
        (None, None) => true,
        _ => false,
    }
}
