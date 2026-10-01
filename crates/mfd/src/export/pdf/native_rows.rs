use std::fmt::Write as _;

use mapping::{
    PdfCaptureAlgorithm, PdfCommand, PdfCoordinate, PdfEdgeFind, PdfLayout, PdfPageSelection,
    PdfReference, PdfRegion, PdfWhitespaceMode, PdfWordSeparation,
};

use super::{
    MAX_PXT_BYTES, native_label, native_region, same_capture_commands, same_region, xml_escape,
};

/// A root vertical boundary followed by one edge-row splitter, containing one
/// named page group with direct default text captures. The splitter's fallback
/// anchor is not present in native XML; only the parser's exact inferred value
/// can pass the final layout comparison.
pub(super) fn template(layout: &PdfLayout) -> Option<String> {
    if layout.page_selection() != PdfPageSelection::All || !native_label(layout.root_name()) {
        return None;
    }
    let [
        PdfCommand::BoundaryFindVertical(boundary),
        PdfCommand::EdgeRows(rows),
    ] = layout.commands()
    else {
        return None;
    };
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        return None;
    };
    if !native_anchor_name(&boundary.begin_anchor)
        || !native_anchor_name(&boundary.end_anchor)
        || !native_label(&group.name)
        || group.children.is_empty()
    {
        return None;
    }
    let boundary_region = native_region(&boundary.region)?;
    let rows_region =
        native_rows_region(&rows.region, &boundary.begin_anchor, &boundary.end_anchor)?;
    let group_region = native_region(&group.region)?;
    let boundary_find = native_edge_find(boundary.find)?;
    let rows_find = native_edge_find(rows.find)?;
    let minimum_extent = match rows.minimum_extent {
        Some(value) if value.is_finite() && value > 0.0 => format!("{value}pt"),
        None => String::new(),
        _ => return None,
    };

    let mut rendered = String::new();
    let _ = write!(
        rendered,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document>\n\
         \t<Template version=\"1\">\n\
         \t\t<Model>\n\
         \t\t\t<Root id=\"1\">\n\
         \t\t\t\t<Children>\n\
         \t\t\t\t\t<BoundaryFindVertical id=\"2\">\n\
         \t\t\t\t\t\t<NameBegin>{}</NameBegin>\n\
         \t\t\t\t\t\t<NameEnd>{}</NameEnd>\n\
         \t\t\t\t\t\t<Region>{}</Region>\n\
         \t\t\t\t\t\t{}\n\
         \t\t\t\t\t</BoundaryFindVertical>\n\
         \t\t\t\t\t<Splitter id=\"3\">\n\
         \t\t\t\t\t\t<Children>\n\
         \t\t\t\t\t\t\t<Grouping id=\"4\">\n\
         \t\t\t\t\t\t\t\t<Children>\n",
        xml_escape(&boundary.begin_anchor),
        xml_escape(&boundary.end_anchor),
        xml_escape(&boundary_region),
        boundary_find,
    );
    if rendered.len() > MAX_PXT_BYTES {
        return None;
    }
    for (index, command) in group.children.iter().enumerate() {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        if !native_label(&capture.name)
            || !matches!(
                capture.algorithm,
                PdfCaptureAlgorithm::BasicVisual {
                    separate_words: PdfWordSeparation::InsertSpace,
                    whitespace: PdfWhitespaceMode::Default
                }
            )
        {
            return None;
        }
        let region = native_region(&capture.region)?;
        let _ = write!(
            rendered,
            "\t\t\t\t\t\t\t\t\t<Capture id=\"{}\">\n\
             \t\t\t\t\t\t\t\t\t\t<Label>{}</Label>\n\
             \t\t\t\t\t\t\t\t\t\t<Region>{}</Region>\n\
             \t\t\t\t\t\t\t\t\t\t<Algorithm><BasicVisual><BaselineCapture/><ParagraphSpacing/><BaselineAngle/><AngleDeviation/><SeparateWords><InsertSpace/></SeparateWords><WhitespaceMode><Default/></WhitespaceMode></BasicVisual></Algorithm>\n\
             \t\t\t\t\t\t\t\t\t</Capture>\n",
            index + 5,
            xml_escape(&capture.name),
            xml_escape(&region),
        );
        if rendered.len() > MAX_PXT_BYTES {
            return None;
        }
    }
    let _ = write!(
        rendered,
        "\t\t\t\t\t\t\t\t</Children>\n\
         \t\t\t\t\t\t\t\t<Label>{}</Label>\n\
         \t\t\t\t\t\t\t\t<Region>{}</Region>\n\
         \t\t\t\t\t\t\t\t<Kind><OneGroupPerPage/></Kind>\n\
         \t\t\t\t\t\t\t\t<Filter/>\n\
         \t\t\t\t\t\t\t</Grouping>\n\
         \t\t\t\t\t\t</Children>\n\
         \t\t\t\t\t\t<Region>{}</Region>\n\
         \t\t\t\t\t\t<Search/>\n\
         \t\t\t\t\t\t<SkipInitial>0</SkipInitial>\n\
         \t\t\t\t\t\t<SkipFinal>0</SkipFinal>\n\
         \t\t\t\t\t\t{}\n\
         \t\t\t\t\t\t<PostProcess><MinimumExtent>{}</MinimumExtent><Behavior><discard/></Behavior></PostProcess>\n\
         \t\t\t\t\t</Splitter>\n\
         \t\t\t\t</Children>\n\
         \t\t\t\t<Label>{}</Label>\n\
         \t\t\t</Root>\n\
         \t\t</Model>\n\
         \t</Template>\n\
         </Document>\n",
        xml_escape(&group.name),
        xml_escape(&group_region),
        xml_escape(&rows_region),
        rows_find,
        xml_escape(&minimum_extent),
        xml_escape(layout.root_name()),
    );
    if rendered.len() > MAX_PXT_BYTES {
        return None;
    }
    let restored =
        crate::import::parse_native_pdf_template_text(&rendered, layout.root_name()).ok()?;
    same_rows_layout(layout, &restored).then_some(rendered)
}

