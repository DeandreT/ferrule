//! View state for large source and target endpoint nodes.

#[cfg(test)]
use crate::test_ui::ContextExt as _;

use std::collections::BTreeMap;
use std::ops::Range;

use crate::canvas::CanvasNode;
use crate::canvas_keyboard::focus;

pub const VISIBLE_PIN_LIMIT: usize = 12;
const MIN_VISIBLE_PINS: usize = 4;
const PIN_ROW_HEIGHT: f32 = 20.0;
const MIN_WIDTH: f32 = 110.0;
const MAX_WIDTH: f32 = 480.0;
/// How far one arrow key moves the width handle.
const KEYBOARD_WIDTH_STEP: f32 = 10.0;
/// Keys a focused endpoint scrollbar takes.
const SCROLL_KEYS: [egui::Key; 6] = [
    egui::Key::ArrowUp,
    egui::Key::ArrowDown,
    egui::Key::PageUp,
    egui::Key::PageDown,
    egui::Key::Home,
    egui::Key::End,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointDisplayPin {
    HiddenBefore,
    Visible(usize),
    HiddenAfter,
}

#[derive(Default)]
pub struct EndpointScrollState {
    offsets: BTreeMap<CanvasNode, usize>,
    visible_rows: BTreeMap<CanvasNode, usize>,
    widths: BTreeMap<CanvasNode, f32>,
    resize_remainders: BTreeMap<CanvasNode, f32>,
    row_heights: BTreeMap<CanvasNode, f32>,
    field_rects: BTreeMap<CanvasNode, egui::Rect>,
    filters: BTreeMap<CanvasNode, EndpointFilter>,
    measurements: BTreeMap<CanvasNode, EndpointMeasurement>,
}

#[derive(Default)]
struct EndpointFilter {
    /// Full schema length, used to invalidate stale offsets and measurements.
    total: usize,
    /// Original semantic indices, ordered by their position in the schema.
    connected: Vec<usize>,
    connected_only: bool,
    /// A search match may temporarily reveal one otherwise filtered-out field.
    revealed: Option<usize>,
}

#[derive(Default)]
struct EndpointMeasurement {
    header: Option<egui::Rect>,
    footer: Option<egui::Rect>,
    rows: Vec<egui::Rect>,
}

impl EndpointScrollState {
    /// Update from canonical connections, not the currently displayed proxy pins.
    /// This is editor view state only: no schema, mapping or wire is changed here.
    pub fn set_connected(
        &mut self,
        node: CanvasNode,
        total: usize,
        connected: impl IntoIterator<Item = usize>,
    ) -> bool {
        let mut connected = connected
            .into_iter()
            .filter(|&semantic| semantic < total)
            .collect::<Vec<_>>();
        connected.sort_unstable();
        connected.dedup();
        let filter = self.filters.entry(node).or_default();
        let old_revealed = filter.revealed;
        filter.revealed = filter
            .revealed
            .filter(|&semantic| semantic < total && connected.binary_search(&semantic).is_err());
        let changed = filter.total != total
            || filter.connected != connected
            || filter.revealed != old_revealed;
        filter.total = total;
        filter.connected = connected;
        if changed {
            self.clamp_offset(node, total);
            self.resize_remainders.remove(&node);
            self.field_rects.remove(&node);
        }
        changed
    }

    pub fn connected_count(&self, node: CanvasNode, total: usize) -> usize {
        self.filters.get(&node).map_or(0, |filter| {
            filter
                .connected
                .partition_point(|&semantic| semantic < total)
        })
    }

    pub fn connected_only(&self, node: CanvasNode) -> bool {
        self.filters
            .get(&node)
            .is_some_and(|filter| filter.connected_only)
    }

    pub fn set_connected_only(&mut self, node: CanvasNode, connected_only: bool) -> bool {
        let filter = self.filters.entry(node).or_default();
        if filter.connected_only == connected_only {
            return false;
        }
        filter.connected_only = connected_only;
        filter.revealed = None;
        self.offsets.remove(&node);
        self.resize_remainders.remove(&node);
        self.field_rects.remove(&node);
        true
    }

    pub fn clear_reveal(&mut self, node: CanvasNode, total: usize) -> bool {
        let changed = self
            .filters
            .get_mut(&node)
            .is_some_and(|filter| filter.revealed.take().is_some());
        if changed {
            self.clamp_offset(node, total);
            self.field_rects.remove(&node);
        }
        changed
    }

    /// Number of eligible rows before scrolling (including a revealed search match).
    pub fn eligible_len(&self, node: CanvasNode, total: usize) -> usize {
        self.filtered_indices(node, total)
            .map_or(total, |(connected, revealed)| {
                connected.len() + usize::from(revealed.is_some())
            })
    }

    /// The complete eligible list in original schema order, never visual pin IDs.
    #[cfg(test)]
    pub fn eligible_indices(&self, node: CanvasNode, total: usize) -> Vec<usize> {
        (0..self.eligible_len(node, total))
            .filter_map(|row| self.semantic_at(node, total, row))
            .collect()
    }

    pub fn visible_limit(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_rows
            .get(&node)
            .copied()
            .unwrap_or(VISIBLE_PIN_LIMIT)
            .min(self.eligible_len(node, total))
    }

    pub fn offset(&self, node: CanvasNode, total: usize) -> usize {
        self.offsets
            .get(&node)
            .copied()
            .unwrap_or(0)
            .min(self.max_offset(node, total))
    }

    /// Row range within the eligible list. Only in All fields mode do these
    /// numbers also happen to be semantic indices. Use `visible_indices` or
    /// `semantic_pin` to resolve non-contiguous Connected fields rows.
    pub fn visible_range(&self, node: CanvasNode, total: usize) -> Range<usize> {
        let start = self.offset(node, total);
        start..start.saturating_add(self.visible_limit(node, total))
    }

    pub fn visible_indices(&self, node: CanvasNode, total: usize) -> Vec<usize> {
        self.visible_range(node, total)
            .filter_map(|row| self.semantic_at(node, total, row))
            .collect()
    }

    pub fn max_offset(&self, node: CanvasNode, total: usize) -> usize {
        self.eligible_len(node, total)
            .saturating_sub(self.visible_limit(node, total))
    }

    pub fn visible_len(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_range(node, total).len()
    }

    pub fn display_pin_count(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_len(node, total)
            + usize::from(self.hidden_before(node, total) > 0)
            + usize::from(self.hidden_after(node, total) > 0)
    }

    pub fn display_pin(
        &self,
        node: CanvasNode,
        displayed_pin: usize,
        total: usize,
    ) -> Option<EndpointDisplayPin> {
        let before = self.hidden_before(node, total);
        let has_before = before > 0;
        if has_before && displayed_pin == 0 {
            return Some(EndpointDisplayPin::HiddenBefore);
        }

        let visible = self.visible_range(node, total);
        let visible_pin = displayed_pin.checked_sub(usize::from(has_before))?;
        if visible_pin < visible.len() {
            return self
                .semantic_at(node, total, visible.start + visible_pin)
                .map(EndpointDisplayPin::Visible);
        }

        (self.hidden_after(node, total) > 0 && visible_pin == visible.len())
            .then_some(EndpointDisplayPin::HiddenAfter)
    }

    pub fn semantic_pin(
        &self,
        node: CanvasNode,
        displayed_pin: usize,
        total: usize,
    ) -> Option<usize> {
        match self.display_pin(node, displayed_pin, total)? {
            EndpointDisplayPin::Visible(semantic) => Some(semantic),
            EndpointDisplayPin::HiddenBefore | EndpointDisplayPin::HiddenAfter => None,
        }
    }

    pub fn visual_pin_for_semantic(
        &self,
        node: CanvasNode,
        semantic_pin: usize,
        total: usize,
    ) -> Option<usize> {
        let row = self.eligible_position(node, total, semantic_pin)?;
        let range = self.visible_range(node, total);
        let has_before = self.hidden_before(node, total) > 0;
        if range.contains(&row) {
            Some(row - range.start + usize::from(has_before))
        } else if row < range.start && has_before {
            Some(0)
        } else if row >= range.end && self.hidden_after(node, total) > 0 {
            Some(range.len() + usize::from(has_before))
        } else {
            None
        }
    }

    pub fn hidden_before(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_range(node, total).start
    }

    pub fn hidden_after(&self, node: CanvasNode, total: usize) -> usize {
        self.eligible_len(node, total)
            .saturating_sub(self.visible_range(node, total).end)
    }

    pub fn scroll_rows(&mut self, node: CanvasNode, total: usize, rows: isize) -> bool {
        let old = self.offset(node, total);
        let new = old
            .saturating_add_signed(rows)
            .min(self.max_offset(node, total));
        self.set_offset(node, total, new)
    }

    pub fn reveal(&mut self, node: CanvasNode, total: usize, semantic_pin: usize) -> bool {
        if semantic_pin >= total {
            return false;
        }
        let mut changed = false;
        if let Some(filter) = self.filters.get_mut(&node)
            && filter.connected_only
        {
            let revealed = filter
                .connected
                .binary_search(&semantic_pin)
                .is_err()
                .then_some(semantic_pin);
            changed = filter.revealed != revealed;
            filter.revealed = revealed;
        }
        let Some(row) = self.eligible_position(node, total, semantic_pin) else {
            return false;
        };
        if changed {
            self.clamp_offset(node, total);
            self.field_rects.remove(&node);
        }
        let range = self.visible_range(node, total);
        if range.contains(&row) {
            return changed;
        }
        let centered = row.saturating_sub(self.visible_limit(node, total) / 2);
        self.set_offset(node, total, centered) || changed
    }

    /// Graph-space bounds measured from the rendered field list, excluding the
    /// complete header (including wrapped filter controls) and footer. Call this
    /// on each geometry callback, in the same space as `final_node_rect` and Ui.
    /// Invalid or empty geometry hides controls until the next valid measurement.
    pub fn set_field_rect(&mut self, node: CanvasNode, rect: egui::Rect) {
        if finite_positive_rect(rect) {
            self.field_rects.insert(node, rect);
        } else {
            self.field_rects.remove(&node);
        }
    }

    pub fn field_rect(&self, node: CanvasNode) -> Option<egui::Rect> {
        self.field_rects.get(&node).copied()
    }

    /// Start one render pass without discarding the preceding pass's bounds,
    /// which are still needed to route wheel input before the scene is drawn.
    pub fn begin_geometry_frame(&mut self) {
        self.measurements.clear();
    }

    pub fn record_field_row(&mut self, node: CanvasNode, rect: egui::Rect) {
        if finite_positive_rect(rect) {
            self.measurements.entry(node).or_default().rows.push(rect);
        }
    }

    pub fn record_header_rect(&mut self, node: CanvasNode, rect: egui::Rect) {
        self.measurements.entry(node).or_default().header =
            finite_positive_rect(rect).then_some(rect);
    }

    pub fn record_footer_rect(&mut self, node: CanvasNode, rect: egui::Rect) {
        self.measurements.entry(node).or_default().footer =
            finite_positive_rect(rect).then_some(rect);
    }

    pub fn finish_geometry(&mut self, node: CanvasNode, node_rect: egui::Rect) {
        self.field_rects.remove(&node);
        let Some(measured) = self.measurements.remove(&node) else {
            return;
        };
        let Some(header) = measured.header else {
            return;
        };
        let Some(first) = measured.rows.first() else {
            return;
        };
        if !finite_positive_rect(node_rect) {
            return;
        }
        let rows = measured
            .rows
            .iter()
            .fold(*first, |bounds, row| bounds.union(*row));
        let top = rows.top().max(header.bottom()).max(node_rect.top());
        let bottom = rows.bottom().min(node_rect.bottom()).min(
            measured
                .footer
                .map_or(node_rect.bottom(), |footer| footer.top()),
        );
        self.set_field_rect(
            node,
            egui::Rect::from_min_max(
                egui::pos2(node_rect.left(), top),
                egui::pos2(node_rect.right(), bottom),
            ),
        );
    }

    /// Use the measured pin-row pitch for resize gestures at the active density.
    pub fn set_row_height(&mut self, node: CanvasNode, height: f32) {
        if height.is_finite() && height > 0.0 {
            if self.row_heights.get(&node) != Some(&height) {
                self.resize_remainders.remove(&node);
            }
            self.row_heights.insert(node, height);
        } else {
            self.row_heights.remove(&node);
            self.resize_remainders.remove(&node);
        }
    }

    pub fn width(&self, node: CanvasNode, natural_width: f32) -> f32 {
        let natural_width = if natural_width.is_finite() {
            natural_width
        } else {
            MIN_WIDTH
        };
        self.widths
            .get(&node)
            .copied()
            .unwrap_or(natural_width)
            .clamp(MIN_WIDTH, MAX_WIDTH)
    }

    pub fn resize(
        &mut self,
        node: CanvasNode,
        total: usize,
        natural_width: f32,
        delta: egui::Vec2,
    ) -> bool {
        let mut changed = false;
        if delta.x.is_finite() && delta.x != 0.0 {
            let old = self.width(node, natural_width);
            let width = (old + delta.x).clamp(MIN_WIDTH, MAX_WIDTH);
            self.widths.insert(node, width);
            changed |= width != old;
        }
        let eligible = self.eligible_len(node, total);
        if delta.y.is_finite() && delta.y != 0.0 && eligible > 0 {
            let row_height = self
                .row_heights
                .get(&node)
                .copied()
                .unwrap_or(PIN_ROW_HEIGHT);
            let remainder = self.resize_remainders.entry(node).or_default();
            // Even finite f32 inputs can overflow during accumulation or when
            // divided by a tiny measured pitch. f64 covers the entire f32
            // range, and modulo keeps only a bounded partial-row remainder.
            let accumulated = f64::from(*remainder) + f64::from(delta.y);
            let pitch = f64::from(row_height);
            let steps = (accumulated / pitch).trunc() as isize;
            *remainder = (accumulated % pitch) as f32;
            changed |= self.step_visible_rows(node, total, steps);
        } else if eligible == 0 {
            self.resize_remainders.remove(&node);
        }
        if changed {
            self.field_rects.remove(&node);
        }
        changed
    }

    /// Shows `rows` more (or fewer) fields, as the height handle does from
    /// the keyboard. Any partial row from a pointer drag is dropped.
    pub fn resize_rows(&mut self, node: CanvasNode, total: usize, rows: isize) -> bool {
        if rows == 0 {
            return false;
        }
        self.resize_remainders.remove(&node);
        let changed = self.step_visible_rows(node, total, rows);
        if changed {
            self.field_rects.remove(&node);
        }
        changed
    }

    fn step_visible_rows(&mut self, node: CanvasNode, total: usize, steps: isize) -> bool {
        let eligible = self.eligible_len(node, total);
        if steps == 0 || eligible == 0 {
            return false;
        }
        let old = self.visible_limit(node, total);
        let minimum = MIN_VISIBLE_PINS.min(eligible);
        let rows = old.saturating_add_signed(steps).clamp(minimum, eligible);
        // A short filtered list cannot be resized vertically. Do not replace
        // the saved All-fields capacity with its clipped size.
        if rows == old {
            return false;
        }
        self.visible_rows.insert(node, rows);
        self.clamp_offset(node, total);
        true
    }

    fn set_offset(&mut self, node: CanvasNode, total: usize, offset: usize) -> bool {
        let old = self.offset(node, total);
        let offset = offset.min(self.max_offset(node, total));
        if offset == 0 {
            self.offsets.remove(&node);
        } else {
            self.offsets.insert(node, offset);
        }
        if old != offset {
            self.field_rects.remove(&node);
        }
        old != offset
    }

    fn clamp_offset(&mut self, node: CanvasNode, total: usize) {
        let maximum = self.max_offset(node, total);
        if let Some(offset) = self.offsets.get_mut(&node) {
            *offset = (*offset).min(maximum);
        }
    }

    fn filtered_indices(
        &self,
        node: CanvasNode,
        total: usize,
    ) -> Option<(&[usize], Option<usize>)> {
        let filter = self
            .filters
            .get(&node)
            .filter(|filter| filter.connected_only)?;
        let end = filter
            .connected
            .partition_point(|&semantic| semantic < total);
        let connected = &filter.connected[..end];
        let revealed = filter
            .revealed
            .filter(|&semantic| semantic < total && connected.binary_search(&semantic).is_err());
        Some((connected, revealed))
    }

    fn semantic_at(&self, node: CanvasNode, total: usize, row: usize) -> Option<usize> {
        let Some((connected, revealed)) = self.filtered_indices(node, total) else {
            return (row < total).then_some(row);
        };
        let Some(revealed) = revealed else {
            return connected.get(row).copied();
        };
        let insertion = connected.partition_point(|&semantic| semantic < revealed);
        if row == insertion {
            Some(revealed)
        } else {
            connected.get(row - usize::from(row > insertion)).copied()
        }
    }

    fn eligible_position(&self, node: CanvasNode, total: usize, semantic: usize) -> Option<usize> {
        if semantic >= total {
            return None;
        }
        let Some((connected, revealed)) = self.filtered_indices(node, total) else {
            return Some(semantic);
        };
        match connected.binary_search(&semantic) {
            Ok(row) => Some(row + usize::from(revealed.is_some_and(|pin| pin < semantic))),
            Err(row) if revealed == Some(semantic) => Some(row),
            Err(_) => None,
        }
    }
}

pub fn scroll_rows(delta_y: f32) -> isize {
    if !delta_y.is_finite() || delta_y == 0.0 {
        return 0;
    }
    let rows = ((-delta_y / 18.0).round() as isize).clamp(-5, 5);
    if rows == 0 {
        -delta_y.signum() as isize
    } else {
        rows
    }
}

pub fn show_scrollbar(
    ui: &mut egui::Ui,
    node: CanvasNode,
    node_rect: egui::Rect,
    total: usize,
    state: &mut EndpointScrollState,
    accent: egui::Color32,
    title: &str,
) -> bool {
    let Some(field_rect) = state.field_rect(node) else {
        // Never guess header/footer bounds: wait for measured node geometry.
        return false;
    };
    let eligible = state.eligible_len(node, total);
    let visible = state.visible_limit(node, total);
    let Some(geometry) = scrollbar_geometry(
        node,
        field_rect,
        node_rect,
        eligible,
        visible,
        state.offset(node, total),
    ) else {
        return false;
    };
    let painter = ui
        .painter()
        .with_clip_rect(geometry.field_rect.intersect(ui.clip_rect()));
    painter.rect_filled(
        geometry.track,
        2.0,
        ui.visuals().widgets.inactive.bg_fill.gamma_multiply(0.45),
    );
    painter.rect_filled(geometry.thumb, 2.0, accent);

    let response = ui.interact(
        geometry.hit_rect,
        ui.scope_id().with(("endpoint_scroll", node)),
        egui::Sense::click_and_drag(),
    );
    let mut changed = false;
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
        && let Some(offset) = geometry.offset_at(pointer, eligible - visible)
    {
        changed |= state.set_offset(node, total, offset);
    }
    focus::name_as(
        &response,
        egui::Role::ScrollBar,
        &format!("Scroll fields · {title}"),
    );
    if response.has_focus() {
        let page = isize::try_from(visible).unwrap_or(isize::MAX);
        let offset = state.offset(node, total);
        let maximum = state.max_offset(node, total);
        let target = match focus::take_key(ui.ctx(), response.id, &SCROLL_KEYS) {
            Some(egui::Key::ArrowUp) => Some(offset.saturating_sub(1)),
            Some(egui::Key::ArrowDown) => Some(offset.saturating_add(1)),
            Some(egui::Key::PageUp) => Some(offset.saturating_add_signed(-page)),
            Some(egui::Key::PageDown) => Some(offset.saturating_add_signed(page)),
            Some(egui::Key::Home) => Some(0),
            Some(egui::Key::End) => Some(maximum),
            _ => None,
        };
        if let Some(target) = target {
            changed |= state.set_offset(node, total, target);
        }
        focus::paint_focus(ui, geometry.hit_rect, 2.0);
    }
    let range = state.visible_range(node, total);
    let hint = if state.connected_only(node) {
        format!(
            "Rows {}-{} of {eligible} shown fields ({total} schema fields)",
            range.start + 1,
            range.end
        )
    } else {
        format!("Fields {}-{} of {total}", range.start + 1, range.end)
    };
    response.on_hover_text(hint);
    changed
}

#[derive(Clone, Copy, Debug)]
struct ScrollbarGeometry {
    field_rect: egui::Rect,
    track: egui::Rect,
    thumb: egui::Rect,
    hit_rect: egui::Rect,
}

impl ScrollbarGeometry {
    fn offset_at(self, pointer: egui::Pos2, maximum: usize) -> Option<usize> {
        let travel = self.track.height() - self.thumb.height();
        if !pointer.x.is_finite() || !pointer.y.is_finite() || travel <= 0.0 {
            // A tiny track can be entirely thumb. Wheel and search still work;
            // inventing a nonzero travel would produce an arbitrary drag jump.
            return None;
        }
        let progress = ((f64::from(pointer.y)
            - f64::from(self.track.top())
            - f64::from(self.thumb.height()) / 2.0)
            / f64::from(travel))
        .clamp(0.0, 1.0);
        Some(((progress * maximum as f64).round() as usize).min(maximum))
    }
}

fn scrollbar_geometry(
    node: CanvasNode,
    field_rect: egui::Rect,
    node_rect: egui::Rect,
    eligible: usize,
    visible: usize,
    offset: usize,
) -> Option<ScrollbarGeometry> {
    if !finite_positive_rect(field_rect)
        || !finite_positive_rect(node_rect)
        || visible == 0
        || eligible <= visible
    {
        return None;
    }
    let field_rect = field_rect.intersect(node_rect);
    if !finite_positive_rect(field_rect) {
        return None;
    }
    let width = 5.0_f32.min(field_rect.width());
    let inset = 3.0_f32.min(field_rect.width() - width);
    let x = if matches!(node, CanvasNode::SourceBlock(_)) {
        field_rect.left() + inset
    } else {
        field_rect.right() - inset - width
    };
    let track = egui::Rect::from_min_max(
        egui::pos2(x, field_rect.top()),
        egui::pos2((x + width).min(field_rect.right()), field_rect.bottom()),
    );
    if !finite_positive_rect(track) {
        return None;
    }
    // The preferred minimum is bounded by actual available height, including
    // the 18-unit-minimum / 2-unit-track boundary that caused the Release panic.
    let height = track.height();
    let thumb_height =
        (height * (visible as f32 / eligible as f32)).clamp(18.0_f32.min(height), height);
    let travel = height - thumb_height;
    let maximum = eligible - visible;
    let progress = offset.min(maximum) as f32 / maximum as f32;
    let top = (track.top() + travel * progress)
        .min(track.bottom() - thumb_height)
        .max(track.top());
    let thumb = egui::Rect::from_min_max(
        egui::pos2(track.left(), top),
        egui::pos2(track.right(), (top + thumb_height).min(track.bottom())),
    );
    let hit_rect = track.expand2(egui::vec2(3.0, 1.0)).intersect(field_rect);
    if !finite_positive_rect(thumb) || !finite_positive_rect(hit_rect) {
        return None;
    }
    Some(ScrollbarGeometry {
        field_rect,
        track,
        thumb,
        hit_rect,
    })
}

pub fn show_resize_handles(
    ui: &mut egui::Ui,
    node: CanvasNode,
    node_rect: egui::Rect,
    total: usize,
    natural_width: f32,
    state: &mut EndpointScrollState,
    title: &str,
) -> bool {
    let Some(field_rect) = state.field_rect(node) else {
        return false;
    };
    let Some(geometry) = resize_geometry(node, field_rect, node_rect) else {
        return false;
    };

    let width_response = ui
        .interact(
            geometry.width_rect,
            ui.scope_id().with(("endpoint_resize_width", node)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("Resize endpoint width");
    let height_response = ui
        .interact(
            geometry.height_rect,
            ui.scope_id().with(("endpoint_resize_height", node)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text("Resize visible fields");
    let width_direction = if matches!(node, CanvasNode::SourceBlock(_)) {
        -1.0
    } else {
        1.0
    };
    // Arrow keys move a focused handle as a drag would, the width handle by a
    // fixed step and the height handle by whole rows.
    let mut changed = false;
    focus::name_as(
        &width_response,
        egui::Role::Splitter,
        &format!("Resize width · {title}"),
    );
    if width_response.has_focus() {
        let edge = match focus::take_key(
            ui.ctx(),
            width_response.id,
            &[egui::Key::ArrowLeft, egui::Key::ArrowRight],
        ) {
            Some(egui::Key::ArrowLeft) => -KEYBOARD_WIDTH_STEP,
            Some(egui::Key::ArrowRight) => KEYBOARD_WIDTH_STEP,
            _ => 0.0,
        };
        if edge != 0.0 {
            changed |= state.resize(
                node,
                total,
                natural_width,
                egui::vec2(edge * width_direction, 0.0),
            );
        }
        focus::paint_focus(ui, geometry.width_rect, 2.0);
    }
    focus::name_as(
        &height_response,
        egui::Role::Splitter,
        &format!("Resize visible fields · {title}"),
    );
    if height_response.has_focus() {
        let rows = match focus::take_key(
            ui.ctx(),
            height_response.id,
            &[egui::Key::ArrowUp, egui::Key::ArrowDown],
        ) {
            Some(egui::Key::ArrowUp) => -1,
            Some(egui::Key::ArrowDown) => 1,
            _ => 0,
        };
        changed |= state.resize_rows(node, total, rows);
        focus::paint_focus(ui, geometry.height_rect, 2.0);
    }
    let corner_delta = if let Some(corner_rect) = geometry.corner_rect {
        // The corner combines the two handles above, which already cover it
        // from the keyboard, so it is not a Tab stop.
        let response = ui
            .interact(
                corner_rect,
                ui.scope_id().with(("endpoint_resize_corner", node)),
                egui::Sense::DRAG,
            )
            .on_hover_cursor(egui::CursorIcon::ResizeNwSe)
            .on_hover_text("Resize endpoint");
        let painter = ui
            .painter()
            .with_clip_rect(corner_rect.intersect(ui.clip_rect()));
        let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.inactive.fg_stroke.color);
        let corner = node_rect.right_bottom();
        for inset in [4.0, 8.0, 12.0] {
            painter.line_segment(
                [
                    corner + egui::vec2(-inset, -3.0),
                    corner + egui::vec2(-3.0, -inset),
                ],
                stroke,
            );
        }
        response.drag_delta()
    } else {
        egui::Vec2::ZERO
    };

    let width_delta = width_response.drag_delta().x * width_direction + corner_delta.x;
    let height_delta = height_response.drag_delta().y + corner_delta.y;
    state.resize(
        node,
        total,
        natural_width,
        egui::vec2(width_delta, height_delta),
    ) || changed
}

#[derive(Clone, Copy, Debug)]
struct ResizeGeometry {
    width_rect: egui::Rect,
    height_rect: egui::Rect,
    corner_rect: Option<egui::Rect>,
}

fn resize_geometry(
    node: CanvasNode,
    field_rect: egui::Rect,
    node_rect: egui::Rect,
) -> Option<ResizeGeometry> {
    if !finite_positive_rect(node_rect) || !finite_positive_rect(field_rect) {
        return None;
    }
    let field_rect = field_rect.intersect(node_rect);
    if !finite_positive_rect(field_rect) {
        return None;
    }
    // Keep the width handle opposite the pins and outside the field-list
    // content, so neither wire drags nor scrollbar drags become resize gestures.
    let (width_left, width_right) = if matches!(node, CanvasNode::SourceBlock(_)) {
        (
            node_rect.left() - 4.0,
            (node_rect.left() + 4.0).min(field_rect.left()),
        )
    } else {
        (
            (node_rect.right() - 4.0).max(field_rect.right()),
            node_rect.right() + 4.0,
        )
    };
    let width_rect = egui::Rect::from_min_max(
        egui::pos2(width_left, field_rect.top()),
        egui::pos2(width_right, field_rect.bottom()),
    );
    let bottom_inset = 18.0_f32.min(node_rect.width() / 3.0);
    let height_rect = egui::Rect::from_min_max(
        egui::pos2(
            node_rect.left() + bottom_inset,
            (node_rect.bottom() - 4.0).max(field_rect.bottom()),
        ),
        egui::pos2(node_rect.right() - bottom_inset, node_rect.bottom() + 4.0),
    );
    if !finite_positive_rect(width_rect) || !finite_positive_rect(height_rect) {
        return None;
    }
    let corner = node_rect.right_bottom();
    let footer_height = node_rect.bottom() - field_rect.bottom();
    let corner_rect = egui::Rect::from_min_max(
        corner - egui::vec2(16.0_f32.min(node_rect.width()), 16.0_f32.min(footer_height)),
        corner,
    );
    Some(ResizeGeometry {
        width_rect,
        height_rect,
        corner_rect: finite_positive_rect(corner_rect).then_some(corner_rect),
    })
}

fn finite_positive_rect(rect: egui::Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.x.is_finite()
        && rect.max.y.is_finite()
        && rect.width().is_finite()
        && rect.height().is_finite()
        && rect.is_positive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_pins_keep_semantic_identity_while_scrolling() {
        let node = CanvasNode::SourceBlock(2);
        let mut state = EndpointScrollState::default();

        assert_eq!(state.visible_range(node, 30), 0..12);
        assert!(state.scroll_rows(node, 30, 7));
        assert_eq!(state.visible_range(node, 30), 7..19);
        assert_eq!(state.semantic_pin(node, 3, 30), Some(9));
        assert_eq!(state.visual_pin_for_semantic(node, 10, 30), Some(4));
        assert_eq!(state.visual_pin_for_semantic(node, 2, 30), Some(0));
    }

    #[test]
    fn displayed_endpoint_pins_include_hidden_edge_proxies() {
        let node = CanvasNode::SourceBlock(2);
        let mut state = EndpointScrollState::default();

        assert_eq!(state.display_pin_count(node, 30), 13);
        assert_eq!(
            state.display_pin(node, 0, 30),
            Some(EndpointDisplayPin::Visible(0))
        );
        assert_eq!(
            state.display_pin(node, 12, 30),
            Some(EndpointDisplayPin::HiddenAfter)
        );
        assert_eq!(state.visual_pin_for_semantic(node, 18, 30), Some(12));

        assert!(state.scroll_rows(node, 30, 7));
        assert_eq!(state.display_pin_count(node, 30), 14);
        assert_eq!(
            state.display_pin(node, 0, 30),
            Some(EndpointDisplayPin::HiddenBefore)
        );
        assert_eq!(
            state.display_pin(node, 1, 30),
            Some(EndpointDisplayPin::Visible(7))
        );
        assert_eq!(
            state.display_pin(node, 13, 30),
            Some(EndpointDisplayPin::HiddenAfter)
        );
        assert_eq!(state.visual_pin_for_semantic(node, 2, 30), Some(0));
        assert_eq!(state.visual_pin_for_semantic(node, 10, 30), Some(4));
        assert_eq!(state.visual_pin_for_semantic(node, 25, 30), Some(13));
        assert_eq!(state.semantic_pin(node, 0, 30), None);
        assert_eq!(state.semantic_pin(node, 4, 30), Some(10));
    }

    #[test]
    fn reveal_centers_a_distant_match_and_clamps_at_the_end() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();

        assert!(state.reveal(node, 30, 20));
        assert_eq!(state.visible_range(node, 30), 14..26);
        assert!(state.reveal(node, 30, 29));
        assert_eq!(state.visible_range(node, 30), 18..30);
        assert!(!state.reveal(node, 30, 29));
    }

    #[test]
    fn resize_changes_width_and_visible_rows_with_bounded_accumulation() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();

        assert!(state.resize(node, 30, 200.0, egui::vec2(35.0, 10.0)));
        assert_eq!(state.width(node, 200.0), 235.0);
        assert_eq!(state.visible_limit(node, 30), 12);
        assert!(state.resize(node, 30, 200.0, egui::vec2(0.0, 11.0)));
        assert_eq!(state.visible_limit(node, 30), 13);
        assert!(state.resize(node, 30, 200.0, egui::vec2(0.0, -400.0)));
        assert_eq!(state.visible_limit(node, 30), MIN_VISIBLE_PINS);
    }

    #[test]
    fn endpoint_wheel_delta_maps_to_bounded_field_rows() {
        assert_eq!(scroll_rows(-18.0), 1);
        assert_eq!(scroll_rows(36.0), -2);
        assert_eq!(scroll_rows(-500.0), 5);
        assert_eq!(scroll_rows(0.5), -1);
        assert_eq!(scroll_rows(0.0), 0);
        assert_eq!(scroll_rows(-0.0), 0);
        assert_eq!(scroll_rows(f32::NAN), 0);
        assert_eq!(scroll_rows(f32::INFINITY), 0);
        assert_eq!(scroll_rows(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn keyboard_row_resizes_take_whole_rows_and_drop_partial_drags() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();
        // A partial upward drag leaves a negative remainder behind.
        assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, -15.0)));
        assert!(state.resize_rows(node, 30, 1));
        assert_eq!(state.visible_limit(node, 30), 13);
        assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, 6.0)));
        assert!(state.resize_rows(node, 30, -10));
        assert_eq!(state.visible_limit(node, 30), MIN_VISIBLE_PINS);
        assert!(!state.resize_rows(node, 30, -1));
        assert!(!state.resize_rows(node, 30, 0));
        assert!(state.resize_rows(node, 30, 100));
        assert_eq!(state.visible_limit(node, 30), 30);
    }

    #[test]
    fn connected_fields_keep_non_contiguous_semantic_indices_in_schema_order() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        assert!(state.set_connected(node, 15, [12, 8, 2, 8, 999]));
        assert_eq!(state.connected_count(node, 15), 3);
        assert_eq!(state.eligible_len(node, 15), 15);
        assert_eq!(state.visible_indices(node, 15), (0..12).collect::<Vec<_>>());

        assert!(state.set_connected_only(node, true));
        assert!(state.connected_only(node));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 12]);
        assert_eq!(state.visible_range(node, 15), 0..3);
        assert_eq!(state.visible_indices(node, 15), vec![2, 8, 12]);
        assert_eq!(state.display_pin_count(node, 15), 3);
        for (visual, semantic) in [2, 8, 12].into_iter().enumerate() {
            assert_eq!(state.semantic_pin(node, visual, 15), Some(semantic));
            assert_eq!(
                state.visual_pin_for_semantic(node, semantic, 15),
                Some(visual)
            );
        }
        assert_eq!(state.semantic_pin(node, 3, 15), None);
        assert_eq!(state.visual_pin_for_semantic(node, 3, 15), None);
        assert_eq!(state.visual_pin_for_semantic(node, 15, 15), None);
        assert_eq!(state.hidden_before(node, 15), 0);
        assert_eq!(state.hidden_after(node, 15), 0);
        assert!(!state.set_connected(node, 15, [8, 12, 2]));
        assert!(!state.set_connected_only(node, true));
    }

    #[test]
    fn connected_field_proxies_count_eligible_rows_not_schema_gaps() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_connected(node, 50, (0..20).map(|row| row * 2 + 1));
        state.set_connected_only(node, true);
        assert_eq!(state.max_offset(node, 50), 8);
        assert!(state.scroll_rows(node, 50, 5));
        assert_eq!(state.visible_range(node, 50), 5..17);
        assert_eq!(
            state.visible_indices(node, 50),
            (5..17).map(|row| row * 2 + 1).collect::<Vec<_>>()
        );
        assert_eq!(state.hidden_before(node, 50), 5);
        assert_eq!(state.hidden_after(node, 50), 3);
        assert_eq!(state.display_pin_count(node, 50), 14);
        assert_eq!(
            state.display_pin(node, 0, 50),
            Some(EndpointDisplayPin::HiddenBefore)
        );
        assert_eq!(
            state.display_pin(node, 1, 50),
            Some(EndpointDisplayPin::Visible(11))
        );
        assert_eq!(
            state.display_pin(node, 13, 50),
            Some(EndpointDisplayPin::HiddenAfter)
        );
        assert_eq!(state.semantic_pin(node, 0, 50), None);
        assert_eq!(state.semantic_pin(node, 13, 50), None);
        assert_eq!(state.visual_pin_for_semantic(node, 3, 50), Some(0));
        assert_eq!(state.visual_pin_for_semantic(node, 29, 50), Some(10));
        assert_eq!(state.visual_pin_for_semantic(node, 39, 50), Some(13));
        // An unconnected row is not a hidden-wire proxy just because its
        // semantic index is numerically between two connected fields.
        assert_eq!(state.visual_pin_for_semantic(node, 2, 50), None);
    }

    #[test]
    fn search_reveals_one_unconnected_field_without_reordering_or_changing_connections() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_connected(node, 15, [12, 2, 8]);
        state.set_connected_only(node, true);

        assert!(state.reveal(node, 15, 0));
        assert_eq!(state.eligible_indices(node, 15), vec![0, 2, 8, 12]);
        assert_eq!(state.connected_count(node, 15), 3);
        assert!(state.reveal(node, 15, 9));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 9, 12]);
        assert_eq!(state.semantic_pin(node, 2, 15), Some(9));
        assert!(!state.reveal(node, 15, 9));
        assert!(!state.reveal(node, 15, 15));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 9, 12]);
        assert!(state.reveal(node, 15, 8));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 12]);

        state.reveal(node, 15, 14);
        assert!(state.clear_reveal(node, 15));
        assert!(!state.clear_reveal(node, 15));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 12]);
        assert!(state.set_connected_only(node, false));
        assert_eq!(
            state.eligible_indices(node, 15),
            (0..15).collect::<Vec<_>>()
        );
        assert_eq!(state.connected_count(node, 15), 3);
    }

    #[test]
    fn connecting_a_revealed_field_keeps_it_visible_as_a_real_connection() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_connected(node, 15, [2, 8, 12]);
        state.set_connected_only(node, true);
        state.reveal(node, 15, 9);
        assert!(state.set_connected(node, 15, [12, 9, 8, 2]));
        assert_eq!(state.eligible_indices(node, 15), vec![2, 8, 9, 12]);
        assert_eq!(state.connected_count(node, 15), 4);
        assert!(!state.clear_reveal(node, 15));
        assert_eq!(state.visual_pin_for_semantic(node, 9, 15), Some(2));
    }

    #[test]
    fn reveal_in_filtered_scrolled_rows_uses_eligible_position_not_schema_index() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_connected(node, 50, (0..20).map(|row| row * 2 + 1));
        state.set_connected_only(node, true);
        state.scroll_rows(node, 50, 5);
        assert!(state.reveal(node, 50, 48));
        assert_eq!(state.eligible_len(node, 50), 21);
        assert_eq!(state.visible_range(node, 50), 9..21);
        assert_eq!(state.semantic_pin(node, 12, 50), Some(48));
        assert!(!state.reveal(node, 50, 48));
        assert!(state.clear_reveal(node, 50));
        assert_eq!(state.visible_range(node, 50), 8..20);
        assert_eq!(state.visual_pin_for_semantic(node, 48, 50), None);
    }

    #[test]
    fn empty_connected_view_remains_revealable_and_switches_back_to_all_fields() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_connected(node, 15, []);
        state.set_connected_only(node, true);
        assert_eq!(state.eligible_len(node, 15), 0);
        assert_eq!(state.visible_limit(node, 15), 0);
        assert_eq!(state.visible_range(node, 15), 0..0);
        assert_eq!(state.display_pin_count(node, 15), 0);
        assert_eq!(state.display_pin(node, 0, 15), None);
        assert!(!state.scroll_rows(node, 15, isize::MAX));
        assert!(!state.resize(node, 15, 200.0, egui::vec2(0.0, 100.0)));
        assert!(state.resize(node, 15, 200.0, egui::vec2(10.0, 100.0)));
        assert_eq!(state.width(node, 200.0), 210.0);

        assert!(state.reveal(node, 15, 7));
        assert_eq!(state.visible_indices(node, 15), vec![7]);
        assert!(state.clear_reveal(node, 15));
        assert_eq!(state.visible_indices(node, 15), Vec::<usize>::new());
        assert!(state.set_connected(node, 15, [2]));
        assert_eq!(state.visible_indices(node, 15), vec![2]);
        assert!(state.set_connected(node, 15, []));
        assert_eq!(state.visible_indices(node, 15), Vec::<usize>::new());
        assert!(state.set_connected_only(node, false));
        assert_eq!(state.visible_range(node, 15), 0..12);
    }

    #[test]
    fn changing_connected_membership_clamps_scrolling_and_preserves_manual_capacity() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        state.resize(node, 50, 200.0, egui::vec2(0.0, -400.0));
        state.set_connected(node, 50, (0..25).map(|row| row * 2));
        state.set_connected_only(node, true);
        assert_eq!(state.visible_limit(node, 50), 4);
        assert!(state.scroll_rows(node, 50, isize::MAX));
        assert_eq!(state.offset(node, 50), 21);
        state.set_connected(node, 50, [2, 8, 12]);
        assert_eq!(state.offset(node, 50), 0);
        assert_eq!(state.visible_indices(node, 50), vec![2, 8, 12]);
        state.set_connected_only(node, false);
        assert_eq!(state.visible_limit(node, 50), 4);
        assert_eq!(state.visible_range(node, 50), 0..4);
    }

    #[test]
    fn resizing_a_short_filtered_list_preserves_the_all_fields_row_capacity() {
        let node = CanvasNode::SourceBlock(0);
        for height_delta in [0.0, -120.0] {
            let mut state = EndpointScrollState::default();
            state.resize(node, 30, 200.0, egui::vec2(0.0, height_delta));
            let capacity = state.visible_limit(node, 30);
            state.set_connected(node, 30, [2, 8, 12]);
            state.set_connected_only(node, true);
            assert_eq!(state.visible_limit(node, 30), 3);
            assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, 100.0)));
            assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, -100.0)));
            state.set_connected_only(node, false);
            assert_eq!(state.visible_limit(node, 30), capacity);
        }
    }

    #[test]
    fn changing_full_schema_length_clamps_stored_offsets_even_with_no_connections() {
        let node = CanvasNode::TargetBlock(0);
        let mut state = EndpointScrollState::default();
        assert!(state.set_connected(node, 30, []));
        assert!(state.scroll_rows(node, 30, isize::MAX));
        assert_eq!(state.offset(node, 30), 18);
        let fields = egui::Rect::from_min_size(egui::pos2(10.0, 50.0), egui::vec2(100.0, 80.0));
        state.set_field_rect(node, fields);
        assert!(state.set_connected(node, 5, []));
        assert_eq!(state.offset(node, 5), 0);
        assert_eq!(state.field_rect(node), None);
        assert!(state.set_connected(node, 30, []));
        assert_eq!(state.visible_range(node, 30), 0..12);
        assert!(!state.set_connected(node, 30, []));
    }

    #[test]
    fn extreme_finite_resize_inputs_leave_only_a_finite_partial_row_remainder() {
        let node = CanvasNode::SourceBlock(0);
        for pitch in [f32::from_bits(1), 20.0, f32::MAX] {
            let mut state = EndpointScrollState::default();
            state.set_row_height(node, pitch);
            for delta in [f32::MAX, f32::MAX, -f32::MAX] {
                state.resize(node, 30, 200.0, egui::vec2(0.0, delta));
                let remainder = state.resize_remainders[&node];
                assert!(remainder.is_finite());
                assert!(remainder.abs() <= pitch);
                assert!((MIN_VISIBLE_PINS..=30).contains(&state.visible_limit(node, 30)));
            }
        }
    }

    #[test]
    fn resizing_uses_measured_row_height_and_ignores_invalid_inputs() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        state.set_row_height(node, 10.0);
        assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, 9.0)));
        assert!(state.resize(node, 30, 200.0, egui::vec2(0.0, 2.0)));
        assert_eq!(state.visible_limit(node, 30), 13);
        state.set_row_height(node, 12.0);
        assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, 11.0)));
        assert!(state.resize(node, 30, 200.0, egui::vec2(0.0, 1.0)));
        assert_eq!(state.visible_limit(node, 30), 14);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!state.resize(node, 30, 200.0, egui::vec2(invalid, invalid)));
            assert_eq!(state.visible_limit(node, 30), 14);
            assert!(state.width(node, invalid).is_finite());
            state.set_row_height(node, invalid);
        }
        assert!(!state.resize(node, 30, 200.0, egui::vec2(0.0, 19.0)));
        assert!(state.resize(node, 30, 200.0, egui::vec2(0.0, 1.0)));
        assert_eq!(state.visible_limit(node, 30), 15);
    }

    #[test]
    fn short_track_bounds_its_preferred_eighteen_unit_thumb_without_panicking() {
        let node = CanvasNode::SourceBlock(0);
        let node_rect = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(200.0, 180.0));
        let fields = egui::Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(190.0, 102.0));
        for offset in [0, 9, 18, usize::MAX] {
            let geometry = scrollbar_geometry(node, fields, node_rect, 30, 12, offset).unwrap();
            assert_eq!(geometry.track.height(), 2.0);
            assert_eq!(geometry.thumb.height(), 2.0);
            assert!(fields.contains_rect(geometry.track));
            assert!(fields.contains_rect(geometry.thumb));
            assert!(fields.contains_rect(geometry.hit_rect));
            assert_eq!(geometry.hit_rect.top(), fields.top());
            assert_eq!(geometry.hit_rect.bottom(), fields.bottom());
            assert_eq!(geometry.offset_at(egui::pos2(25.0, 101.0), 18), None);
        }
    }

    #[test]
    fn scrollbar_geometry_rejects_empty_invalid_and_non_finite_rectangles() {
        let node = CanvasNode::TargetBlock(0);
        let valid = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 80.0));
        let invalid = [
            egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 0.0)),
            egui::Rect::from_min_max(egui::pos2(10.0, 12.0), egui::pos2(20.0, 10.0)),
            egui::Rect::from_min_max(egui::pos2(f32::NAN, 10.0), egui::pos2(20.0, 30.0)),
            egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(20.0, f32::INFINITY)),
            egui::Rect::from_min_max(egui::pos2(10.0, f32::NEG_INFINITY), egui::pos2(20.0, 30.0)),
            // Finite endpoints can still produce an infinite width.
            egui::Rect::from_min_max(egui::pos2(-f32::MAX, 10.0), egui::pos2(f32::MAX, 30.0)),
        ];
        for rect in invalid {
            assert!(scrollbar_geometry(node, rect, valid, 30, 12, 0).is_none());
            assert!(scrollbar_geometry(node, valid, rect, 30, 12, 0).is_none());
            assert!(resize_geometry(node, rect, valid).is_none());
            assert!(resize_geometry(node, valid, rect).is_none());
        }
        assert!(scrollbar_geometry(node, valid, valid, 0, 0, 0).is_none());
        assert!(scrollbar_geometry(node, valid, valid, 30, 0, 0).is_none());
        assert!(scrollbar_geometry(node, valid, valid, 12, 12, 0).is_none());
        assert!(scrollbar_geometry(node, valid, valid, 10, 12, 0).is_none());
    }

    #[test]
    fn scrollbar_paint_and_hit_bounds_follow_measured_headers_at_different_scales() {
        for scale in [0.25, 1.0, 4.0] {
            let node_rect = egui::Rect::from_min_max(
                egui::pos2(10.0 * scale, 20.0 * scale),
                egui::pos2(230.0 * scale, 250.0 * scale),
            );
            for header_height in [23.0, 47.0, 81.0] {
                let fields = egui::Rect::from_min_max(
                    egui::pos2(18.0 * scale, (20.0 + header_height) * scale),
                    egui::pos2(222.0 * scale, 230.0 * scale),
                );
                for node in [CanvasNode::SourceBlock(0), CanvasNode::TargetBlock(0)] {
                    for offset in [0, 9, 18] {
                        let geometry =
                            scrollbar_geometry(node, fields, node_rect, 30, 12, offset).unwrap();
                        assert_eq!(geometry.track.top(), fields.top());
                        assert_eq!(geometry.track.bottom(), fields.bottom());
                        for rect in [geometry.track, geometry.thumb, geometry.hit_rect] {
                            assert!(finite_positive_rect(rect));
                            assert!(fields.contains_rect(rect));
                            assert!(node_rect.contains_rect(rect));
                        }
                        let resize = resize_geometry(node, fields, node_rect).unwrap();
                        assert_eq!(resize.width_rect.top(), fields.top());
                        assert_eq!(resize.width_rect.bottom(), fields.bottom());
                        assert!(resize.height_rect.top() >= fields.bottom());
                        if let Some(corner) = resize.corner_rect {
                            assert!(corner.top() >= fields.bottom());
                        }
                        if matches!(node, CanvasNode::SourceBlock(_)) {
                            assert!(resize.width_rect.right() <= geometry.hit_rect.left());
                        } else {
                            assert!(resize.width_rect.left() >= geometry.hit_rect.right());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_and_narrow_tracks_keep_finite_contained_thumbs() {
        let node_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 200.0));
        for height in [0.001, 2.0, 17.0, 18.0, 19.0, 100.0] {
            for width in [0.001, 2.0, 5.0, 40.0] {
                let fields =
                    egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(width, height));
                for node in [CanvasNode::SourceBlock(0), CanvasNode::TargetBlock(0)] {
                    for offset in [0, usize::MAX] {
                        let geometry =
                            scrollbar_geometry(node, fields, node_rect, 30, 12, offset).unwrap();
                        assert!(fields.contains_rect(geometry.track));
                        assert!(fields.contains_rect(geometry.thumb));
                        assert!(fields.contains_rect(geometry.hit_rect));
                        assert!(geometry.thumb.height() <= geometry.track.height());
                    }
                }
            }
        }
    }

    #[test]
    fn scrollbar_drag_offsets_are_bounded_and_invalid_pointer_positions_are_ignored() {
        let node = CanvasNode::TargetBlock(0);
        let fields = egui::Rect::from_min_max(egui::pos2(10.0, 30.0), egui::pos2(190.0, 150.0));
        let geometry = scrollbar_geometry(node, fields, fields, 30, 12, 0).unwrap();
        assert_eq!(geometry.offset_at(fields.left_top(), 18), Some(0));
        assert_eq!(geometry.offset_at(fields.right_bottom(), 18), Some(18));
        for pointer in [egui::pos2(f32::NAN, 50.0), egui::pos2(50.0, f32::INFINITY)] {
            assert_eq!(geometry.offset_at(pointer, 18), None);
        }
    }

    #[test]
    fn field_geometry_is_measured_and_invalidated_instead_of_guessing_header_offsets() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        assert_eq!(state.field_rect(node), None);
        let fields = egui::Rect::from_min_max(egui::pos2(10.0, 47.0), egui::pos2(180.0, 170.0));
        state.set_field_rect(node, fields);
        assert_eq!(state.field_rect(node), Some(fields));
        state.set_field_rect(node, egui::Rect::NOTHING);
        assert_eq!(state.field_rect(node), None);
        state.set_field_rect(node, fields);
        assert!(state.scroll_rows(node, 30, 1));
        assert_eq!(state.field_rect(node), None);
        state.set_field_rect(node, fields);
        assert!(state.set_connected_only(node, true));
        assert_eq!(state.field_rect(node), None);
        state.set_field_rect(node, fields);
        assert!(state.set_connected(node, 30, [2, 8, 12]));
        assert_eq!(state.field_rect(node), None);
        state.set_field_rect(node, fields);
        assert!(!state.set_connected(node, 30, [8, 12, 2]));
        assert_eq!(state.field_rect(node), Some(fields));
        assert!(state.resize(node, 30, 200.0, egui::vec2(10.0, 0.0)));
        assert_eq!(state.field_rect(node), None);
    }

    #[test]
    fn scrollbar_ui_accepts_a_two_unit_measured_field_list() {
        let ctx = egui::Context::default();
        let mut state = EndpointScrollState::default();
        let node = CanvasNode::SourceBlock(0);
        let fields = egui::Rect::from_min_max(egui::pos2(20.0, 100.0), egui::pos2(190.0, 102.0));
        let node_rect = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(200.0, 180.0));
        let _ = ctx.run_test_ui(egui::RawInput::default(), |ui| {
            assert!(!show_scrollbar(
                ui,
                node,
                node_rect,
                30,
                &mut state,
                egui::Color32::WHITE,
                "Source"
            ));
            state.set_field_rect(node, fields);
            assert!(!show_scrollbar(
                ui,
                node,
                node_rect,
                30,
                &mut state,
                egui::Color32::WHITE,
                "Source"
            ));
            assert!(!show_resize_handles(
                ui, node, node_rect, 30, 200.0, &mut state, "Source"
            ));
        });
    }

    #[test]
    fn rendered_field_geometry_excludes_the_complete_header_footer_and_proxy_rows() {
        let node = CanvasNode::SourceBlock(0);
        let mut state = EndpointScrollState::default();
        let rect = |top, bottom| {
            egui::Rect::from_min_max(egui::pos2(10.0, top), egui::pos2(210.0, bottom))
        };
        state.begin_geometry_frame();
        // Hidden-before and hidden-after proxy rows are deliberately not added.
        state.record_field_row(node, rect(80.0, 100.0));
        state.record_field_row(node, rect(102.0, 122.0));
        state.record_header_rect(node, rect(10.0, 85.0));
        state.record_footer_rect(node, rect(115.0, 140.0));
        state.finish_geometry(node, rect(0.0, 150.0));
        assert_eq!(state.field_rect(node), Some(rect(85.0, 115.0)));
        state.begin_geometry_frame();
        state.record_header_rect(node, rect(10.0, 75.0));
        state.finish_geometry(node, rect(0.0, 150.0));
        assert_eq!(state.field_rect(node), None);
        state.begin_geometry_frame();
        state.record_field_row(node, rect(80.0, 100.0));
        // Missing header measurement must never fall back to a magic offset.
        state.finish_geometry(node, rect(0.0, 150.0));
        assert_eq!(state.field_rect(node), None);
    }
}
