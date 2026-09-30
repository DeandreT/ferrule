use super::*;

fn preview_value(output: &mut crate::run_report::RunOutput) -> anyhow::Result<String> {
    let crate::run_report::OutputPreview::Text { content, .. } = output.preview() else {
        anyhow::bail!("pipeline artifact is not a text preview");
    };
    let value: serde_json::Value = serde_json::from_str(content)?;
    Ok(value["Value"].as_str().unwrap_or_default().to_owned())
}

#[test]
fn stored_preview_identity_never_selects_run_publication() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-preview-stored-path");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().unwrap();
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    pipeline.stages[0].mapping_path = Some("designs/first.mfd".into());
    pipeline.stages[0].project.target_path = Some("outputs/result.json".into());
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;

    let mut draft = crate::pipeline_run::PipelineRunDraft::load(&pipeline_path)?;
    draft.inputs[0].path = "orders.json".into();
    assert_eq!(draft.outputs.len(), 1);
    assert_eq!(
        draft.outputs[0].preview_path,
        directory.join("outputs/result.json").display().to_string()
    );
    assert!(draft.outputs[0].path.is_empty());
    assert_eq!(draft.preview_paths()?.len(), 1);
    assert!(
        draft
            .requests()
            .unwrap_err()
            .to_string()
            .contains("choose at least one output")
    );

    draft.outputs[0].preview_path.clear();
    let error = draft.preview_paths().unwrap_err().to_string();
    assert!(error.contains("prepare / Primary needs a preview format path"));
    assert!(draft.outputs[0].path.is_empty());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn preview_host_loader_requires_bounded_local_files() -> anyhow::Result<()> {
    let input = cli::PipelineHostFile {
        name: "orders".into(),
        path: PathBuf::from("https://example.invalid/orders.json"),
    };
    let error = crate::pipeline_run::load_preview_hosts(&[input], || false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("local file for host input `orders`"));

    let directory = temporary_project_path("pipeline-preview-host-directory");
    let input = cli::PipelineHostFile {
        name: "orders".into(),
        path: directory.parent().unwrap().to_path_buf(),
    };
    let error = crate::pipeline_run::load_preview_hosts(&[input], || false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a regular file"));
    std::fs::remove_dir_all(directory.parent().unwrap())?;
    Ok(())
}

#[test]
fn unsupported_preview_output_fails_before_host_file_read() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-preview-preflight");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().unwrap();
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    pipeline.stages[0].project.target_options.json_document = false;
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.inputs[0].path = "missing-input.json".into();
    draft.outputs[0].preview_path = "database.sqlite".into();

    app.start_pipeline_preview();
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline preview failed");
    assert!(app.diagnostics.items().iter().any(|item| {
        item.message.contains("SQLite") && !item.message.contains("missing-input.json")
    }));
    assert!(!directory.join("database.sqlite").exists());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn preview_reports_every_stage_and_named_target_without_writing_files() -> anyhow::Result<()> {
    let pipeline_path = temporary_project_path("pipeline-preview-all-stages");
    two_stage_pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().unwrap();
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    let prepare = &mut pipeline.stages[0].project;
    prepare.extra_targets.push(NamedTarget {
        name: "selected".into(),
        path: None,
        schema: prepare.target.clone(),
        options: prepare.target_options.clone(),
        root: prepare.root.clone(),
    });
    pipeline.stages[1].source = mapping::PipelineInput::StageTarget {
        stage: "prepare".into(),
        target: Some("selected".into()),
    };
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;

    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    for output in &mut draft.outputs {
        let target = output.target.as_deref().unwrap_or("primary");
        output.preview_path = format!("{}-{target}.json", output.stage);
        assert!(output.path.is_empty());
    }

    app.start_pipeline_preview();
    wait_for_pipeline_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    let report = &mut app.run_report.as_mut().expect("preview report").report;
    assert_eq!(
        report.kind,
        crate::run_report::RunReportKind::PipelinePreview
    );
    assert_eq!(report.outputs.len(), 3);
    assert_eq!(report.outputs[0].name, "prepare / Primary");
    assert_eq!(report.outputs[1].name, "prepare / selected");
    assert_eq!(report.outputs[2].name, "finish / Primary");
    assert_eq!(preview_value(&mut report.outputs[0])?, "A");
    assert_eq!(preview_value(&mut report.outputs[1])?, "A");
    assert_eq!(preview_value(&mut report.outputs[2])?, "B");
    assert_eq!(report.trace.stages, ["prepare", "finish"]);
    assert_eq!(report.trace.event_stages.len(), report.trace.events.len());
    for name in [
        "prepare-primary.json",
        "prepare-selected.json",
        "finish-primary.json",
    ] {
        assert!(!directory.join(name).exists(), "Preview wrote {name}");
    }
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn preview_uses_saved_value_then_supplied_host_while_run_uses_runtime_default() -> anyhow::Result<()>
{
    let pipeline_path = temporary_project_path("pipeline-preview-host-contract");
    pipeline_fixture(&pipeline_path)?;
    let directory = pipeline_path.parent().unwrap();
    std::fs::write(directory.join("orders.json"), r#"{"Value":"source"}"#)?;
    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    let copy_project = pipeline.stages[0].project.clone();
    let mut prepare = run_value_project();
    if let Some(Node::RuntimeParameterDefault { preview, .. }) = prepare.graph.nodes.get_mut(&1) {
        *preview = Some("design preview".into());
    }
    pipeline.stages[0].project = prepare;
    pipeline.stages.push(mapping::PipelineStage {
        id: "finish".into(),
        mapping_path: None,
        project: copy_project,
        source: mapping::PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        },
        extra_sources: Vec::new(),
    });
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;

    let mut app = FerruleApp::default();
    for supplied in [None, Some("host override")] {
        app.load_pipeline_for_run(&pipeline_path);
        let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
        assert!(draft.issues.is_empty(), "{:?}", draft.issues);
        draft.inputs[0].path = "orders.json".into();
        for output in &mut draft.outputs {
            output.preview_path = format!("{}.json", output.stage);
        }
        if let Some(value) = supplied {
            draft
                .host_parameters
                .entries
                .push(host_parameters::HostParameterEntry {
                    name: "choice".into(),
                    value: value.into(),
                });
        }
        app.start_pipeline_preview();
        wait_for_pipeline_completion(&mut app);
        assert!(app.diagnostics.is_empty(), "{}", app.status);
        let report = &mut app.run_report.as_mut().expect("preview report").report;
        let expected = supplied.unwrap_or("design preview");
        assert_eq!(preview_value(&mut report.outputs[0])?, expected);
        assert_eq!(preview_value(&mut report.outputs[1])?, expected);
        assert!(!directory.join("prepare.json").exists());
        assert!(!directory.join("finish.json").exists());
    }

    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[1].path = "published.json".into();
    app.start_pipeline_run();
    wait_for_pipeline_completion(&mut app);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert_eq!(
        selected_run_value(&directory.join("published.json"))?,
        "fallback"
    );
    assert!(!directory.join("prepare.json").exists());
    assert!(!directory.join("finish.json").exists());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn debug_preview_steps_across_stages_and_cancel_never_publishes() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-debug-preview")?;
    let directory = pipeline_path.parent().unwrap();
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.outputs[0].preview_path = "prepare.json".into();
    draft.outputs[1].preview_path = "finish.json".into();

    app.start_pipeline_debug_preview();
    let (stage, _) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "prepare");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline preview cancelled");
    assert!(!app.show_run_report);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );

    app.start_pipeline_debug_preview();
    let (stage, _) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "prepare");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Step);
    let (stage, _) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "finish");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Continue);
    wait_for_pipeline_completion(&mut app);
    let report = &mut app
        .run_report
        .as_mut()
        .expect("debug preview report")
        .report;
    assert_eq!(
        report.kind,
        crate::run_report::RunReportKind::PipelinePreview
    );
    assert_eq!(preview_value(&mut report.outputs[0])?, "A");
    assert_eq!(preview_value(&mut report.outputs[1])?, "B");
    assert_eq!(report.trace.stages, ["prepare", "finish"]);
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn later_stage_preview_failure_returns_no_report_or_files() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_failure_pipeline_app("pipeline-preview-failure")?;
    let directory = pipeline_path.parent().unwrap();
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.outputs[0].preview_path = "prepare.json".into();
    draft.outputs[1].preview_path = "finish.json".into();

    app.start_pipeline_preview();
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline preview failed");
    assert!(app.run_report.is_none());
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn failed_preview_retry_clears_an_earlier_success_report() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-preview-stale-report")?;
    let directory = pipeline_path.parent().unwrap();
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.outputs[0].preview_path = "prepare.json".into();
    draft.outputs[1].preview_path = "finish.json".into();
    app.start_pipeline_preview();
    wait_for_pipeline_completion(&mut app);
    assert!(app.run_report.is_some());

    let mut pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&pipeline_path)?)?;
    let finish = &mut pipeline.stages[1].project;
    attach_divide_failure(finish);
    finish.root.bindings[0].node = 2;
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline reopens");
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].preview_path = "prepare.json".into();
    draft.outputs[1].preview_path = "finish.json".into();
    app.start_pipeline_preview();
    assert!(app.run_report.is_none());
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline preview failed");
    assert!(app.run_report.is_none());
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn cancelled_run_retry_clears_an_earlier_success_report() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-run-stale-report")?;
    let directory = pipeline_path.parent().unwrap();
    app.start_pipeline_run();
    wait_for_pipeline_completion(&mut app);
    assert!(app.run_report.is_some());
    assert_eq!(selected_run_value(&directory.join("prepare.json"))?, "A");

    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline reopens");
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "prepare.json".into();
    draft.outputs[1].path = "finish.json".into();
    app.start_pipeline_debug_run();
    assert!(app.run_report.is_none());
    let (stage, _) = wait_for_pipeline_pause(&mut app);
    assert_eq!(stage, "prepare");
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert!(app.run_report.is_none());
    assert_eq!(selected_run_value(&directory.join("prepare.json"))?, "A");
    assert_eq!(selected_run_value(&directory.join("finish.json"))?, "B");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
