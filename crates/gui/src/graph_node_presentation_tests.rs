//! Authored presentation fixtures; no external mapping or customer data.

use crate::test_ui::ContextExt as _;

use super::*;
use crate::appearance::CanvasDensity;

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn raw_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 800.0),
        )),
        ..Default::default()
    }
}

fn string_node(text: &str) -> Node {
    Node::Const {
        value: ir::Value::String(text.into()),
    }
}

fn galleys(output: &egui::FullOutput) -> Vec<&egui::Galley> {
    fn collect<'a>(shape: &'a egui::Shape, output: &mut Vec<&'a egui::Galley>) {
        match shape {
            egui::Shape::Text(text) => output.push(&text.galley),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, output);
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut result);
    }
    result
}

#[test]
fn constants_fit_measured_text_in_all_three_densities() {
    for density in CanvasDensity::ALL {
        let context = context();
        let _ = context.run_test_ui(raw_input(), |ui| {
            density.apply(ui);
            let empty = string_node("");
            let short = string_node("OK");
            let longer = string_node("a meaningfully longer authored literal");
            let empty_width = node_width(ui, &empty, "Constant", false);
            let short_width = node_width(ui, &short, "Constant", false);
            let longer_width = node_width(ui, &longer, "Constant", false);
            assert!(empty_width < short_width);
            assert!(short_width < longer_width);
            assert!((MIN_HEADER_WIDTH..=MAX_HEADER_WIDTH).contains(&longer_width));
            assert_eq!(
                header(&empty, "Constant", false).unwrap().summary,
                "string \"\""
            );

            // Same character count, different glyph widths: no character-count
            // estimate may decide how wide a constant becomes.
            let narrow = node_width(ui, &string_node("iiiiiiiiiiii"), "Constant", false);
            let wide = node_width(ui, &string_node("WWWWWWWWWWWW"), "Constant", false);
            assert!(narrow < wide);
        });
    }
}

#[test]
fn rendered_headers_use_real_selected_fonts_and_pixel_elision() {
    for (density, zoom) in [
        (CanvasDensity::Standard, 1.0),
        (CanvasDensity::Compact, 1.5),
        (CanvasDensity::Dense, 2.0),
    ] {
        let context = context();
        context.set_zoom_factor(zoom);
        let node = string_node(&"Long authored value 資料 🦀 ".repeat(400));
        let before = serde_json::to_string(&node).unwrap();
        let output = context.run_test_ui(raw_input(), |ui| {
            density.apply(ui);
            let header = header(&node, "Constant", false).unwrap();
            let size = header_size(ui, &header);
            assert!(size.x <= MAX_HEADER_WIDTH);
            let response = show_header(ui, header, "Constant", None);
            assert!(response.rect.width() <= MAX_HEADER_WIDTH + 1.0);
            assert!(response.rect.height() <= size.y + 1.0);
        });
        let galleys = galleys(&output);
        let literal = galleys
            .iter()
            .find(|galley| galley.text().starts_with("string "))
            .unwrap();
        assert_eq!(literal.rows.len(), 1);
        assert!(literal.elided);
        assert_eq!(
            literal.job.sections[0].format.font_id.size,
            density.font_size()
        );
        assert_eq!(serde_json::to_string(&node).unwrap(), before);
    }
}

#[test]
fn quoted_whitespace_multiline_json_and_unicode_are_presentation_only() {
    let fixtures = [
        "".to_owned(),
        "  keep significant whitespace  ".to_owned(),
        "\tlead\r\ntrail\t ".to_owned(),
        "{\n  \"label\": \"資料 🦀\",\n  \"items\": [1, 2, 3]\n}".to_owned(),
        format!(" \t{}\nfull-tail-marker  ", "資料e\u{301}🦀".repeat(2000)),
    ];
    for text in fixtures {
        let value = ir::Value::String(text.clone());
        let preview = literal_preview(&value);
        assert!(preview.starts_with('"') && preview.ends_with('"'));
        assert!(!preview.contains('\n') && !preview.contains('\r'));
        if text.chars().count() <= MAX_LITERAL_PREVIEW_CHARS {
            assert_eq!(preview, format!("{text:?}"));
        } else {
            assert!(preview.ends_with("…\""));
            assert!(preview.len() <= MAX_LITERAL_PREVIEW_CHARS * 10 + 8);
        }
        let node = Node::Const {
            value: value.clone(),
        };
        let header = header(&node, "Constant", false).unwrap();
        assert!(header.accessible_label.contains(&format!("{text:?}")));
        assert!(hint(&node, "Constant").contains(&format!("{text:?}")));
        let Node::Const { value: stored } = node else {
            unreachable!()
        };
        assert_eq!(stored, value);
        assert_eq!(stored, ir::Value::String(text));
    }
}

