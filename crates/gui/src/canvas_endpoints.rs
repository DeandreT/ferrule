//! View state for large source and target endpoint nodes.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::canvas::CanvasNode;

pub const VISIBLE_PIN_LIMIT: usize = 12;
const MIN_VISIBLE_PINS: usize = 4;
const PIN_ROW_HEIGHT: f32 = 20.0;
const MIN_WIDTH: f32 = 110.0;
const MAX_WIDTH: f32 = 480.0;

#[derive(Default)]
pub struct EndpointScrollState {
    offsets: BTreeMap<CanvasNode, usize>,
    visible_rows: BTreeMap<CanvasNode, usize>,
    widths: BTreeMap<CanvasNode, f32>,
    resize_remainders: BTreeMap<CanvasNode, f32>,
}

impl EndpointScrollState {
    pub fn visible_limit(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_rows
            .get(&node)
            .copied()
            .unwrap_or(VISIBLE_PIN_LIMIT)
            .min(total)
    }

    pub fn offset(&self, node: CanvasNode, total: usize) -> usize {
        self.offsets
            .get(&node)
            .copied()
            .unwrap_or(0)
            .min(max_offset(total, self.visible_limit(node, total)))
    }

    pub fn visible_range(&self, node: CanvasNode, total: usize) -> Range<usize> {
        let start = self.offset(node, total);
        start..(start + self.visible_limit(node, total)).min(total)
    }

    pub fn visible_len(&self, node: CanvasNode, total: usize) -> usize {
        self.visible_range(node, total).len()
    }

    pub fn semantic_pin(
        &self,
        node: CanvasNode,
        displayed_pin: usize,
        total: usize,
    ) -> Option<usize> {
        let range = self.visible_range(node, total);
        (displayed_pin < range.len()).then_some(range.start + displayed_pin)
    }

    pub fn displayed_pin(
        &self,
        node: CanvasNode,
        semantic_pin: usize,
        total: usize,
    ) -> Option<usize> {
        let range = self.visible_range(node, total);
        if range.contains(&semantic_pin) {
            Some(semantic_pin - range.start)
        } else {
            None
        }
    }

    pub fn scroll_rows(&mut self, node: CanvasNode, total: usize, rows: isize) -> bool {
        let old = self.offset(node, total);
        let new = old
            .saturating_add_signed(rows)
            .min(max_offset(total, self.visible_limit(node, total)));
        self.set_offset(node, total, new)
    }

    pub fn reveal(&mut self, node: CanvasNode, total: usize, semantic_pin: usize) -> bool {
        if semantic_pin >= total {
            return false;
        }
        let range = self.visible_range(node, total);
        if range.contains(&semantic_pin) {
            return false;
        }
        let centered = semantic_pin.saturating_sub(self.visible_limit(node, total) / 2);
        self.set_offset(node, total, centered)
    }

    pub fn width(&self, node: CanvasNode, natural_width: f32) -> f32 {
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
        if delta.x != 0.0 {
            let old = self.width(node, natural_width);
            let width = (old + delta.x).clamp(MIN_WIDTH, MAX_WIDTH);
            self.widths.insert(node, width);
            changed |= width != old;
        }
        if delta.y != 0.0 && total > 0 {
            let remainder = self.resize_remainders.entry(node).or_default();
            *remainder += delta.y;
            let steps = (*remainder / PIN_ROW_HEIGHT).trunc() as isize;
            if steps != 0 {
                *remainder -= steps as f32 * PIN_ROW_HEIGHT;
                let old = self.visible_limit(node, total);
                let minimum = MIN_VISIBLE_PINS.min(total);
                let rows = old.saturating_add_signed(steps).clamp(minimum, total);
                self.visible_rows.insert(node, rows);
                let max_offset = max_offset(total, rows);
                if let Some(offset) = self.offsets.get_mut(&node) {
                    *offset = (*offset).min(max_offset);
                }
                changed |= rows != old;
            }
        }
        changed
    }

    fn set_offset(&mut self, node: CanvasNode, total: usize, offset: usize) -> bool {
        let old = self.offset(node, total);
        let offset = offset.min(max_offset(total, self.visible_limit(node, total)));
        if offset == 0 {
            self.offsets.remove(&node);
        } else {
            self.offsets.insert(node, offset);
        }
        old != offset
    }
}

