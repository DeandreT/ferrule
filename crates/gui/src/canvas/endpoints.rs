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
#[path = "endpoints_tests.rs"]
mod tests;
