use super::*;
#[path = "../../../../conformance/primary-edi-documents/projects.rs"]
mod edi_projects;

#[test]
fn edi_document_projects_save_preview_and_run_all_writable_families() -> anyhow::Result<()> {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/primary-edi-documents/cases.json"
    ))?;
    let directory = TestDir::new()?;
    let context = egui::Context::default();
    for case in cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(index, case)| *index < 10 || case["id"] == "P28")
        .map(|(_, case)| case)
    {
        let root = directory.0.join(case["id"].as_str().unwrap());
        std::fs::create_dir_all(&root)?;
        for (name, value) in case["files"].as_object().unwrap() {
            let file = root.join(name);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(file, value.as_str().unwrap())?;
        }
        let model = case["model"].as_u64().unwrap() as usize;
        let document = edi_projects::document(edi_projects::MODELS[model]);
        let path = root.join("project.json");
        std::fs::write(
            &path,
            mapping::document_project_file::encode_pretty(&document)?,
        )?;
        let mut app = FerruleApp::default();
        app.load_project_from(&path);
        let saved = root.join("saved.json");
        app.save_document_to(&saved)?;
        assert_eq!(
            mapping::document_project_file::decode_bytes(&std::fs::read(saved)?)?.primary_source(),
            document.primary_source()
        );
        for (name, value) in case["parameters"].as_object().unwrap() {
            app.host_parameters.entries.push(HostParameterEntry {
                name: name.clone(),
                value: value.as_str().unwrap().into(),
            });
        }
        app.primary_document.as_mut().unwrap().input_root = root.display().to_string();
        app.begin_preview();
        app.execute_preview();
        let deadline = Instant::now() + Duration::from_secs(60);
        while app.pending_preview.is_some() && Instant::now() < deadline {
            app.poll_preview(&context);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.pending_preview.is_none(), "{}", app.status);
        let report = app.run_report.as_mut().unwrap();
        let crate::run_report::OutputPreview::Text { content, .. } =
            report.report.outputs[0].preview()
        else {
            panic!("EDI text preview")
        };
        assert_eq!(
            content.as_str(),
            case["expected"]["output"].as_str().unwrap()
        );
        app.run_saved(false);
        let deadline = Instant::now() + Duration::from_secs(60);
        while app.pending_file_run.is_some() && Instant::now() < deadline {
            app.poll_file_run(&context);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.pending_file_run.is_none(), "{}", app.status);
        assert_eq!(
            std::fs::read_to_string(root.join("never-preview-output.edi"))?,
            case["expected"]["output"].as_str().unwrap()
        );
    }
    Ok(())
}
