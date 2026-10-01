use std::fmt::Write as _;

use mapping::{
    PdfAnchorAssignment, PdfAnchorAxis, PdfCapture, PdfCaptureAlgorithm, PdfCommand, PdfCoordinate,
    PdfEdgeFind, PdfLayout, PdfPageSelection, PdfReference, PdfRegion, PdfWhitespaceMode,
    PdfWordSeparation,
};

use super::{MAX_PXT_BYTES, native_label, same_capture, same_region, xml_escape};

/// One first-page transparent group with fixed vertical anchors, direct captures,
/// one painted-edge row splitter, and optional direct captures after its rows.
/// Each row has one named group with horizontal anchors before its captures.
pub(super) fn template(layout: &PdfLayout) -> Option<String> {
    if layout.page_selection() != PdfPageSelection::All || !native_label(layout.root_name()) {
        return None;
    }
    let [PdfCommand::Pages(page)] = layout.commands() else {
        return None;
    };
    if page.selection != PdfPageSelection::First {
        return None;
    }
    let row_index = page
        .children
        .iter()
        .position(|command| matches!(command, PdfCommand::EdgeRows(_)))?;
    let (before_rows, rows_and_after) = page.children.split_at(row_index);
    let [PdfCommand::EdgeRows(rows), after_rows @ ..] = rows_and_after else {
        return None;
    };
    let first_capture = before_rows
        .iter()
        .position(|command| matches!(command, PdfCommand::Capture(_)))?;
    let (vertical_anchors, header_captures) = before_rows.split_at(first_capture);
    if vertical_anchors.len() < 2 || header_captures.is_empty() {
        return None;
    }
    let [PdfCommand::GroupPerPage(group)] = rows.children.as_slice() else {
        return None;
    };
    if !native_label(&group.name) || group.region != PdfRegion::full() {
        return None;
    }
    let first_row_capture = group
        .children
        .iter()
        .position(|command| matches!(command, PdfCommand::Capture(_)))?;
    let (horizontal_anchors, row_captures) = group.children.split_at(first_row_capture);
    if horizontal_anchors.len() < 2 || row_captures.is_empty() || rows.fallback_anchor.is_some() {
        return None;
    }

    let mut rendered = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document><Template version=\"1\"><Model><Root id=\"1\"><Children>\n\
         <Grouping id=\"2\"><Children>\n",
    );
    let mut next_id = 3_usize;
    let mut vertical_names = Vec::with_capacity(vertical_anchors.len());
    for command in vertical_anchors {
        let PdfCommand::Anchor(anchor) = command else {
            return None;
        };
        if anchor.axis != PdfAnchorAxis::Vertical {
            return None;
        }
        render_anchor(&mut rendered, anchor, &vertical_names, &mut next_id)?;
        vertical_names.push(anchor.name.as_str());
    }
    for command in header_captures {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        render_capture(&mut rendered, capture, &[], &mut next_id)?;
    }

    let (PdfReference::Anchor(begin), PdfReference::Anchor(end)) =
        (&rows.region.top.reference, &rows.region.bottom.reference)
    else {
        return None;
    };
    if !vertical_names.contains(&begin.as_str()) || !vertical_names.contains(&end.as_str()) {
        return None;
    }
    if matches!(rows.region.left.reference, PdfReference::Anchor(_))
        || matches!(rows.region.right.reference, PdfReference::Anchor(_))
    {
        return None;
    }
    let rows_region = native_region(&rows.region, &vertical_names)?;
    let group_region = native_region(&group.region, &[])?;
    let edge_find = native_edge_find(rows.find)?;
    let minimum_extent = match rows.minimum_extent {
        Some(value) if value.is_finite() && value > 0.0 => format!("{value}pt"),
        None => String::new(),
        _ => return None,
    };
    let splitter_id = take_id(&mut next_id)?;
    let row_group_id = take_id(&mut next_id)?;
    let _ = write!(
        rendered,
        "<Splitter id=\"{splitter_id}\"><Children>\
         <Grouping id=\"{row_group_id}\"><Children>"
    );
    bounded(&rendered)?;

    let mut horizontal_names = Vec::with_capacity(horizontal_anchors.len());
    for command in horizontal_anchors {
        let PdfCommand::Anchor(anchor) = command else {
            return None;
        };
        if anchor.axis != PdfAnchorAxis::Horizontal {
            return None;
        }
        render_anchor(&mut rendered, anchor, &horizontal_names, &mut next_id)?;
        horizontal_names.push(anchor.name.as_str());
    }
    for command in row_captures {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        let (PdfReference::Anchor(left), PdfReference::Anchor(right)) = (
            &capture.region.left.reference,
            &capture.region.right.reference,
        ) else {
            return None;
        };
        if !horizontal_names.contains(&left.as_str())
            || !horizontal_names.contains(&right.as_str())
            || matches!(capture.region.top.reference, PdfReference::Anchor(_))
            || matches!(capture.region.bottom.reference, PdfReference::Anchor(_))
        {
            return None;
        }
        render_capture(&mut rendered, capture, &horizontal_names, &mut next_id)?;
    }
    let _ = write!(
        rendered,
        "</Children><Label>{}</Label><Region>{}</Region>\
         <Kind><OneGroupPerPage/></Kind><Filter/></Grouping>\
         </Children><Region>{}</Region>\
         <Search/><SkipInitial>0</SkipInitial><SkipFinal>0</SkipFinal>\
         {}<PostProcess><MinimumExtent>{}</MinimumExtent>\
         <Behavior><discard/></Behavior></PostProcess></Splitter>",
        xml_escape(&group.name),
        xml_escape(&group_region),
        xml_escape(&rows_region),
        edge_find,
        xml_escape(&minimum_extent),
    );
    bounded(&rendered)?;
    for command in after_rows {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        render_capture(&mut rendered, capture, &[], &mut next_id)?;
    }
    let _ = writeln!(
        rendered,
        "</Children><Label/><Kind><OneGroupPerPage/></Kind><Filter>1</Filter>\
         </Grouping></Children><Label>{}</Label>\
         </Root></Model></Template></Document>",
        xml_escape(layout.root_name()),
    );
    bounded(&rendered)?;

    let restored =
        crate::import::parse_native_pdf_template_text(&rendered, layout.root_name()).ok()?;
    same_layout_bits(layout, &restored).then_some(rendered)
}

