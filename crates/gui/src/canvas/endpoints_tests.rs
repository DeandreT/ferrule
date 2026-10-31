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
    let rect =
        |top, bottom| egui::Rect::from_min_max(egui::pos2(10.0, top), egui::pos2(210.0, bottom));
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
