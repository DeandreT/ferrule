use std::collections::BTreeMap;

use ir::{Instance, Value};
use mapping::{PdfCommand, PdfCoordinate, PdfEdgeFind, PdfLayout, PdfReference, PdfRegion};

use crate::extract::{Glyph, Page, Rect};
use crate::{MAX_INSTANCE_DEPTH, MAX_OUTPUT_NODES, MAX_VALUE_BYTES, PdfError};

const LEVEL_EPSILON: f64 = 1.0;
const EMPTY_EPSILON: f64 = 0.01;

pub(super) fn evaluate(pages: &[Page], layout: &PdfLayout) -> Result<Instance, PdfError> {
    let mut fields = Vec::new();
    let mut budget = OutputBudget::default();
    let mut page_anchors = BTreeMap::new();
    budget.node()?;
    let mut selections = vec![layout.page_selection()];
    evaluate_global_commands(
        layout.commands(),
        pages,
        &mut selections,
        &mut page_anchors,
        &mut fields,
        &mut budget,
    )?;
    Ok(Instance::Group(fields))
}

fn evaluate_global_commands(
    commands: &[PdfCommand],
    pages: &[Page],
    selections: &mut Vec<mapping::PdfPageSelection>,
    page_anchors: &mut BTreeMap<u32, BTreeMap<String, f64>>,
    fields: &mut Vec<(String, Instance)>,
    budget: &mut OutputBudget,
) -> Result<(), PdfError> {
    let mut index = 0;
    while index < commands.len() {
        match &commands[index] {
            PdfCommand::Pages(selected) => {
                selections.push(selected.selection);
                let mut selected_anchors = BTreeMap::new();
                evaluate_global_commands(
                    &selected.children,
                    pages,
                    selections,
                    &mut selected_anchors,
                    fields,
                    budget,
                )?;
                selections.pop();
                index += 1;
            }
            PdfCommand::Merge(merge) => {
                for source in &merge.sources {
                    selections.push(source.page_selection);
                    for page in pages
                        .iter()
                        .filter(|page| page_is_selected(page.number, selections))
                    {
                        let mut anchors = BTreeMap::new();
                        let region = resolve_region(&source.region, page.bounds, &anchors)?;
                        let produced = evaluate_commands(
                            &merge.children,
                            page,
                            region,
                            &mut anchors,
                            budget,
                            1,
                        )?;
                        merge_fields(fields, produced)?;
                    }
                    selections.pop();
                }
                index += 1;
            }
            _ => {
                let start = index;
                while index < commands.len()
                    && !matches!(commands[index], PdfCommand::Pages(_) | PdfCommand::Merge(_))
                {
                    index += 1;
                }
                for page in pages
                    .iter()
                    .filter(|page| page_is_selected(page.number, selections))
                {
                    let anchors = page_anchors.entry(page.number).or_default();
                    let produced = evaluate_commands(
                        &commands[start..index],
                        page,
                        page.bounds,
                        anchors,
                        budget,
                        1,
                    )?;
                    merge_fields(fields, produced)?;
                }
            }
        }
    }
    Ok(())
}

fn page_is_selected(page: u32, selections: &[mapping::PdfPageSelection]) -> bool {
    selections.iter().all(|selection| selection.includes(page))
}

