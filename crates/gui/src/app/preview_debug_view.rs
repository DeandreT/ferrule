//! Read-only views of a paused live preview, run or pipeline: the pending
//! write, node value, input or failure, and its annotated context.

use super::*;

pub(in crate::app) fn show_live_debug_state(ui: &mut egui::Ui, phase: &PreviewPhase, debug: bool) {
    match phase {
        PreviewPhase::Running => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong(if debug {
                    "Debug preview is running"
                } else {
                    "Preview is running"
                });
            });
            if debug {
                ui.weak("Execution pauses at matching target writes or graph evaluations.");
            }
        }
        PreviewPhase::Stopping => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong("Stopping preview");
            });
        }
        PreviewPhase::Paused(write) => {
            ui.strong("Paused before target-field insertion");
            let target = match &write.scope.target {
                engine::TraceTarget::Primary => "Primary",
                engine::TraceTarget::Named(name) => name,
            };
            ui.label(format!("Target: {target}"));
            let scope = if write.scope.target_path.is_empty() {
                "<root>".to_string()
            } else {
                write.scope.target_path.join(" / ")
            };
            ui.label(format!("Scope: {scope}"));
            ui.label(format!("Pending field: {}", write.field));
            if write.field_truncated {
                ui.weak("Field name preview was truncated.");
            }
            ui.label(format!("Binding: {}", binding_label(write.binding)));
            ui.label(format!("Pending value: {}", debug_value(&write.pending)));
            ui.separator();
            // Source frames are the bulk of what a paused write shows, so
            // they get half the viewport rather than a fixed sliver.
            let details_height = (ui.ctx().content_rect().height() * 0.5).max(280.0);
            egui::ScrollArea::vertical()
                .id_salt("preview_live_debug_details")
                .max_height(details_height)
                .show(ui, |ui| {
                    if write.positions.is_empty() {
                        ui.weak("Source position: <root>");
                    } else {
                        for position in &write.positions {
                            let collection = if position.collection.is_empty() {
                                "<current>".to_string()
                            } else {
                                position.collection.join(" / ")
                            };
                            let mut label =
                                format!("Source position: {collection} #{}", position.index);
                            if position.grouped {
                                label.push_str(" (grouped)");
                            }
                            if let Some(path) = &position.document_path {
                                label.push_str(&format!(" — {path}"));
                            }
                            ui.label(label);
                        }
                    }
                    show_primary_root_origin(ui, write.source.primary_root_origin.as_ref());
                    ui.strong("Active source frames (outer to inner)");
                    if write.source.omitted_outer_frames > 0 {
                        ui.weak(format!(
                            "{} omitted from this bounded view",
                            crate::ui_kit::plural(write.source.omitted_outer_frames, "outer frame")
                        ));
                    }
                    if write.source.frames.is_empty() {
                        ui.weak("No active source frame.");
                    }
                    for (index, frame) in write.source.frames.iter().enumerate() {
                        ui.label(format!(
                            "Frame {}: {}",
                            write.source.omitted_outer_frames + index + 1,
                            debug_value(&frame.preview)
                        ));
                        ui.indent(("debug_source_frame", index), |ui| {
                            for field in &frame.fields {
                                let suffix = if field.name_truncated { "…" } else { "" };
                                ui.monospace(format!(
                                    "{}{suffix}: {}",
                                    field.name,
                                    debug_value(&field.preview)
                                ));
                            }
                            if frame.omitted_fields > 0 {
                                ui.weak(format!(
                                    "{} omitted",
                                    crate::ui_kit::plural(frame.omitted_fields, "more field")
                                ));
                            }
                        });
                    }
                    if let Some(probe) = &write.source_field_probe {
                        let value = probe
                            .preview
                            .as_ref()
                            .map_or_else(|| "<absent>".to_owned(), debug_value);
                        ui.monospace(format!(
                            "Source probe: frame {} / {} = {value}",
                            probe.frame_from_inner, probe.field
                        ));
                    }
                    ui.separator();
                    ui.strong("Already inserted in this scope");
                    if write.draft.fields.is_empty() {
                        ui.weak("No fields yet.");
                    }
                    for field in &write.draft.fields {
                        ui.monospace(format!("{}: {}", field.name, debug_value(&field.preview)));
                    }
                    if write.draft.omitted_fields > 0 {
                        ui.weak(format!(
                            "{} omitted from this bounded view",
                            crate::ui_kit::plural(write.draft.omitted_fields, "more field")
                        ));
                    }
                });
            ui.weak("The pending value has been computed but is not yet in the target. Step inserts it and pauses before the next write.");
        }
        PreviewPhase::PausedNode(value) => show_live_node_debug_state(ui, value),
        PreviewPhase::PausedFunctionNode(value) => show_live_function_node_debug_state(ui, value),
        PreviewPhase::PausedInput(input) => show_live_input_debug_state(ui, input),
        PreviewPhase::PausedFunctionInput(input) => show_live_function_input_debug_state(ui, input),
        PreviewPhase::PausedFailure(failure) => show_live_failure_debug_state(ui, failure),
        PreviewPhase::PausedFunctionFailure(failure) => {
            show_live_function_failure_debug_state(ui, failure)
        }
    }
}

