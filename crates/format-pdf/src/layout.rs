use std::collections::BTreeMap;

use ir::{Instance, Value};
use mapping::{
    PdfCommand, PdfCoordinate, PdfEdgeFind, PdfLayout, PdfMerge, PdfMergeComposition, PdfReference,
    PdfRegion, PdfTextCase, PdfTextGroupOutput, PdfTextGroups, PdfTextProperties,
};

use crate::extract::{Glyph, Page, Rect};
use crate::{MAX_EVENTS, MAX_INSTANCE_DEPTH, MAX_OUTPUT_NODES, MAX_VALUE_BYTES, PdfError};

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
                evaluate_merge(merge, pages, selections, fields, budget)?;
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

fn evaluate_merge(
    merge: &PdfMerge,
    pages: &[Page],
    selections: &mut Vec<mapping::PdfPageSelection>,
    fields: &mut Vec<(String, Instance)>,
    budget: &mut OutputBudget,
) -> Result<(), PdfError> {
    match merge.composition {
        PdfMergeComposition::Independent => {
            for source in &merge.sources {
                selections.push(source.page_selection);
                for page in pages
                    .iter()
                    .filter(|page| page_is_selected(page.number, selections))
                {
                    let mut anchors = BTreeMap::new();
                    let region = resolve_region(&source.region, page.bounds, &anchors)?;
                    let produced =
                        evaluate_commands(&merge.children, page, region, &mut anchors, budget, 1)?;
                    merge_fields(fields, produced)?;
                }
                selections.pop();
            }
        }
        PdfMergeComposition::VerticalCollage => {
            let mut strips = Vec::new();
            for source in &merge.sources {
                selections.push(source.page_selection);
                for page in pages
                    .iter()
                    .filter(|page| page_is_selected(page.number, selections))
                {
                    let region = resolve_region(&source.region, page.bounds, &BTreeMap::new())?;
                    strips.push((page, region));
                }
                selections.pop();
            }
            if let Some(collage) = vertical_collage(strips)? {
                let mut anchors = BTreeMap::new();
                let bounds = collage.bounds;
                let produced =
                    evaluate_commands(&merge.children, &collage, bounds, &mut anchors, budget, 1)?;
                merge_fields(fields, produced)?;
            }
        }
    }
    Ok(())
}

fn page_is_selected(page: u32, selections: &[mapping::PdfPageSelection]) -> bool {
    selections.iter().all(|selection| selection.includes(page))
}

