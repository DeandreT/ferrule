use super::projects as csv_projects;
use super::*;
#[path = "../../../../conformance/primary-fixed-width-documents/projects.rs"]
mod fixed_projects;

#[test]
fn fixed_width_document_projects_run_preview_and_save() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let mut app = FerruleApp::default();
    for model in fixed_projects::MODELS {
        let document = fixed_projects::document(model);
        let path = directory.0.join(format!("{model}.json"));
        std::fs::write(
            &path,
            mapping::document_project_file::encode_pretty(&document)?,
        )?;
        app.load_project_from(&path);
        let copy = directory.0.join(format!("{model}-copy.json"));
        app.save_document_to(&copy)?;
        assert_eq!(
            mapping::document_project_file::decode_bytes(&std::fs::read(copy)?)?.primary_source(),
            document.primary_source()
        );
    }
    let documents = directory.0.join("input");
    std::fs::create_dir_all(&documents)?;
    std::fs::write(documents.join("B.dat"), "a_one_2_\r\n")?;
    std::fs::write(documents.join("a.dat"), "a_two_3_\n")?;
    let path = directory.0.join("project.json");
    std::fs::write(
        &path,
        mapping::document_project_file::encode_pretty(&fixed_projects::document(
            "parameter_wildcard",
        ))?,
    )?;
    app.load_project_from(&path);
    for (name, value) in [("Input", "*.dat"), ("Status", "ok")] {
        app.host_parameters.entries.push(HostParameterEntry {
            name: name.into(),
            value: value.into(),
        });
    }
    app.primary_document.as_mut().unwrap().input_root = documents.display().to_string();
    let context = egui::Context::default();
    app.begin_preview();
    app.execute_preview();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_preview.is_some() && Instant::now() < deadline {
        app.poll_preview(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_preview.is_none(), "{}", app.status);
    let report = app.run_report.as_mut().unwrap();
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("text preview")
    };
    assert_eq!(content.as_str(), "a_one+two_____5__ok__\n");
    app.run_saved(false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_file_run.is_some() && Instant::now() < deadline {
        app.poll_file_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_file_run.is_none(), "{}", app.status);
    assert_eq!(
        std::fs::read_to_string(directory.0.join("never-preview-output.dat"))?,
        "a_one+two_____5__ok__\n"
    );
    Ok(())
}