#[test]
fn auto_width_remeasures_after_edits_and_respects_manual_widths() {
    let context = context();
    let _ = context.run_test_ui(raw_input(), |ui| {
        CanvasDensity::Compact.apply(ui);
        let short = header(&string_node("x"), "Constant", false).unwrap();
        let edited = header(
            &string_node("longer committed authored value"),
            "Constant",
            false,
        )
        .unwrap();
        assert!(header_size(ui, &short).x < header_size(ui, &edited).x);
        assert_eq!(resolved_header_width(ui, &short, Some(200.0)), 200.0);
        assert_eq!(resolved_header_width(ui, &edited, Some(200.0)), 200.0);
        assert_eq!(
            resolved_header_width(ui, &short, None),
            header_size(ui, &short).x
        );
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                resolved_header_width(ui, &short, Some(invalid)),
                header_size(ui, &short).x
            );
        }
        assert_eq!(
            resolved_header_width(ui, &short, Some(0.0)),
            MIN_HEADER_WIDTH
        );
    });
}

#[test]
fn narrow_text_past_sixteen_characters_is_not_arbitrarily_shortened() {
    let context = context();
    let node = string_node("iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii");
    let output = context.run_test_ui(raw_input(), |ui| {
        CanvasDensity::Compact.apply(ui);
        let header = header(&node, "Constant", false).unwrap();
        assert_eq!(
            header.summary,
            "string \"iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii\""
        );
        show_header(ui, header, "Constant", None);
    });
    let galleys = galleys(&output);
    let literal = galleys
        .iter()
        .find(|galley| galley.text().starts_with("string "))
        .unwrap();
    assert!(!literal.elided);
}

#[test]
fn literal_output_text_is_distinct_from_the_output_node_marker() {
    let node = string_node("keep (output)");
    assert_eq!(
        header(&node, "Constant", false).unwrap().summary,
        "string \"keep (output)\""
    );
    assert_eq!(
        header(&node, "Constant (output)", true).unwrap().summary,
        "string \"keep (output)\" · output",
    );
}

#[test]
fn owned_source_headers_retain_the_complete_owner_and_frame_metadata() {
    let node = Node::OwnedPosition(mapping::OwnedPosition {
        owner: mapping::SourceOwner::Named {
            name: "Authored source 資料".into(),
        },
        collection: Some(vec!["Rows".into()]),
    });
    let before = serde_json::to_string(&node).unwrap();
    let header = header(&node, "Owned position", false).unwrap();
    assert!(header.accessible_label.contains("Authored source 資料"));
    assert!(header.accessible_label.contains("explicit [\"Rows\"]"));
    assert!(
        header
            .accessible_label
            .contains("Counts exactly this input's active items")
    );
    assert!(!is_inline(&node));
    assert_eq!(serde_json::to_string(&node).unwrap(), before);
}

#[test]
fn owned_row_path_header_keeps_explicit_root_and_named_owner_read_only() {
    let node = Node::OwnedRowDocumentPath(mapping::OwnedRowDocumentPath {
        owner: mapping::SourceOwner::Named {
            name: "Authored source 資料".into(),
        },
        collection: Some(vec![]),
    });
    let before = serde_json::to_string(&node).unwrap();
    let header = header(&node, "Owned row document path", false).unwrap();
    assert!(header.accessible_label.contains("Authored source 資料"));
    assert!(header.accessible_label.contains("explicit []"));
    assert!(
        header
            .accessible_label
            .contains("Read-only; no owner or ancestor fallback")
    );
    assert!(!is_inline(&node));
    assert_eq!(serde_json::to_string(&node).unwrap(), before);
}

#[test]
fn conditionals_have_explicit_title_and_unelided_semantic_port_labels() {
    let node = Node::If {
        condition: 31,
        then: 7,
        else_: 23,
    };
    let before = serde_json::to_string(&node).unwrap();
    assert_eq!(header(&node, "If", false).unwrap().summary, "If / else");
    assert!(!is_inline(&node));
    let context = context();
    let output = context.run_test_ui(raw_input(), |ui| {
        CanvasDensity::Dense.apply(ui);
        ui.set_max_width(4.0);
        for (index, expected) in ["condition", "then", "else"].into_iter().enumerate() {
            assert_eq!(conditional_input_label(&node, index), Some(expected));
            show_input(ui, "ignored fallback", Some(&node), index);
        }
        assert_eq!(conditional_input_label(&node, 3), None);
    });
    let galleys = galleys(&output);
    for label in ["condition", "then", "else"] {
        let galley = galleys
            .iter()
            .find(|galley| galley.text() == label)
            .unwrap();
        assert!(!galley.elided);
        assert_eq!(galley.rows.len(), 1);
    }
    assert_eq!(serde_json::to_string(&node).unwrap(), before);
}