fn evaluate_commands(
    commands: &[PdfCommand],
    page: &Page,
    current: Rect,
    anchors: &mut BTreeMap<String, f64>,
    budget: &mut OutputBudget,
    depth: usize,
) -> Result<Vec<(String, Instance)>, PdfError> {
    budget.depth(depth)?;
    let mut fields = Vec::new();
    for command in commands {
        match command {
            PdfCommand::Capture(capture) => {
                let region = resolve_region(&capture.region, current, anchors)?;
                let value = capture_text(page, region, &capture.name, budget)?;
                budget.node()?;
                fields.push((capture.name.clone(), Instance::Scalar(value)));
            }
            PdfCommand::GroupPerPage(group) => {
                let region = resolve_region(&group.region, current, anchors)?;
                let mut child_anchors = anchors.clone();
                let children = evaluate_commands(
                    &group.children,
                    page,
                    region,
                    &mut child_anchors,
                    budget,
                    depth + 2,
                )?;
                budget.nodes(2)?;
                fields.push((
                    group.name.clone(),
                    Instance::Repeated(vec![Instance::Group(children)]),
                ));
            }
            PdfCommand::EdgeRows(rows) => {
                let region = resolve_region(&rows.region, current, anchors)?;
                let fallback_anchor = rows
                    .fallback_anchor
                    .as_ref()
                    .map(|anchor| resolve_region(anchor, region, anchors))
                    .transpose()?;
                let mut row_fields = Vec::new();
                for row in row_regions(
                    page,
                    region,
                    rows.find,
                    rows.minimum_extent,
                    fallback_anchor,
                ) {
                    if !page_has_text(page, row) {
                        continue;
                    }
                    let mut child_anchors = anchors.clone();
                    let produced = match evaluate_commands(
                        &rows.children,
                        page,
                        row,
                        &mut child_anchors,
                        budget,
                        depth + 1,
                    ) {
                        Ok(produced) => produced,
                        Err(PdfError::InvalidCandidateRegion) => continue,
                        Err(error) => return Err(error),
                    };
                    merge_fields(&mut row_fields, produced)?;
                }
                merge_fields(&mut fields, row_fields)?;
            }
            PdfCommand::Pages(_) | PdfCommand::Merge(_) => {
                return Err(PdfError::InvalidLayout(
                    "PDF page-selection and merge commands must be at the layout root".into(),
                ));
            }
            PdfCommand::Anchor(anchor) => {
                let value = resolve_coordinate(&anchor.at, current, anchors)?;
                anchors.insert(anchor.name.clone(), value);
            }
            PdfCommand::BoundaryFindVertical(boundary) => {
                let region = resolve_region(&boundary.region, current, anchors)?;
                let levels = edge_levels(page, region, boundary.find);
                let detected = levels
                    .first()
                    .zip(levels.last())
                    .map(|(begin, end)| (*begin, *end))
                    .or_else(|| text_extents(page, region));
                let Some((begin, end)) = detected else {
                    return Err(PdfError::InvalidLayout(format!(
                        "vertical boundary `{}`/`{}` found no visual content on page {}",
                        boundary.begin_anchor, boundary.end_anchor, page.number
                    )));
                };
                if end - begin <= EMPTY_EPSILON {
                    return Err(PdfError::InvalidLayout(format!(
                        "vertical boundary `{}`/`{}` found fewer than two edges on page {}",
                        boundary.begin_anchor, boundary.end_anchor, page.number
                    )));
                }
                anchors.insert(boundary.begin_anchor.clone(), begin);
                anchors.insert(boundary.end_anchor.clone(), end);
            }
        }
    }
    Ok(fields)
}

fn resolve_region(
    region: &PdfRegion,
    current: Rect,
    anchors: &BTreeMap<String, f64>,
) -> Result<Rect, PdfError> {
    let resolved = Rect {
        left: resolve_coordinate(&region.left, current, anchors)?,
        top: resolve_coordinate(&region.top, current, anchors)?,
        right: resolve_coordinate(&region.right, current, anchors)?,
        bottom: resolve_coordinate(&region.bottom, current, anchors)?,
    };
    if ![resolved.left, resolved.top, resolved.right, resolved.bottom]
        .iter()
        .all(|value| value.is_finite())
        || resolved.right - resolved.left <= EMPTY_EPSILON
        || resolved.bottom - resolved.top <= EMPTY_EPSILON
        || resolved.left < current.left - EMPTY_EPSILON
        || resolved.top < current.top - EMPTY_EPSILON
        || resolved.right > current.right + EMPTY_EPSILON
        || resolved.bottom > current.bottom + EMPTY_EPSILON
    {
        return Err(PdfError::InvalidCandidateRegion);
    }
    Ok(resolved)
}