fn vertical_collage(strips: Vec<(&Page, Rect)>) -> Result<Option<Page>, PdfError> {
    let Some((first_page, _)) = strips.first() else {
        return Ok(None);
    };
    let mut collage = Page {
        number: first_page.number,
        bounds: Rect {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
        glyphs: Vec::new(),
        horizontal_edges: Vec::new(),
    };
    let mut text_bytes = 0usize;

    for (page, source) in strips {
        let width = source.right - source.left;
        let height = source.bottom - source.top;
        let logical_top = collage.bounds.bottom;
        let logical_bottom = logical_top + height;
        if !width.is_finite() || !logical_bottom.is_finite() {
            return Err(PdfError::InvalidLayout(
                "PDF vertical collage resolved to a non-finite extent".into(),
            ));
        }
        collage.bounds.right = collage.bounds.right.max(width);
        collage.bounds.bottom = logical_bottom;

        for glyph in &page.glyphs {
            let Some(clipped) = intersect_rect(glyph.bounds, source) else {
                continue;
            };
            if collage
                .glyphs
                .len()
                .checked_add(collage.horizontal_edges.len())
                .is_none_or(|count| count >= MAX_EVENTS)
            {
                return Err(PdfError::TooManyEvents);
            }
            text_bytes = text_bytes
                .checked_add(glyph.text.len())
                .ok_or(PdfError::DecodedTextTooLarge)?;
            if text_bytes > MAX_VALUE_BYTES {
                return Err(PdfError::DecodedTextTooLarge);
            }
            collage.glyphs.push(Glyph {
                text: glyph.text.clone(),
                bounds: translate_rect(clipped, -source.left, logical_top - source.top),
                font_face: glyph.font_face.clone(),
                cell_height: glyph.cell_height,
                baseline_angle: glyph.baseline_angle,
            });
        }
        for edge in &page.horizontal_edges {
            if edge.y < source.top || edge.y > source.bottom {
                continue;
            }
            let left = edge.left.max(source.left);
            let right = edge.right.min(source.right);
            if right - left <= EMPTY_EPSILON {
                continue;
            }
            if collage
                .glyphs
                .len()
                .checked_add(collage.horizontal_edges.len())
                .is_none_or(|count| count >= MAX_EVENTS)
            {
                return Err(PdfError::TooManyEvents);
            }
            collage
                .horizontal_edges
                .push(crate::extract::HorizontalEdge {
                    left: left - source.left,
                    right: right - source.left,
                    y: edge.y - source.top + logical_top,
                });
        }
    }
    Ok(Some(collage))
}

fn intersect_rect(left: Rect, right: Rect) -> Option<Rect> {
    let intersection = Rect {
        left: left.left.max(right.left),
        top: left.top.max(right.top),
        right: left.right.min(right.right),
        bottom: left.bottom.min(right.bottom),
    };
    (intersection.right - intersection.left > EMPTY_EPSILON
        && intersection.bottom - intersection.top > EMPTY_EPSILON)
        .then_some(intersection)
}

fn translate_rect(rect: Rect, x: f64, y: f64) -> Rect {
    Rect {
        left: rect.left + x,
        top: rect.top + y,
        right: rect.right + x,
        bottom: rect.bottom + y,
    }
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
                    budget,
                )? {
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
            PdfCommand::TextGroups(groups) => {
                let region = resolve_region(&groups.region, current, anchors)?;
                evaluate_text_groups(groups, page, region, anchors, &mut fields, budget, depth)?;
            }
            PdfCommand::TextRows(rows) => {
                let region = resolve_region(&rows.region, current, anchors)?;
                let mut row_fields = Vec::new();
                for row in text_row_regions(page, region, rows.minimum_extent, None, budget)? {
                    budget.work()?;
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
                    if !produced
                        .iter()
                        .any(|(_, instance)| instance_has_content(instance))
                    {
                        continue;
                    }
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

fn evaluate_text_groups(
    groups: &PdfTextGroups,
    page: &Page,
    region: Rect,
    anchors: &BTreeMap<String, f64>,
    fields: &mut Vec<(String, Instance)>,
    budget: &mut OutputBudget,
    depth: usize,
) -> Result<(), PdfError> {
    let prepared = groups
        .groups
        .iter()
        .map(|group| {
            let needle = normalize_literal(
                &group.matcher.needle,
                group.matcher.case,
                group.matcher.flexible_whitespace,
                "text-group marker",
                budget,
            )?;
            if needle.is_empty() {
                return Err(PdfError::InvalidLayout(
                    "PDF text-group marker normalized to an empty string".into(),
                ));
            }
            Ok((
                group.matcher.case,
                group.matcher.flexible_whitespace,
                needle,
                &group.matcher.properties,
            ))
        })
        .collect::<Result<Vec<_>, PdfError>>()?;

    let mut markers = Vec::new();
    for line in marker_lines(page, region, budget)? {
        let mut normalized: [Option<String>; 4] = Default::default();
        for (group_index, (case, flexible_whitespace, needle, properties)) in
            prepared.iter().enumerate()
        {
            budget.work()?;
            let key = normalization_key(*case, *flexible_whitespace);
            let property_text;
            let property_haystack;
            let haystack = if properties.is_empty() {
                match &normalized[key] {
                    Some(value) => value,
                    None => {
                        let value = normalize_literal(
                            &line.text,
                            *case,
                            *flexible_whitespace,
                            "text-group visual line",
                            budget,
                        )?;
                        normalized[key].insert(value)
                    }
                }
            } else {
                property_text = render_property_line(&line.glyphs, properties, budget)?;
                property_haystack = normalize_literal(
                    &property_text,
                    *case,
                    *flexible_whitespace,
                    "text-group property line",
                    budget,
                )?;
                &property_haystack
            };
            budget.work_units(
                haystack
                    .len()
                    .checked_add(needle.len())
                    .ok_or(PdfError::TooManyEvents)?,
            )?;
            if haystack.contains(needle) {
                markers.push((group_index, line.top.max(region.top)));
                break;
            }
        }
    }

    for (index, &(group_index, top)) in markers.iter().enumerate() {
        let bottom = markers
            .get(index + 1)
            .map_or(region.bottom, |(_, next_top)| *next_top)
            .min(region.bottom);
        if bottom - top <= EMPTY_EPSILON {
            continue;
        }
        let candidate = Rect {
            left: region.left,
            top,
            right: region.right,
            bottom,
        };
        let group = &groups.groups[group_index];
        let child_depth = match &group.output {
            PdfTextGroupOutput::Flatten => depth + 1,
            PdfTextGroupOutput::Repeated { .. } => depth + 2,
        };
        let mut child_anchors = anchors.clone();
        let produced = match evaluate_commands(
            &group.children,
            page,
            candidate,
            &mut child_anchors,
            budget,
            child_depth,
        ) {
            Ok(produced) => produced,
            Err(PdfError::InvalidCandidateRegion) => continue,
            Err(error) => return Err(error),
        };
        match &group.output {
            PdfTextGroupOutput::Flatten => merge_fields(fields, produced)?,
            PdfTextGroupOutput::Repeated { name } => {
                budget.nodes(2)?;
                merge_fields(
                    fields,
                    vec![(
                        name.clone(),
                        Instance::Repeated(vec![Instance::Group(produced)]),
                    )],
                )?;
            }
        }
    }
    Ok(())
}

struct MarkerLine<'a> {
    top: f64,
    text: String,
    glyphs: Vec<&'a Glyph>,
}

fn marker_lines<'a>(
    page: &'a Page,
    region: Rect,
    budget: &mut OutputBudget,
) -> Result<Vec<MarkerLine<'a>>, PdfError> {
    budget.work_units(page.glyphs.len())?;
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .collect::<Vec<_>>();
    budget.sort_items(glyphs.len())?;
    glyphs.sort_by(|left, right| {
        vertical_center(left)
            .total_cmp(&vertical_center(right))
            .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
    });

    let mut lines: Vec<Vec<&Glyph>> = Vec::new();
    budget.work_units(glyphs.len())?;
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

    lines
        .into_iter()
        .map(|mut glyphs| {
            budget.sort_items(glyphs.len())?;
            glyphs.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
            budget.work_units(glyphs.len())?;
            let top = glyphs
                .iter()
                .map(|glyph| glyph.bounds.top)
                .fold(f64::INFINITY, f64::min);
            let text = render_visual_line(&glyphs, "text-group visual line", budget)?;
            Ok(MarkerLine { top, text, glyphs })
        })
        .collect()
}

fn render_property_line(
    glyphs: &[&Glyph],
    properties: &PdfTextProperties,
    budget: &mut OutputBudget,
) -> Result<String, PdfError> {
    budget.work_units(glyphs.len())?;
    let mut value = String::new();
    let mut previous_right = None;
    for glyph in glyphs {
        if !glyph_matches_properties(glyph, properties) {
            value.push('\0');
            previous_right = None;
            continue;
        }
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
            return Err(PdfError::ValueTooLarge("text-group property line".into()));
        }
    }
    Ok(value)
}