pub(in crate::app) fn show_live_node_debug_state(
    ui: &mut egui::Ui,
    value: &engine::PendingNodeValue,
) {
    ui.strong(format!("Paused after graph node #{} evaluated", value.node));
    let suffix = if value.value.truncated { "…" } else { "" };
    ui.monospace(format!(
        "Result: {}: {}{suffix}",
        value.value.value_type, value.value.preview
    ));
    show_live_expression_context(
        ui,
        &value.positions,
        value.omitted_outer_positions,
        value.position_paths_truncated,
        &value.source,
    );
    ui.weak("Step continues to the next graph evaluation. Cancel stops before output publication.");
}

pub(in crate::app) fn show_live_function_node_debug_state(
    ui: &mut egui::Ui,
    value: &engine::PendingFunctionNodeValue,
) {
    ui.strong(format!(
        "Paused after function #{} node #{} evaluated",
        value.function.get(),
        value.node
    ));
    let suffix = if value.value.truncated { "…" } else { "" };
    ui.monospace(format!(
        "Result: {}: {}{suffix}",
        value.value.value_type, value.value.preview
    ));
    show_live_expression_context(
        ui,
        &value.positions,
        value.omitted_outer_positions,
        value.position_paths_truncated,
        &value.source,
    );
    ui.weak("Positions identify the caller. Reusable function bodies have no source frames. Step continues to the next graph or function evaluation; Cancel stops before output publication.");
}

pub(in crate::app) fn show_live_input_debug_state(
    ui: &mut egui::Ui,
    input: &engine::PendingNodeInput,
) {
    ui.strong(format!(
        "Paused after node #{} reached node #{} input #{}",
        input.input,
        input.consumer,
        input.input_index + 1
    ));
    let suffix = if input.value.truncated { "…" } else { "" };
    ui.monospace(format!(
        "Delivered: {}: {}{suffix}",
        input.value.value_type, input.value.preview
    ));
    show_live_expression_context(
        ui,
        &input.positions,
        input.omitted_outer_positions,
        input.position_paths_truncated,
        &input.source,
    );
    ui.weak("Step continues to the next recorded input delivery. Cancel stops before output publication.");
}

pub(in crate::app) fn show_live_function_input_debug_state(
    ui: &mut egui::Ui,
    input: &engine::PendingFunctionNodeInput,
) {
    ui.strong(format!(
        "Paused after function #{} node #{} reached node #{} input #{}",
        input.function.get(),
        input.input,
        input.consumer,
        input.input_index + 1
    ));
    let suffix = if input.value.truncated { "…" } else { "" };
    ui.monospace(format!(
        "Delivered: {}: {}{suffix}",
        input.value.value_type, input.value.preview
    ));
    show_live_expression_context(
        ui,
        &input.positions,
        input.omitted_outer_positions,
        input.position_paths_truncated,
        &input.source,
    );
    ui.weak("Positions identify the caller. Reusable function bodies have no source frames. Step continues to the next recorded input delivery; Cancel stops before output publication.");
}

pub(in crate::app) fn show_live_failure_debug_state(
    ui: &mut egui::Ui,
    failure: &engine::PendingNodeFailure,
) {
    ui.strong(format!("Paused as graph node #{} failed", failure.node));
    let suffix = if failure.error.truncated { "…" } else { "" };
    ui.monospace(format!("Error: {}{suffix}", failure.error.preview));
    show_live_expression_context(
        ui,
        &failure.positions,
        failure.omitted_outer_positions,
        failure.position_paths_truncated,
        &failure.source,
    );
    ui.weak("Continue reports the original error. Cancel stops the run before output publication.");
}