pub(super) fn render_anchor(
    output: &mut String,
    anchor: &PdfAnchorAssignment,
    known: &[&str],
    next_id: &mut usize,
) -> Option<()> {
    if !native_anchor_name(&anchor.name) {
        return None;
    }
    let tag = match anchor.axis {
        PdfAnchorAxis::Horizontal => "HorizontalAnchorAssignment",
        PdfAnchorAxis::Vertical => "VerticalAnchorAssignment",
    };
    let expression = native_coordinate(&anchor.at, known)?;
    let id = take_id(next_id)?;
    let _ = write!(
        output,
        "<{tag} id=\"{id}\"><Name>{}</Name><Expression>{}</Expression></{tag}>",
        xml_escape(&anchor.name),
        xml_escape(&expression),
    );
    bounded(output)
}

pub(super) fn render_capture(
    output: &mut String,
    capture: &PdfCapture,
    known: &[&str],
    next_id: &mut usize,
) -> Option<()> {
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
    let region = native_region(&capture.region, known)?;
    let id = take_id(next_id)?;
    let _ = write!(
        output,
        "<Capture id=\"{id}\"><Label>{}</Label><Region>{}</Region>\
         <Algorithm><BasicVisual><BaselineCapture/><ParagraphSpacing/>\
         <BaselineAngle/><AngleDeviation/><SeparateWords><InsertSpace/></SeparateWords>\
         <WhitespaceMode><Default/></WhitespaceMode></BasicVisual></Algorithm></Capture>",
        xml_escape(&capture.name),
        xml_escape(&region),
    );
    bounded(output)
}