fn resolve_coordinate(
    coordinate: &PdfCoordinate,
    current: Rect,
    anchors: &BTreeMap<String, f64>,
) -> Result<f64, PdfError> {
    let base = match &coordinate.reference {
        PdfReference::Left => current.left,
        PdfReference::Top => current.top,
        PdfReference::Right => current.right,
        PdfReference::Bottom => current.bottom,
        PdfReference::Anchor(name) => *anchors.get(name).ok_or_else(|| {
            PdfError::InvalidLayout(format!("PDF anchor `{name}` has no run-time value"))
        })?,
    };
    let value = base + coordinate.offset;
    if !value.is_finite() {
        return Err(PdfError::InvalidLayout(
            "PDF coordinate resolved to a non-finite value".into(),
        ));
    }
    Ok(value)
}

fn edge_levels(page: &Page, region: Rect, find: PdfEdgeFind) -> Vec<f64> {
    let mut segments = page
        .horizontal_edges
        .iter()
        .filter_map(|edge| {
            if edge.y + LEVEL_EPSILON < region.top || edge.y - LEVEL_EPSILON > region.bottom {
                return None;
            }
            let left = edge.left.max(region.left);
            let right = edge.right.min(region.right);
            (right - left > EMPTY_EPSILON).then_some((edge.y, left, right))
        })
        .collect::<Vec<_>>();
    segments.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1))
    });

    let mut levels = Vec::new();
    let mut index = 0;
    while index < segments.len() {
        let first_y = segments[index].0;
        let mut same_level = Vec::new();
        while index < segments.len() && (segments[index].0 - first_y).abs() <= LEVEL_EPSILON {
            same_level.push((segments[index].1, segments[index].2));
            index += 1;
        }
        same_level.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut longest: f64 = 0.0;
        if let Some(&(mut left, mut right)) = same_level.first() {
            for &(next_left, next_right) in &same_level[1..] {
                if next_left - right <= find.fill {
                    right = right.max(next_right);
                } else {
                    longest = longest.max(right - left);
                    left = next_left;
                    right = next_right;
                }
            }
            longest = longest.max(right - left);
        }
        if longest + EMPTY_EPSILON >= find.prominence {
            levels.push(first_y);
        }
    }
    levels
}

fn row_regions(
    page: &Page,
    region: Rect,
    find: PdfEdgeFind,
    minimum_extent: Option<f64>,
    fallback_anchor: Option<Rect>,
) -> Vec<Rect> {
    let levels = edge_levels(page, region, find);
    if levels.len() >= 2 {
        let mut boundaries = levels;
        if boundaries
            .first()
            .is_some_and(|first| first - region.top > EMPTY_EPSILON)
        {
            boundaries.insert(0, region.top);
        }
        if boundaries
            .last()
            .is_some_and(|last| region.bottom - last > EMPTY_EPSILON)
        {
            boundaries.push(region.bottom);
        }
        return boundaries
            .windows(2)
            .filter_map(|levels| {
                let top = levels[0].max(region.top);
                let bottom = levels[1].min(region.bottom);
                row_has_minimum_extent(top, bottom, minimum_extent).then_some(Rect {
                    left: region.left,
                    top,
                    right: region.right,
                    bottom,
                })
            })
            .collect();
    }
    text_row_regions(page, region, minimum_extent, fallback_anchor)
}

fn text_extents(page: &Page, region: Rect) -> Option<(f64, f64)> {
    page.glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .fold(None, |extents, glyph| match extents {
            Some((top, bottom)) => Some((
                f64::min(top, glyph.bounds.top),
                f64::max(bottom, glyph.bounds.bottom),
            )),
            None => Some((glyph.bounds.top, glyph.bounds.bottom)),
        })
}