pub(in crate::app) fn show_live_function_failure_debug_state(
    ui: &mut egui::Ui,
    failure: &engine::PendingFunctionNodeFailure,
) {
    ui.strong(format!(
        "Paused as function #{} node #{} failed",
        failure.function.get(),
        failure.node
    ));
    let suffix = if failure.error.truncated { "…" } else { "" };
    ui.monospace(format!("Error: {}{suffix}", failure.error.preview));
    show_live_expression_context(
        ui,
        &failure.positions,
        failure.omitted_outer_positions,
        failure.position_paths_truncated,
        &failure.source,
    );
    ui.weak("Positions identify the caller. Reusable function bodies have no source frames. Continue reports the original error; Cancel stops before output publication.");
}

fn show_live_expression_context(
    ui: &mut egui::Ui,
    positions: &[engine::TracePosition],
    omitted_outer_positions: usize,
    position_paths_truncated: bool,
    source: &engine::DebugSourceContext,
) {
    show_primary_root_origin(ui, source.primary_root_origin.as_ref());
    if omitted_outer_positions > 0 {
        ui.weak(format!(
            "{} omitted",
            crate::ui_kit::plural(omitted_outer_positions, "outer position")
        ));
    }
    if position_paths_truncated {
        ui.weak("Some position paths were shortened in this bounded view.");
    }
    for position in positions {
        let collection = if position.collection.is_empty() {
            "<current>".to_owned()
        } else {
            position.collection.join(" / ")
        };
        ui.label(format!("Source position: {collection} #{}", position.index));
    }
    ui.strong("Active source frames (outer to inner)");
    if source.frames.is_empty() && source.omitted_outer_frames == 0 {
        ui.weak("No active source frame was recorded at this point.");
    }
    if source.omitted_outer_frames > 0 {
        ui.weak(format!(
            "{} omitted",
            crate::ui_kit::plural(source.omitted_outer_frames, "outer frame")
        ));
    }
    for (index, frame) in source.frames.iter().enumerate() {
        ui.label(format!(
            "Frame {}: {}",
            source.omitted_outer_frames + index + 1,
            debug_value(&frame.preview)
        ));
        ui.indent(("debug_node_source_frame", index), |ui| {
            for field in &frame.fields {
                ui.monospace(format!("{}: {}", field.name, debug_value(&field.preview)));
            }
            if frame.omitted_fields > 0 {
                ui.weak(format!(
                    "{} omitted",
                    crate::ui_kit::plural(frame.omitted_fields, "more field")
                ));
            }
        });
    }
}

fn debug_value(value: &engine::DebugInstancePreview) -> String {
    if let Some(scalar) = &value.value {
        let suffix = if scalar.truncated { "…" } else { "" };
        format!("{}: {}{suffix}", scalar.value_type, scalar.preview)
    } else {
        instance_kind_label(value.kind, value.length)
    }
}

/// Names a non-scalar value by its shape and immediate size, as in
/// "group of 3 fields"; the engine never copies nested contents.
fn instance_kind_label(kind: engine::TraceOutputKind, length: Option<usize>) -> String {
    use engine::TraceOutputKind;
    let (shape, singular, plural) = match kind {
        TraceOutputKind::Scalar => ("value", "item", "items"),
        TraceOutputKind::Group => ("group", "field", "fields"),
        TraceOutputKind::Repeated => ("list", "item", "items"),
        TraceOutputKind::MappedSequence => ("sequence", "item", "items"),
        TraceOutputKind::DocumentSet => ("document set", "document", "documents"),
    };
    match length {
        Some(1) => format!("{shape} of 1 {singular}"),
        Some(length) => format!("{shape} of {length} {plural}"),
        None => shape.to_owned(),
    }
}

/// Says how a pending target field gets its value, by graph node.
fn binding_label(binding: engine::TraceTargetFieldBinding) -> String {
    use engine::TraceTargetFieldBinding;
    match binding {
        TraceTargetFieldBinding::StaticBinding { value } => format!("value of node #{value}"),
        TraceTargetFieldBinding::DynamicBinding { key, value } => {
            format!("value of node #{value}, field named by node #{key}")
        }
        TraceTargetFieldBinding::StaticChild => "nested group".to_owned(),
        TraceTargetFieldBinding::DynamicChild { key } => {
            format!("nested group named by node #{key}")
        }
    }
}

