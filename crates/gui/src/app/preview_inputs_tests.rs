use super::*;

fn app_with_preview_value(value: &str) -> FerruleApp {
    let mut app = FerruleApp {
        project: run_value_project(),
        ..FerruleApp::default()
    };
    if let Some(Node::RuntimeParameterDefault { preview, .. }) = app.project.graph.nodes.get_mut(&1)
    {
        *preview = Some(value.into());
    }
    app.preview_draft = Some(preview_draft());
    app
}

fn preview_draft() -> crate::preview::PreviewDraft {
    crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: "preview.json".into(),
        input_text: r#"{"Value":"source"}"#.into(),
        debug_breakpoint: None,
    }
}

fn preview_contains(app: &mut FerruleApp, value: &str) -> bool {
    let report = app.run_report.as_mut().expect("preview produces a report");
    matches!(
        report.report.outputs[0].preview(),
        crate::run_report::OutputPreview::Text { content, .. } if content.contains(value)
    )
}

#[test]
fn saved_preview_is_used_before_default_and_supplied_value_wins() {
    let mut app = app_with_preview_value("saved preview");
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(preview_contains(&mut app, "saved preview"));

    app.host_parameters
        .entries
        .push(host_parameters::HostParameterEntry {
            name: "choice".into(),
            value: "supplied value".into(),
        });
    app.preview_draft = Some(preview_draft());
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(preview_contains(&mut app, "supplied value"));
}

#[test]
fn required_input_preview_does_not_supply_a_normal_file_run() -> anyhow::Result<()> {
    let project_path = temporary_project_path("required-preview-file-run");
    let directory = project_path.parent().unwrap();
    let input_path = directory.join("input.json");
    let output_path = directory.join("output.json");
    std::fs::write(&input_path, r#"{"Value":"source"}"#)?;
    let mut app = app_with_preview_value("saved preview");
    app.project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "choice".into(),
            ty: ScalarType::String,
            preview: Some("saved preview".into()),
        },
    );
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(preview_contains(&mut app, "saved preview"));

    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.save_document_to(&project_path)?;
    app.input_path = input_path.display().to_string();
    app.output_path = output_path.display().to_string();
    app.run(&egui::Context::default());
    wait_for_file_run_completion(&mut app);
    assert!(!output_path.exists());
    assert!(
        app.diagnostics.items().iter().any(
            |item| item.message.contains("choice") && item.message.contains("does not provide")
        ),
        "{}",
        app.status
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn debug_preview_pauses_with_saved_preview_value() {
    let mut app = app_with_preview_value("debug saved preview");
    app.execute_debug_preview();
    let pending = wait_for_debug_pause(&mut app);
    assert_eq!(pending.field, "Value");
    assert_eq!(
        pending.pending.value.as_ref().unwrap().preview,
        "debug saved preview"
    );
    app.preview_command(preview_ui::PreviewCommand::Continue);
    wait_for_preview_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(preview_contains(&mut app, "debug saved preview"));
}

#[test]
fn malformed_preview_reports_type_failure_before_output() {
    let mut app = app_with_preview_value("not an integer");
    app.project.target =
        SchemaNode::group("Record", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Int(7),
        },
    );
    if let Some(Node::RuntimeParameterDefault { ty, .. }) = app.project.graph.nodes.get_mut(&1) {
        *ty = ScalarType::Int;
    }
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.run_report.is_none());
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("choice")
                && item.message.contains("expected Int, got string")),
        "{}",
        app.status
    );
}