fn text_row_regions(
    page: &Page,
    region: Rect,
    minimum_extent: Option<f64>,
    fallback_anchor: Option<Rect>,
) -> Vec<Rect> {
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .collect::<Vec<_>>();
    glyphs.sort_by(|left, right| vertical_center(left).total_cmp(&vertical_center(right)));
    let mut lines: Vec<TextLine> = Vec::new();
    for glyph in glyphs {
        let center = vertical_center(glyph);
        let height = (glyph.bounds.bottom - glyph.bounds.top).max(1.0);
        let anchored = fallback_anchor.is_some_and(|anchor| glyph_in_region(glyph, anchor));
        match lines.last_mut() {
            Some(line) if (center - line.center()).abs() <= height * 0.5 => {
                line.top = line.top.min(glyph.bounds.top);
                line.bottom = line.bottom.max(glyph.bounds.bottom);
                line.anchored |= anchored;
            }
            _ => lines.push(TextLine {
                top: glyph.bounds.top,
                bottom: glyph.bounds.bottom,
                anchored,
            }),
        }
    }

    let anchors = lines
        .iter()
        .filter(|line| line.anchored)
        .collect::<Vec<_>>();
    let row_lines = if anchors.len() >= 2 {
        anchors
    } else {
        lines.iter().collect::<Vec<_>>()
    };

    row_lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let top = index
                .checked_sub(1)
                .map_or(region.top, |previous| {
                    (row_lines[previous].center() + line.center()) / 2.0
                })
                .max(region.top);
            let bottom = row_lines
                .get(index + 1)
                .map_or(region.bottom, |next| (line.center() + next.center()) / 2.0)
                .min(region.bottom);
            row_has_minimum_extent(top, bottom, minimum_extent).then_some(Rect {
                left: region.left,
                top,
                right: region.right,
                bottom,
            })
        })
        .collect()
}

struct TextLine {
    top: f64,
    bottom: f64,
    anchored: bool,
}

