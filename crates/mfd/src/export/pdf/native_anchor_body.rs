use std::fmt::Write as _;

use mapping::{PdfAnchorAxis, PdfCommand, PdfLayout, PdfPageSelection, PdfReference, PdfRegion};

use super::native_anchor_rows::{
    bounded, native_anchor_name, native_edge_find, native_region, render_anchor, render_capture,
    same_layout_bits, take_id,
};
use super::{native_label, xml_escape};

/// A first-page named header, followed by an all-page vertical boundary and
/// painted-edge rows. Each row has only ordered horizontal anchors and direct
/// default visual captures. This is distinct from the Garden first-page family.
pub(super) fn template(layout: &PdfLayout) -> Option<String> {
    if layout.page_selection() != PdfPageSelection::All || !native_label(layout.root_name()) {
        return None;
    }
    let [
        PdfCommand::Pages(header_page),
        PdfCommand::BoundaryFindVertical(boundary),
        PdfCommand::EdgeRows(rows),
    ] = layout.commands()
    else {
        return None;
    };
    let [PdfCommand::GroupPerPage(header)] = header_page.children.as_slice() else {
        return None;
    };
    let [PdfCommand::GroupPerPage(row_group)] = rows.children.as_slice() else {
        return None;
    };
    if header_page.selection != PdfPageSelection::First
        || !native_label(&header.name)
        || header.region != PdfRegion::full()
        || header.children.is_empty()
        || !native_label(&row_group.name)
        || row_group.region != PdfRegion::full()
        || rows.fallback_anchor.is_some()
        || !native_anchor_name(&boundary.begin_anchor)
        || !native_anchor_name(&boundary.end_anchor)
    {
        return None;
    }
    let first_row_capture = row_group
        .children
        .iter()
        .position(|command| matches!(command, PdfCommand::Capture(_)))?;
    let (horizontal_anchors, row_captures) = row_group.children.split_at(first_row_capture);
    if horizontal_anchors.len() < 2 || row_captures.is_empty() {
        return None;
    }
    let (PdfReference::Anchor(begin), PdfReference::Anchor(end)) =
        (&rows.region.top.reference, &rows.region.bottom.reference)
    else {
        return None;
    };
    if begin != &boundary.begin_anchor
        || end != &boundary.end_anchor
        || matches!(rows.region.left.reference, PdfReference::Anchor(_))
        || matches!(rows.region.right.reference, PdfReference::Anchor(_))
    {
        return None;
    }
    let header_region = native_region(&header.region, &[])?;
    let boundary_region = native_region(&boundary.region, &[])?;
    let row_region = native_region(
        &rows.region,
        &[boundary.begin_anchor.as_str(), boundary.end_anchor.as_str()],
    )?;
    let row_group_region = native_region(&row_group.region, &[])?;
    let boundary_find = native_edge_find(boundary.find)?;
    let row_find = native_edge_find(rows.find)?;
    let minimum_extent = match rows.minimum_extent {
        Some(value) if value.is_finite() && value > 0.0 => format!("{value}pt"),
        None => String::new(),
        _ => return None,
    };

    let mut rendered = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document><Template version=\"1\"><Model><Root id=\"1\"><Children>\n",
    );
    let mut next_id = 2_usize;
    let header_id = take_id(&mut next_id)?;
    let _ = write!(rendered, "<Grouping id=\"{header_id}\"><Children>");
    bounded(&rendered)?;
    for command in &header.children {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        render_capture(&mut rendered, capture, &[], &mut next_id)?;
    }
    let _ = write!(
        rendered,
        "</Children><Label>{}</Label><Region>{}</Region>\
         <Kind><OneGroupPerPage/></Kind><Filter>1</Filter></Grouping>",
        xml_escape(&header.name),
        xml_escape(&header_region),
    );
    bounded(&rendered)?;

    let body_id = take_id(&mut next_id)?;
    let boundary_id = take_id(&mut next_id)?;
    let splitter_id = take_id(&mut next_id)?;
    let row_group_id = take_id(&mut next_id)?;
    let _ = write!(
        rendered,
        "<Grouping id=\"{body_id}\"><Children>\
         <BoundaryFindVertical id=\"{boundary_id}\">\
         <NameBegin>{}</NameBegin><NameEnd>{}</NameEnd>\
         <Region>{}</Region>{}</BoundaryFindVertical>\
         <Splitter id=\"{splitter_id}\"><Children>\
         <Grouping id=\"{row_group_id}\"><Children>",
        xml_escape(&boundary.begin_anchor),
        xml_escape(&boundary.end_anchor),
        xml_escape(&boundary_region),
        boundary_find,
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
    let _ = writeln!(
        rendered,
        "</Children><Label>{}</Label><Region>{}</Region>\
         <Kind><OneGroupPerPage/></Kind><Filter/></Grouping>\
         </Children><Region>{}</Region>\
         <Search/><SkipInitial>0</SkipInitial><SkipFinal>0</SkipFinal>\
         {}<PostProcess><MinimumExtent>{}</MinimumExtent>\
         <Behavior><discard/></Behavior></PostProcess></Splitter>\
         </Children><Label/><Kind><OneGroupPerPage/></Kind><Filter/></Grouping>\
         </Children><Label>{}</Label></Root></Model></Template></Document>",
        xml_escape(&row_group.name),
        xml_escape(&row_group_region),
        xml_escape(&row_region),
        row_find,
        xml_escape(&minimum_extent),
        xml_escape(layout.root_name()),
    );
    bounded(&rendered)?;

    let restored =
        crate::import::parse_native_pdf_template_text(&rendered, layout.root_name()).ok()?;
    same_layout_bits(layout, &restored).then_some(rendered)
}
