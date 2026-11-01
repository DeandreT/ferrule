//! SWIFT MT target headers: one direction for the output and one line per
//! written message, `<basic header> <application header>`.
//!
//! Drafts and text-input state belong to the loaded document and target name.
//! Closing the Inspector, refreshing sources, undoing unrelated edits and Save
//! As retain them. Replacing a document starts from its saved headers, even
//! when its path and options are unchanged; embedded stages have separate owners.

use mapping::{FormatOptions, SwiftMtDirection, SwiftMtMessageHeaders, SwiftMtOutputOptions};

/// Editor text kept between frames, with the saved value it was derived
/// from so undo or another editor resets it.
#[derive(Clone)]
struct Draft {
    direction: SwiftMtDirection,
    text: String,
    saved: Option<SwiftMtOutputOptions>,
}

impl Draft {
    fn from_options(options: &FormatOptions) -> Self {
        let saved = options.swift_mt_output.clone();
        Self {
            direction: saved
                .as_ref()
                .map_or(SwiftMtDirection::Input, SwiftMtOutputOptions::direction),
            text: saved.as_ref().map(headers_text).unwrap_or_default(),
            saved,
        }
    }
}

pub(super) fn headers_text(output: &SwiftMtOutputOptions) -> String {
    output
        .messages()
        .iter()
        .map(|message| format!("{} {}", message.basic_header, message.application_header))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Blank text clears the headers. Each other line must hold exactly the two
/// header values; their character positions are checked by
/// [`header_issues`] and project validation.
pub(super) fn parse_headers(
    direction: SwiftMtDirection,
    text: &str,
) -> Result<Option<SwiftMtOutputOptions>, String> {
    let mut messages = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let [basic, application] = parts.as_slice() else {
            return Err(format!(
                "Line {}: expected a basic header and an application header",
                index + 1
            ));
        };
        messages.push(SwiftMtMessageHeaders {
            basic_header: (*basic).to_owned(),
            application_header: (*application).to_owned(),
        });
    }
    if messages.is_empty() {
        return Ok(None);
    }
    SwiftMtOutputOptions::new(direction, messages)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Header values the writer would refuse, including repeated basic headers
/// and output message input references, one entry per refusal.
pub(super) fn header_issues(output: &SwiftMtOutputOptions) -> Vec<String> {
    format_edi::swift::header_refusals(output)
        .into_iter()
        .map(|(message, refusal)| format!("Message {message}: {refusal}"))
        .collect()
}

/// Returns whether the saved options changed. `document` is the session-local
/// project owner; `target` is `None` for primary, else the stable target name.
pub(super) fn show_fields(
    ui: &mut egui::Ui,
    options: &mut FormatOptions,
    document: egui::Id,
    target: Option<&str>,
) -> bool {
    ui.push_id(("swift_mt_output", document, target), |ui| {
        show_header_fields(ui, options)
    })
    .inner
}

fn show_header_fields(ui: &mut egui::Ui, options: &mut FormatOptions) -> bool {
    let id = ui.make_persistent_id("draft");
    let mut draft = ui
        .data(|data| data.get_temp::<Draft>(id))
        .filter(|draft| draft.saved == options.swift_mt_output)
        .unwrap_or_else(|| Draft::from_options(options));
    ui.strong("SWIFT MT output");
    let mut edited = false;
    ui.horizontal(|ui| {
        ui.label("Direction");
        edited |= ui
            .selectable_value(&mut draft.direction, SwiftMtDirection::Input, "Input")
            .changed();
        edited |= ui
            .selectable_value(&mut draft.direction, SwiftMtDirection::Output, "Output")
            .changed();
    });
    ui.weak(
        "One line per message: basic header, then the application header after the message type.",
    );
    edited |= ui
        .add(
            egui::TextEdit::multiline(&mut draft.text)
                .font(egui::TextStyle::Monospace)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("F01BANKBEBBAXXX0000000001 BANKDEFFXXXXN"),
        )
        .changed();
    let mut changed = false;
    match parse_headers(draft.direction, &draft.text) {
        Ok(parsed) => {
            if edited && parsed != options.swift_mt_output {
                options.swift_mt_output = parsed.clone();
                changed = true;
            }
            draft.saved = parsed;
            match &options.swift_mt_output {
                None => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Add one header line per message to write SWIFT MT output.",
                    );
                }
                Some(output) => {
                    for issue in header_issues(output) {
                        ui.colored_label(ui.visuals().error_fg_color, issue);
                    }
                }
            }
        }
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt;

    const BASIC: &str = "F01BANKBEBBAXXX0000000001";

    #[test]
    fn header_lines_round_trip_and_blank_text_clears() {
        let text = format!("{BASIC} BANKDEFFXXXXN\n\nF01BANKBEBBAXXX0000000002   BANKDEFFXXXX");
        let parsed = parse_headers(SwiftMtDirection::Input, &text)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.messages().len(), 2);
        assert_eq!(
            headers_text(&parsed),
            format!("{BASIC} BANKDEFFXXXXN\nF01BANKBEBBAXXX0000000002 BANKDEFFXXXX")
        );
        assert!(header_issues(&parsed).is_empty());
        assert_eq!(parse_headers(SwiftMtDirection::Input, " \n "), Ok(None));
    }

    #[test]
    fn malformed_lines_and_values_are_reported() {
        assert!(parse_headers(SwiftMtDirection::Input, BASIC).is_err());
        let parsed = parse_headers(SwiftMtDirection::Output, &format!("{BASIC} BANKDEFFXXXXN"))
            .unwrap()
            .unwrap();
        let issues = header_issues(&parsed);
        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].contains("output application header"),
            "{issues:?}"
        );
        let repeated = parse_headers(
            SwiftMtDirection::Input,
            &format!("{BASIC} BANKDEFFXXXXN\n{BASIC} BANKDEFFXXXXN"),
        )
        .unwrap()
        .unwrap();
        let issues = header_issues(&repeated);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(
            issues[0].starts_with("Message 2:") && issues[0].contains("basic header"),
            "{issues:?}"
        );
    }

    #[test]
    fn drawing_saved_headers_leaves_the_options_unchanged() {
        let saved = parse_headers(
            SwiftMtDirection::Output,
            &format!("{BASIC} 1200240101BANKBEBBAXXX22221234562401011201N"),
        )
        .unwrap();
        let mut options = FormatOptions {
            swift_mt_output: saved.clone(),
            ..FormatOptions::default()
        };
        let context = egui::Context::default();
        for _ in 0..2 {
            let _ = context.run_test_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    assert!(!show_fields(
                        ui,
                        &mut options,
                        egui::Id::unique("test_document"),
                        Some("test"),
                    ));
                });
            });
        }
        assert_eq!(options.swift_mt_output, saved);
    }
}