impl TextLine {
    fn center(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

fn row_has_minimum_extent(top: f64, bottom: f64, minimum_extent: Option<f64>) -> bool {
    bottom - top + EMPTY_EPSILON >= minimum_extent.unwrap_or(EMPTY_EPSILON)
}

fn page_has_text(page: &Page, region: Rect) -> bool {
    page.glyphs
        .iter()
        .any(|glyph| glyph_in_region(glyph, region))
}

fn capture_text(
    page: &Page,
    region: Rect,
    path: &str,
    budget: &mut OutputBudget,
) -> Result<Value, PdfError> {
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .collect::<Vec<_>>();
    if glyphs.is_empty() {
        return Ok(Value::Null);
    }
    glyphs.sort_by(|left, right| {
        vertical_center(left)
            .total_cmp(&vertical_center(right))
            .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
    });

    let mut lines: Vec<Vec<&Glyph>> = Vec::new();
    for glyph in glyphs {
        let center = vertical_center(glyph);
        let height = (glyph.bounds.bottom - glyph.bounds.top).max(1.0);
        match lines.last_mut() {
            Some(line)
                if line.last().is_some_and(|previous| {
                    (vertical_center(previous) - center).abs()
                        <= height.max(previous.bounds.bottom - previous.bounds.top) * 0.5
                }) =>
            {
                line.push(glyph);
            }
            _ => lines.push(vec![glyph]),
        }
    }

    let mut value = String::new();
    for (line_index, line) in lines.iter_mut().enumerate() {
        line.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
        if line_index > 0 && !value.ends_with(char::is_whitespace) {
            value.push(' ');
        }
        let mut previous_right = None;
        for glyph in line {
            let height = (glyph.bounds.bottom - glyph.bounds.top).max(1.0);
            if previous_right.is_some_and(|right| {
                glyph.bounds.left - right > height * 0.15
                    && !value.ends_with(char::is_whitespace)
                    && !glyph.text.starts_with(char::is_whitespace)
            }) {
                value.push(' ');
            }
            value.push_str(&glyph.text);
            previous_right = Some(glyph.bounds.right);
            if value.len() > MAX_VALUE_BYTES {
                return Err(PdfError::ValueTooLarge(path.to_owned()));
            }
        }
    }
    let value = value.trim().to_owned();
    budget.value_bytes(value.len(), path)?;
    if value.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(Value::String(value))
    }
}

fn glyph_in_region(glyph: &Glyph, region: Rect) -> bool {
    let x = (glyph.bounds.left + glyph.bounds.right) / 2.0;
    let y = vertical_center(glyph);
    x >= region.left - EMPTY_EPSILON
        && x <= region.right + EMPTY_EPSILON
        && y >= region.top - EMPTY_EPSILON
        && y <= region.bottom + EMPTY_EPSILON
}

fn vertical_center(glyph: &Glyph) -> f64 {
    (glyph.bounds.top + glyph.bounds.bottom) / 2.0
}

fn merge_fields(
    destination: &mut Vec<(String, Instance)>,
    incoming: Vec<(String, Instance)>,
) -> Result<(), PdfError> {
    for (name, value) in incoming {
        let Some((_, existing)) = destination
            .iter_mut()
            .find(|(candidate, _)| candidate == &name)
        else {
            destination.push((name, value));
            continue;
        };
        match value {
            Instance::Repeated(mut values) => match existing {
                Instance::Repeated(existing) => existing.append(&mut values),
                _ => return Err(incompatible_output(&name)),
            },
            replacement @ Instance::Scalar(_) => match existing {
                Instance::Scalar(Value::Null) => *existing = replacement,
                Instance::Scalar(_) => {
                    // A non-repeating capture keeps the first value across selected pages.
                }
                _ => return Err(incompatible_output(&name)),
            },
            _ => return Err(incompatible_output(&name)),
        }
    }
    Ok(())
}

fn incompatible_output(name: &str) -> PdfError {
    PdfError::InvalidLayout(format!(
        "PDF output `{name}` produced incompatible repeated and singleton values"
    ))
}

#[derive(Default)]
struct OutputBudget {
    nodes: usize,
    value_bytes: usize,
}

impl OutputBudget {
    fn node(&mut self) -> Result<(), PdfError> {
        self.nodes(1)
    }

    fn nodes(&mut self, count: usize) -> Result<(), PdfError> {
        self.nodes = self
            .nodes
            .checked_add(count)
            .ok_or(PdfError::TooManyOutputNodes)?;
        if self.nodes > MAX_OUTPUT_NODES {
            return Err(PdfError::TooManyOutputNodes);
        }
        Ok(())
    }

    fn value_bytes(&mut self, count: usize, path: &str) -> Result<(), PdfError> {
        self.value_bytes = self
            .value_bytes
            .checked_add(count)
            .ok_or_else(|| PdfError::ValueTooLarge(path.to_owned()))?;
        if self.value_bytes > MAX_VALUE_BYTES {
            return Err(PdfError::ValueTooLarge(path.to_owned()));
        }
        Ok(())
    }

    fn depth(&self, depth: usize) -> Result<(), PdfError> {
        if depth > MAX_INSTANCE_DEPTH {
            return Err(PdfError::InstanceTooDeep);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use ir::{Instance, Value};
    use mapping::{
        PdfCapture, PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout,
        PdfMerge, PdfMergeSource, PdfPageSelection, PdfPages, PdfReference, PdfRegion,
    };

    use super::evaluate;
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
                glyph("Long", 10.0, 5.0, 34.0, 15.0),
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
            Some(&Value::String("Long title".into()))
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
                    })],
                }),
                PdfCommand::Merge(PdfMerge {
                    name: "Table".into(),
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
}