pub fn show_scrollbar(
    ui: &mut egui::Ui,
    node: CanvasNode,
    node_rect: egui::Rect,
    total: usize,
    state: &mut EndpointScrollState,
    accent: egui::Color32,
) -> bool {
    let visible = state.visible_limit(node, total);
    if total <= visible || !node_rect.is_positive() {
        return false;
    }

    let mut changed = false;

    let on_left = matches!(node, CanvasNode::SourceBlock(_));
    let x = if on_left {
        node_rect.left() + 3.0
    } else {
        node_rect.right() - 8.0
    };
    let track = egui::Rect::from_min_max(
        egui::pos2(x, node_rect.top() + 28.0),
        egui::pos2(x + 5.0, node_rect.bottom() - 20.0),
    );
    let track = track.intersect(node_rect);
    if !track.is_positive() {
        return changed;
    }

    let thumb_height = (track.height() * visible as f32 / total as f32).clamp(18.0, track.height());
    let travel = (track.height() - thumb_height).max(0.0);
    let progress = state.offset(node, total) as f32 / max_offset(total, visible).max(1) as f32;
    let thumb = egui::Rect::from_min_size(
        egui::pos2(track.left(), track.top() + travel * progress),
        egui::vec2(track.width(), thumb_height),
    );

    ui.painter().rect_filled(
        track,
        2.0,
        ui.visuals().widgets.inactive.bg_fill.gamma_multiply(0.45),
    );
    ui.painter().rect_filled(thumb, 2.0, accent);

    let response = ui.interact(
        track.expand2(egui::vec2(3.0, 1.0)),
        ui.id().with(("endpoint_scroll", node)),
        egui::Sense::click_and_drag(),
    );
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let progress =
            ((pointer.y - track.top() - thumb_height / 2.0) / travel.max(1.0)).clamp(0.0, 1.0);
        let offset = (progress * max_offset(total, visible) as f32).round() as usize;
        changed |= state.set_offset(node, total, offset);
    }
    response.on_hover_text(format!(
        "Fields {}-{} of {total}",
        state.offset(node, total) + 1,
        state.visible_range(node, total).end
    ));
    changed
}

pub fn show_resize_handles(
    ui: &mut egui::Ui,
    node: CanvasNode,
    node_rect: egui::Rect,
    total: usize,
    natural_width: f32,
    state: &mut EndpointScrollState,
) -> bool {
    if !node_rect.is_positive() {
        return false;
    }

    // Keep the width handle opposite the pins so resizing an endpoint never
    // steals a wire drag. Sources place pins on the right; targets use left.
    let on_left = matches!(node, CanvasNode::SourceBlock(_));
    let edge_x = if on_left {
        node_rect.left()
    } else {
        node_rect.right()
    };
    let corner = node_rect.right_bottom();
    let corner_rect = egui::Rect::from_min_max(corner - egui::vec2(16.0, 16.0), corner);
    let width_rect = egui::Rect::from_min_max(
        egui::pos2(edge_x - 4.0, node_rect.top() + 24.0),
        egui::pos2(edge_x + 4.0, node_rect.bottom() - 18.0),
    );
    let bottom_rect = egui::Rect::from_min_max(
        egui::pos2(node_rect.left() + 18.0, node_rect.bottom() - 4.0),
        egui::pos2(node_rect.right() - 18.0, node_rect.bottom() + 4.0),
    );

    let width_response = ui
        .interact(
            width_rect,
            ui.id().with(("endpoint_resize_width", node)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("Resize endpoint width");
    let height_response = ui
        .interact(
            bottom_rect,
            ui.id().with(("endpoint_resize_height", node)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text("Resize visible fields");
    let corner_response = ui
        .interact(
            corner_rect,
            ui.id().with(("endpoint_resize_corner", node)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeNwSe)
        .on_hover_text("Resize endpoint");

    let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.inactive.fg_stroke.color);
    for inset in [4.0, 8.0, 12.0] {
        ui.painter().line_segment(
            [
                corner + egui::vec2(-inset, -3.0),
                corner + egui::vec2(-3.0, -inset),
            ],
            stroke,
        );
    }

    let width_direction = if on_left { -1.0 } else { 1.0 };
    let width_delta =
        width_response.drag_delta().x * width_direction + corner_response.drag_delta().x;
    let height_delta = height_response.drag_delta().y + corner_response.drag_delta().y;
    state.resize(
        node,
        total,
        natural_width,
        egui::vec2(width_delta, height_delta),
    )
}

const fn max_offset(total: usize, visible: usize) -> usize {
    total.saturating_sub(visible)
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
        assert_eq!(state.semantic_pin(node, 3, 30), Some(10));
        assert_eq!(state.displayed_pin(node, 10, 30), Some(3));
        assert_eq!(state.displayed_pin(node, 2, 30), None);
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
}