fn native_anchor_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn native_rows_region(region: &PdfRegion, begin: &str, end: &str) -> Option<String> {
    if !matches!(&region.left.reference, PdfReference::Left)
        || !matches!(&region.right.reference, PdfReference::Right)
    {
        return None;
    }
    Some(format!(
        "{{ Left: {}, Top: {}, Right: {}, Bottom: {} }}",
        super::native_coordinate(&region.left)?,
        native_anchor_coordinate(&region.top, begin)?,
        super::native_coordinate(&region.right)?,
        native_anchor_coordinate(&region.bottom, end)?,
    ))
}

fn native_anchor_coordinate(coordinate: &PdfCoordinate, expected_name: &str) -> Option<String> {
    let PdfReference::Anchor(name) = &coordinate.reference else {
        return None;
    };
    if name != expected_name || !coordinate.offset.is_finite() {
        return None;
    }
    let sign = if coordinate.offset.is_sign_negative() {
        '-'
    } else {
        '+'
    };
    Some(format!("[{name}] {sign} {}pt", coordinate.offset.abs()))
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
        "<FeatureFind><EdgeFind><Fill>{}pt</Fill><Prominence>{}</Prominence><Resolution><Standard/></Resolution></EdgeFind></FeatureFind>",
        find.fill, find.prominence
    ))
}

fn same_rows_layout(expected: &PdfLayout, actual: &PdfLayout) -> bool {
    if expected.root_name() != actual.root_name()
        || expected.page_selection() != actual.page_selection()
    {
        return false;
    }
    let (
        [
            PdfCommand::BoundaryFindVertical(expected_boundary),
            PdfCommand::EdgeRows(expected_rows),
        ],
        [
            PdfCommand::BoundaryFindVertical(actual_boundary),
            PdfCommand::EdgeRows(actual_rows),
        ],
    ) = (expected.commands(), actual.commands())
    else {
        return false;
    };
    if expected_boundary.begin_anchor != actual_boundary.begin_anchor
        || expected_boundary.end_anchor != actual_boundary.end_anchor
        || !same_region(&expected_boundary.region, &actual_boundary.region)
        || !same_edge_find(expected_boundary.find, actual_boundary.find)
        || !same_region(&expected_rows.region, &actual_rows.region)
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
