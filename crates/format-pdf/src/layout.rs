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
    budget.node()?;
    for page in pages
        .iter()
        .filter(|page| layout.page_selection().includes(page.number))
    {
        let mut anchors = BTreeMap::new();
        let page_fields = evaluate_commands(
            layout.commands(),
            page,
            page.bounds,
            &mut anchors,
            &mut budget,
            1,
        )?;
        merge_fields(&mut fields, page_fields)?;
    }
    Ok(Instance::Group(fields))
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
                let mut row_fields = Vec::new();
                for row in row_regions(page, region, rows.find, rows.minimum_extent) {
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
) -> Vec<Rect> {
    let levels = edge_levels(page, region, find);
    if levels.len() >= 2 {
        return levels
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
    text_row_regions(page, region, minimum_extent)
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

fn text_row_regions(page: &Page, region: Rect, minimum_extent: Option<f64>) -> Vec<Rect> {
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .collect::<Vec<_>>();
    glyphs.sort_by(|left, right| vertical_center(left).total_cmp(&vertical_center(right)));
    let mut lines: Vec<(f64, f64)> = Vec::new();
    for glyph in glyphs {
        let center = vertical_center(glyph);
        let height = (glyph.bounds.bottom - glyph.bounds.top).max(1.0);
        match lines.last_mut() {
            Some((top, bottom)) if (center - (*top + *bottom) / 2.0).abs() <= height * 0.5 => {
                *top = top.min(glyph.bounds.top);
                *bottom = bottom.max(glyph.bounds.bottom);
            }
            _ => lines.push((glyph.bounds.top, glyph.bounds.bottom)),
        }
    }
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, &(line_top, line_bottom))| {
            let top = index
                .checked_sub(1)
                .map_or(region.top, |previous| (lines[previous].1 + line_top) / 2.0)
                .max(region.top);
            let bottom = lines
                .get(index + 1)
                .map_or(region.bottom, |next| (line_bottom + next.0) / 2.0)
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
    use ir::{Instance, Value};
    use mapping::{
        PdfCapture, PdfCommand, PdfCoordinate, PdfEdgeFind, PdfEdgeRows, PdfGroup, PdfLayout,
        PdfPageSelection, PdfReference, PdfRegion,
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
            horizontal_edges: [10.0, 50.0, 70.0, 110.0, 130.0]
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
        let layout = PdfLayout::new(
            "Document",
            PdfPageSelection::All,
            vec![PdfCommand::EdgeRows(PdfEdgeRows {
                region: PdfRegion::full(),
                find: PdfEdgeFind {
                    fill: 2.0,
                    prominence: 100.0,
                },
                minimum_extent: Some(30.0),
                children: vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Row".into(),
                    region: PdfRegion::full(),
                    children: vec![capture("Name", 0.0, 100.0), capture("Count", 100.0, 200.0)],
                })],
            })],
        )
        .unwrap();
        let instance = evaluate(&[page], &layout).unwrap();
        let rows = instance
            .field("Row")
            .and_then(Instance::as_repeated)
            .unwrap();
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
}