#[test]
fn inline_operations_remain_individual_nodes_and_keep_complete_documentation() {
    let nodes = [
        Node::Call {
            function: "exists".into(),
            args: vec![41],
        },
        Node::Call {
            function: "not".into(),
            args: vec![42],
        },
    ];
    for (node, label) in nodes.iter().zip(["Exists", "Not"]) {
        let before = serde_json::to_string(node).unwrap();
        assert!(is_inline(node));
        assert_eq!(header(node, label, false).unwrap().summary, label);
        let Node::Call { function, .. } = node else {
            unreachable!()
        };
        assert!(hint(node, label).contains(functions::builtin(function).unwrap().documentation));
        assert_eq!(serde_json::to_string(node).unwrap(), before);
    }
    assert!(!is_inline(&Node::Call {
        function: "not".into(),
        args: vec![1, 2]
    }));
    assert!(!is_inline(&Node::Call {
        function: "concat".into(),
        args: vec![1]
    }));
}

#[test]
fn header_edit_trigger_stays_on_one_line_and_remains_keyboard_accessible() {
    let context = context();
    let node = string_node("edit fixture");
    let mut trigger = egui::Id::NULL;
    let _ = context.run_test_ui(raw_input(), |ui| {
        CanvasDensity::Compact.apply(ui);
        let (label, edit) = show_header_with_edit(
            ui,
            header(&node, "Constant", false).unwrap(),
            "Constant",
            None,
            false,
            true,
        );
        assert!(edit.enabled());
        assert!(edit.rect.height() >= 18.0);
        assert!(label.rect.top() <= edit.rect.bottom() && edit.rect.top() <= label.rect.bottom());
        trigger = edit.id;
    });
    context.memory_mut(|memory| memory.request_focus(trigger));
    let mut input = raw_input();
    input.events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = context.run_test_ui(input, |ui| {
        CanvasDensity::Compact.apply(ui);
        let (_, edit) = show_header_with_edit(
            ui,
            header(&node, "Constant", false).unwrap(),
            "Constant",
            None,
            false,
            true,
        );
        assert_eq!(edit.id, trigger);
        assert!(edit.has_focus());
        assert!(edit.clicked());
    });
    let _ = context.run_test_ui(raw_input(), |ui| {
        let (_, edit) = show_header_with_edit(
            ui,
            header(&node, "Constant", false).unwrap(),
            "Constant",
            None,
            true,
            false,
        );
        assert!(!edit.enabled());
        assert!(!edit.clicked());
    });
}

/// Lays out a header the way egui-snarl does: its collapse button and drag
/// space come first on the same left-to-right row. Returns the cursor after
/// them and the row's final right edge.
fn snarl_header_row<R>(ui: &mut Ui, header: impl FnOnce(&mut Ui) -> R) -> (f32, f32, R) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
        let icon_width = ui.spacing().icon_width;
        let _ = ui.allocate_exact_size(Vec2::splat(icon_width), egui::Sense::click());
        let _ = ui.allocate_exact_size(Vec2::new(24.0, 0.0), egui::Sense::hover());
        let start = ui.cursor().min.x;
        let inner = header(ui);
        (start, ui.min_rect().right(), inner)
    })
    .inner
}

#[test]
fn headers_start_after_snarls_collapse_button_and_keep_their_widths() {
    let context = context();
    let _ = context.run_test_ui(raw_input(), |ui| {
        CanvasDensity::Compact.apply(ui);
        let node = string_node("collapse fixture");
        let header = || header(&node, "Constant", false).unwrap();
        let edit_space = ui.spacing().interact_size.y.max(18.0) + ui.spacing().item_spacing.x;
        for manual in [None, Some(200.0)] {
            let preferred = resolved_header_width(ui, &header(), manual);
            let (start, right, label) =
                snarl_header_row(ui, |ui| show_header(ui, header(), "Constant", manual));
            assert!(start <= label.rect.left(), "{manual:?}: {start} {label:?}");
            assert_eq!(right, start + preferred, "{manual:?}");

            let preferred = manual.unwrap_or_else(|| header_size(ui, &header()).x + edit_space);
            let (start, right, (label, edit)) = snarl_header_row(ui, |ui| {
                show_header_with_edit(ui, header(), "Constant", manual, true, true)
            });
            assert!(start <= label.rect.left(), "{manual:?}: {start} {label:?}");
            // The trigger follows the title; auto width fits both exactly.
            assert!(label.rect.right() <= edit.rect.left());
            assert!(edit.rect.right() <= start + preferred + 0.5, "{manual:?}");
            if manual.is_none() {
                assert!((edit.rect.right() - (start + preferred)).abs() < 0.5);
            }
            assert_eq!(right, start + preferred, "{manual:?}");
        }
    });
}