fn annotation_text(value: &engine::DebugAnnotationText) -> String {
    format!(
        "{}{}",
        value.preview,
        if value.truncated { "…" } else { "" }
    )
}

/// Shows the primary root's XML type annotation. Only the XML reader records
/// one, so a source without that fact (JSON, CSV, EDI, an XML root without
/// type alternatives, or a non-group root) shows no XML section at all.
fn show_primary_root_origin(ui: &mut egui::Ui, origin: Option<&engine::DebugPrimaryRootOrigin>) {
    use engine::DebugPrimaryRootOrigin;
    let Some(origin) = origin else {
        return;
    };
    if matches!(
        origin,
        DebugPrimaryRootOrigin::NotGroup { .. } | DebugPrimaryRootOrigin::Unknown
    ) {
        return;
    }
    ui.strong("Primary source XML annotation");
    match origin {
        DebugPrimaryRootOrigin::NotGroup { .. } | DebugPrimaryRootOrigin::Unknown => {}
        DebugPrimaryRootOrigin::Absent => {
            ui.weak("The XML root had no type annotation.");
        }
        DebugPrimaryRootOrigin::Explicit { identity } => {
            ui.monospace(format!("Type: {}", annotation_text(identity)));
        }
        DebugPrimaryRootOrigin::ExplicitPadded {
            literal,
            resolved_identity,
        } => {
            ui.monospace(format!("Annotation text: {:?}", annotation_text(literal)));
            ui.monospace(format!(
                "Resolved type: {}",
                annotation_text(resolved_identity)
            ));
            ui.weak("The padded annotation does not select a type view.");
        }
    }
}

#[cfg(test)]
mod root_annotation_render_tests {
    use super::*;
    use crate::test_ui::ContextExt as _;
    use engine::{DebugAnnotationText, DebugPrimaryRootOrigin};

    fn render(origin: Option<&DebugPrimaryRootOrigin>) -> String {
        fn collect(shape: &egui::epaint::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, texts);
                    }
                }
                _ => {}
            }
        }
        let context = egui::Context::default();
        let output = context.run_test_ui(Default::default(), |ui| {
            show_primary_root_origin(ui, origin)
        });
        let mut texts = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut texts);
        }
        texts.join("\n")
    }

    #[test]
    fn annotation_panel_shows_only_retained_xml_annotation_facts() {
        let absent = render(Some(&DebugPrimaryRootOrigin::Absent));
        let explicit = render(Some(&DebugPrimaryRootOrigin::Explicit {
            identity: DebugAnnotationText {
                preview: "{urn:root}Derived".into(),
                truncated: false,
            },
        }));
        let padded = render(Some(&DebugPrimaryRootOrigin::ExplicitPadded {
            literal: DebugAnnotationText {
                preview: "  t:Derived\t".into(),
                truncated: false,
            },
            resolved_identity: DebugAnnotationText {
                preview: "{urn:root}Derived".into(),
                truncated: false,
            },
        }));
        assert!(absent.contains("Primary source XML annotation"));
        assert!(absent.contains("had no type annotation"));
        assert!(explicit.contains("Type: {urn:root}Derived"));
        assert!(padded.contains("Annotation text: \"  t:Derived\\t\""));
        assert!(padded.contains("Resolved type: {urn:root}Derived"));
        assert!(padded.contains("does not select a type view"));
        // JSON, CSV and other non-XML sources retain no annotation fact.
        assert!(render(Some(&DebugPrimaryRootOrigin::Unknown)).is_empty());
        assert!(
            render(Some(&DebugPrimaryRootOrigin::NotGroup {
                kind: engine::TraceOutputKind::Repeated,
            }))
            .is_empty()
        );
        assert!(render(None).is_empty());
    }

    #[test]
    fn bounded_annotation_panel_marks_truncation_without_changing_the_snapshot() {
        let fact = DebugPrimaryRootOrigin::Explicit {
            identity: DebugAnnotationText {
                preview: "é".repeat(160),
                truncated: true,
            },
        };
        let before = fact.clone();
        assert!(render(Some(&fact)).contains(&format!("{}…", "é".repeat(160))));
        assert_eq!(fact, before);
    }
}