pub(super) fn native_anchor_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub(super) fn native_region(region: &PdfRegion, known: &[&str]) -> Option<String> {
    Some(format!(
        "{{ Left: {}, Top: {}, Right: {}, Bottom: {} }}",
        native_coordinate(&region.left, known)?,
        native_coordinate(&region.top, known)?,
        native_coordinate(&region.right, known)?,
        native_coordinate(&region.bottom, known)?,
    ))
}

fn native_coordinate(coordinate: &PdfCoordinate, known: &[&str]) -> Option<String> {
    if !coordinate.offset.is_finite() {
        return None;
    }
    let reference = match &coordinate.reference {
        PdfReference::Left => "Left".to_string(),
        PdfReference::Top => "Top".to_string(),
        PdfReference::Right => "Right".to_string(),
        PdfReference::Bottom => "Bottom".to_string(),
        PdfReference::Anchor(name)
            if native_anchor_name(name) && known.contains(&name.as_str()) =>
        {
            format!("[{name}]")
        }
        PdfReference::Anchor(_) => return None,
    };
    let sign = if coordinate.offset.is_sign_negative() {
        '-'
    } else {
        '+'
    };
    Some(format!("{reference} {sign} {}pt", coordinate.offset.abs()))
}

pub(super) fn native_edge_find(find: PdfEdgeFind) -> Option<String> {
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

pub(super) fn take_id(next: &mut usize) -> Option<usize> {
    let id = *next;
    *next = next.checked_add(1)?;
    Some(id)
}

pub(super) fn bounded(output: &str) -> Option<()> {
    (output.len() <= MAX_PXT_BYTES).then_some(())
}

pub(super) fn same_layout_bits(expected: &PdfLayout, actual: &PdfLayout) -> bool {
    expected.root_name() == actual.root_name()
        && expected.page_selection() == actual.page_selection()
        && same_commands_bits(expected.commands(), actual.commands())
}

fn same_commands_bits(expected: &[PdfCommand], actual: &[PdfCommand]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(expected, actual)| match (expected, actual) {
                (PdfCommand::Capture(expected), PdfCommand::Capture(actual)) => {
                    same_capture(expected, actual)
                }
                (PdfCommand::Anchor(expected), PdfCommand::Anchor(actual)) => {
                    expected.name == actual.name
                        && expected.axis == actual.axis
                        && same_coordinate_bits(&expected.at, &actual.at)
                }
                (PdfCommand::GroupPerPage(expected), PdfCommand::GroupPerPage(actual)) => {
                    expected.name == actual.name
                        && same_region(&expected.region, &actual.region)
                        && same_commands_bits(&expected.children, &actual.children)
                }
                (PdfCommand::Pages(expected), PdfCommand::Pages(actual)) => {
                    expected.selection == actual.selection
                        && same_commands_bits(&expected.children, &actual.children)
                }
                (PdfCommand::EdgeRows(expected), PdfCommand::EdgeRows(actual)) => {
                    same_region(&expected.region, &actual.region)
                        && expected.find.fill.to_bits() == actual.find.fill.to_bits()
                        && expected.find.prominence.to_bits() == actual.find.prominence.to_bits()
                        && same_optional_float(expected.minimum_extent, actual.minimum_extent)
                        && same_optional_region(
                            expected.fallback_anchor.as_ref(),
                            actual.fallback_anchor.as_ref(),
                        )
                        && same_commands_bits(&expected.children, &actual.children)
                }
                (
                    PdfCommand::BoundaryFindVertical(expected),
                    PdfCommand::BoundaryFindVertical(actual),
                ) => {
                    expected.begin_anchor == actual.begin_anchor
                        && expected.end_anchor == actual.end_anchor
                        && same_region(&expected.region, &actual.region)
                        && expected.find.fill.to_bits() == actual.find.fill.to_bits()
                        && expected.find.prominence.to_bits() == actual.find.prominence.to_bits()
                }
                _ => false,
            })
}

fn same_coordinate_bits(expected: &PdfCoordinate, actual: &PdfCoordinate) -> bool {
    expected.reference == actual.reference && expected.offset.to_bits() == actual.offset.to_bits()
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