fn glyph_matches_properties(glyph: &Glyph, properties: &PdfTextProperties) -> bool {
    if let Some(expected) = properties.font_face.as_deref()
        && glyph
            .font_face
            .as_deref()
            .map(canonical_font_face)
            .is_none_or(|actual| actual != canonical_font_face(expected))
    {
        return false;
    }
    if let Some(expected) = properties.cell_height
        && (glyph.cell_height - expected.value).abs() > expected.deviation + EMPTY_EPSILON
    {
        return false;
    }
    if let Some(expected) = properties.baseline_angle
        && angle_distance(glyph.baseline_angle, expected.value) > expected.deviation + EMPTY_EPSILON
    {
        return false;
    }
    true
}

fn canonical_font_face(face: &str) -> &str {
    face.split_once('+').map_or(face, |(prefix, name)| {
        if prefix.len() == 6 && prefix.bytes().all(|byte| byte.is_ascii_uppercase()) {
            name
        } else {
            face
        }
    })
}

fn angle_distance(left: f64, right: f64) -> f64 {
    let difference = (left - right).rem_euclid(360.0);
    difference.min(360.0 - difference)
}

fn render_visual_line(
    glyphs: &[&Glyph],
    path: &str,
    budget: &mut OutputBudget,
) -> Result<String, PdfError> {
    budget.work_units(glyphs.len())?;
    let mut value = String::new();
    let mut previous_right = None;
    for glyph in glyphs {
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
    Ok(value)
}

fn normalization_key(case: PdfTextCase, flexible_whitespace: bool) -> usize {
    usize::from(matches!(case, PdfTextCase::AsciiInsensitive)) * 2
        + usize::from(flexible_whitespace)
}

fn normalize_literal(
    value: &str,
    case: PdfTextCase,
    flexible_whitespace: bool,
    path: &str,
    budget: &mut OutputBudget,
) -> Result<String, PdfError> {
    budget.work_units(value.len())?;
    let mut normalized = String::with_capacity(value.len());
    let mut whitespace = false;
    for mut character in value.chars() {
        if flexible_whitespace && character.is_whitespace() {
            whitespace = !normalized.is_empty();
            continue;
        }
        if whitespace {
            normalized.push(' ');
            whitespace = false;
        }
        if matches!(case, PdfTextCase::AsciiInsensitive) {
            character.make_ascii_lowercase();
        }
        normalized.push(character);
        if normalized.len() > MAX_VALUE_BYTES {
            return Err(PdfError::ValueTooLarge(path.to_owned()));
        }
    }
    Ok(normalized)
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
    budget: &mut OutputBudget,
) -> Result<Vec<Rect>, PdfError> {
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
        return Ok(boundaries
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
            .collect());
    }
    text_row_regions(page, region, minimum_extent, fallback_anchor, budget)
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
    budget: &mut OutputBudget,
) -> Result<Vec<Rect>, PdfError> {
    budget.work_units(page.glyphs.len())?;
    let mut glyphs = page
        .glyphs
        .iter()
        .filter(|glyph| glyph_in_region(glyph, region))
        .collect::<Vec<_>>();
    budget.sort_items(glyphs.len())?;
    glyphs.sort_by(|left, right| vertical_center(left).total_cmp(&vertical_center(right)));
    let mut lines: Vec<TextLine> = Vec::new();
    budget.work_units(glyphs.len())?;
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

    budget.work_units(lines.len())?;
    let anchors = lines
        .iter()
        .filter(|line| line.anchored)
        .collect::<Vec<_>>();
    let row_lines = if anchors.len() >= 2 {
        anchors
    } else {
        lines.iter().collect::<Vec<_>>()
    };

    budget.work_units(row_lines.len())?;
    Ok(row_lines
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
        .collect())
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
        if line_index > 0 {
            value.truncate(value.trim_end_matches(char::is_whitespace).len());
            value.push('\n');
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

fn instance_has_content(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(Value::Null) => false,
        Instance::Scalar(_) => true,
        Instance::Group(fields) => fields.iter().any(|(_, child)| instance_has_content(child)),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            items.iter().any(instance_has_content)
        }
        Instance::DocumentSet(_) => true,
    }
}

#[derive(Default)]
struct OutputBudget {
    nodes: usize,
    value_bytes: usize,
    work: usize,
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

    fn work(&mut self) -> Result<(), PdfError> {
        self.work_units(1)
    }

    fn work_units(&mut self, count: usize) -> Result<(), PdfError> {
        self.work = self
            .work
            .checked_add(count)
            .ok_or(PdfError::TooManyEvents)?;
        if self.work > MAX_EVENTS {
            return Err(PdfError::TooManyEvents);
        }
        Ok(())
    }

    fn sort_items(&mut self, count: usize) -> Result<(), PdfError> {
        if count < 2 {
            return Ok(());
        }
        let passes = usize::BITS as usize - (count - 1).leading_zeros() as usize;
        self.work_units(count.checked_mul(passes).ok_or(PdfError::TooManyEvents)?)
    }

    fn depth(&self, depth: usize) -> Result<(), PdfError> {
        if depth > MAX_INSTANCE_DEPTH {
            return Err(PdfError::InstanceTooDeep);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