#[cfg(test)]
mod value_label_tests {
    use super::*;
    use crate::test_ui::ContextExt as _;
    use engine::{TraceOutputKind, TraceTargetFieldBinding};

    #[test]
    fn every_binding_kind_has_a_readable_label() {
        let labels = [
            (
                TraceTargetFieldBinding::StaticBinding { value: 11 },
                "value of node #11",
            ),
            (
                TraceTargetFieldBinding::DynamicBinding { key: 3, value: 4 },
                "value of node #4, field named by node #3",
            ),
            (TraceTargetFieldBinding::StaticChild, "nested group"),
            (
                TraceTargetFieldBinding::DynamicChild { key: 5 },
                "nested group named by node #5",
            ),
        ];
        for (binding, expected) in labels {
            assert_eq!(binding_label(binding), expected);
        }
    }

    #[test]
    fn every_value_kind_names_its_shape_and_size() {
        let labels = [
            (TraceOutputKind::Scalar, "value", "value of 2 items"),
            (TraceOutputKind::Group, "group", "group of 2 fields"),
            (TraceOutputKind::Repeated, "list", "list of 2 items"),
            (
                TraceOutputKind::MappedSequence,
                "sequence",
                "sequence of 2 items",
            ),
            (
                TraceOutputKind::DocumentSet,
                "document set",
                "document set of 2 documents",
            ),
        ];
        for (kind, unsized_label, sized) in labels {
            assert_eq!(instance_kind_label(kind, None), unsized_label);
            assert_eq!(instance_kind_label(kind, Some(2)), sized);
        }
        assert_eq!(
            instance_kind_label(TraceOutputKind::Group, Some(1)),
            "group of 1 field"
        );
        assert_eq!(
            instance_kind_label(TraceOutputKind::Repeated, Some(0)),
            "list of 0 items"
        );
    }

    fn scalar(value_type: &'static str, preview: &str) -> engine::DebugInstancePreview {
        engine::DebugInstancePreview {
            kind: TraceOutputKind::Scalar,
            value: Some(engine::TraceValue {
                value_type,
                preview: preview.into(),
                truncated: false,
            }),
            length: None,
        }
    }

    fn paused_write() -> engine::PendingTargetWrite {
        engine::PendingTargetWrite {
            scope: engine::TraceScope {
                source_scope: mapping::SourceScopeLocation::root(
                    mapping::SourceScopeTarget::Primary,
                ),
                target: engine::TraceTarget::Primary,
                target_path: vec!["Invoices".into()],
                structural_path: vec![0],
            },
            positions: Vec::new(),
            source: engine::DebugSourceContext {
                frames: vec![engine::DebugSourceFrame {
                    preview: engine::DebugInstancePreview {
                        kind: TraceOutputKind::Group,
                        value: None,
                        length: Some(3),
                    },
                    fields: vec![engine::DebugDraftField {
                        name: "Customer".into(),
                        name_truncated: false,
                        preview: scalar("string", "ACME"),
                    }],
                    omitted_fields: 0,
                }],
                omitted_outer_frames: 0,
                primary_root_origin: Some(engine::DebugPrimaryRootOrigin::Unknown),
            },
            source_field_probe: None,
            field: "Sequence".into(),
            field_truncated: false,
            binding: TraceTargetFieldBinding::StaticBinding { value: 11 },
            pending: scalar("int", "1"),
            draft: engine::DebugScopeDraft {
                fields: Vec::new(),
                omitted_fields: 0,
            },
        }
    }

    #[test]
    fn paused_write_reads_as_sentences_without_internal_representations() {
        fn collect(shape: &egui::epaint::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, texts);
                    }
                }
                _ => {}
            }
        }
        let phase = PreviewPhase::Paused(Box::new(paused_write()));
        let context = egui::Context::default();
        let output = context.run_test_ui(Default::default(), |ui| {
            show_live_debug_state(ui, &phase, true)
        });
        let mut texts = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &mut texts);
        }
        let text = texts.join("\n");
        assert!(text.contains("Binding: value of node #11"), "{text}");
        assert!(text.contains("Frame 1: group of 3 fields"), "{text}");
        assert!(text.contains("Customer: string: ACME"), "{text}");
        assert!(!text.contains("StaticBinding"), "{text}");
        assert!(!text.contains("immediate item"), "{text}");
        assert!(!text.contains("XML annotation"), "{text}");
    }
}
