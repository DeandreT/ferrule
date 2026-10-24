use super::*;

#[path = "../../../../conformance/primary-json-documents/projects.rs"]
mod json_projects;

#[test]
fn json_document_projects_keep_their_saved_plans() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let mut app = FerruleApp::default();
    for model in json_projects::MODELS {
        let document = json_projects::document(model);
        let path = directory.0.join(format!("{model}.json"));
        std::fs::write(
            &path,
            mapping::document_project_file::encode_pretty(&document)?,
        )?;
        app.load_project_from(&path);
        let copy = directory.0.join(format!("{model}-copy.json"));
        app.save_document_to(&copy)?;
        let saved = mapping::document_project_file::decode_bytes(&std::fs::read(copy)?)?;
        assert_eq!(saved.primary_source(), document.primary_source());
    }
    Ok(())
}

#[test]
fn json_document_projects_run_and_preview_one_document_set() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let documents = directory.0.join("documents/in");
    std::fs::create_dir_all(&documents)?;
    std::fs::write(
        documents.join("a.json"),
        r#"{"Order":[{"Key":"k1","Text":"c","Count":3}]}"#,
    )?;
    std::fs::write(
        documents.join("B.json"),
        r#"{"Order":[{"Key":"k2","Text":"b","Count":20},{"Key":"k1","Text":"a","Count":1}]}"#,
    )?;
    let path = directory.0.join("orders.json");
    std::fs::write(
        &path,
        mapping::document_project_file::encode_pretty(&json_projects::document(
            "parameter_wildcard",
        ))?,
    )?;
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    for (name, value) in [("Input", "in/*.json"), ("Status", "ok")] {
        app.host_parameters.entries.push(HostParameterEntry {
            name: name.into(),
            value: value.into(),
        });
    }
    app.primary_document
        .as_mut()
        .expect("document session")
        .input_root = directory.0.join("documents").display().to_string();
    let expected = serde_json::json!({"Group":[
        {"Key":"k2","Text":"b","Count":20,"Status":"ok"},
        {"Key":"k1","Text":"a+c","Count":4,"Status":"ok"}
    ]});
    let context = egui::Context::default();
    app.begin_preview();
    app.execute_preview();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_preview.is_some() && Instant::now() < deadline {
        app.poll_preview(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_preview.is_none(), "{}", app.status);
    let report = app.run_report.as_mut().expect("preview report");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON preview")
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content.as_str())?,
        expected
    );
    let output = directory.0.join("never-preview-output.json");
    assert!(!output.exists());
    app.run_saved(false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_file_run.is_some() && Instant::now() < deadline {
        app.poll_file_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_file_run.is_none(), "{}", app.status);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(output)?)?,
        expected
    );
    Ok(())
}
