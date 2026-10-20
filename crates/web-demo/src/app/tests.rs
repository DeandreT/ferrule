use mapping::XbrlBoundaryOptions;

use super::*;

#[test]
fn failed_project_serialization_clears_the_prior_download() {
    let mut app = DemoApp::new();
    assert!(!app.project_json.is_empty());
    app.project.graph.nodes.insert(
        999,
        mapping::Node::Const {
            value: ir::Value::Float(f64::INFINITY),
        },
    );
    app.sync_project_json();
    assert!(app.project_json.is_empty());
    assert_eq!(app.status, "Project serialization failed");
    let diagnostic = app.diagnostic.clone();
    assert!(diagnostic.is_some());
    app.download_project();
    assert_eq!(app.status, "Project download unavailable");
    assert_eq!(app.diagnostic, diagnostic);
}

#[test]
fn demo_project_runs_on_the_sample_input() {
    let mut app = DemoApp::new();
    app.run();
    let output = app.output.as_ref().map_or("", MappedOutput::text);
    assert!(output.contains("<AllIds>A-1, B-2</AllIds>"), "{output}");
    assert!(output.contains("<ItemCount>2</ItemCount>"), "{output}");
    assert!(output.contains("<Total>10</Total>"), "{output}");
}

fn xml_result(app: &mut DemoApp) -> String {
    app.run();
    assert_eq!(app.status, "Mapping completed");
    let (filename, text) = app.output_download().expect("successful run");
    assert_eq!(filename, "mapped-output.xml");
    assert!(text.starts_with("<?xml"), "{text}");
    text.to_owned()
}

#[test]
fn csv_refusal_after_xml_success_keeps_the_xml_result_and_its_provenance() {
    let mut app = DemoApp::new();
    let xml = xml_result(&mut app);
    assert_eq!(app.output_freshness(), Some(OutputFreshness::Current));
    assert_eq!(app.output_heading(), "Output (XML)");

    // The output format picker marks the run inputs changed.
    app.target_format = DataFormat::Csv;
    app.run_pending = true;
    assert_eq!(app.output_freshness(), Some(OutputFreshness::InputsChanged));
    assert_eq!(
        app.output_heading(),
        "Output (XML, stale: inputs changed since this run)"
    );
    assert_eq!(
        app.output_download(),
        Some(("mapped-output.xml".to_owned(), xml.as_str()))
    );

    app.run();
    assert_eq!(app.status, "Run failed");
    assert_eq!(
        app.diagnostic.as_deref(),
        Some(
            runtime::RuntimeError::CsvTargetNotRepeated
                .to_string()
                .as_str()
        )
    );
    assert_eq!(app.output_freshness(), Some(OutputFreshness::RunFailed));
    assert_eq!(
        app.output_heading(),
        "Output (XML, stale: last successful run; the latest run failed)"
    );
    assert_eq!(
        app.output_download(),
        Some(("mapped-output.xml".to_owned(), xml.as_str()))
    );

    app.target_format = DataFormat::Json;
    app.run_pending = true;
    app.run();
    let (filename, json) = app.output_download().expect("successful JSON run");
    assert_eq!(filename, "mapped-output.json");
    assert!(json.contains("\"AllIds\": \"A-1, B-2\""), "{json}");
    assert_eq!(app.output_heading(), "Output (JSON)");
}

#[test]
fn failed_first_run_offers_no_download() {
    let mut app = DemoApp::new();
    app.target_format = DataFormat::Csv;
    app.run();
    assert_eq!(app.status, "Run failed");
    assert_eq!(app.output_freshness(), None);
    assert_eq!(app.output_heading(), "Output (no result)");
    assert_eq!(app.output_download(), None);
    app.download_output();
    assert_eq!(app.status, "Output download unavailable");
}

#[test]
fn live_off_format_change_presents_the_retained_result_as_stale() {
    let mut app = DemoApp::new();
    let context = egui::Context::default();
    history_tests::settle(&mut app, &context);
    let (_, xml) = app.output_download().expect("live sample run");
    let xml = xml.to_owned();
    app.live_run = false;
    app.target_format = DataFormat::Json;
    app.run_pending = true;

    let painted = history_tests::painted(&history_tests::settle(&mut app, &context));
    let labels: Vec<&str> = painted.iter().map(|(text, _, _)| text.as_str()).collect();
    assert!(labels.contains(&"Output (XML, stale: inputs changed since this run)"));
    assert!(labels.contains(&"Download stale output"));
    assert!(!labels.contains(&"Output (JSON)"));
    assert!(app.run_pending, "live-off frames do not run");
    assert_eq!(
        app.output_download(),
        Some(("mapped-output.xml".to_owned(), xml.as_str()))
    );

    app.live_run = true;
    let painted = history_tests::painted(&history_tests::settle(&mut app, &context));
    let labels: Vec<&str> = painted.iter().map(|(text, _, _)| text.as_str()).collect();
    assert!(labels.contains(&"Output (JSON)"));
    assert!(labels.contains(&"Download output"));
    assert_eq!(
        app.output_download().map(|(filename, _)| filename),
        Some("mapped-output.json".to_owned())
    );
}

#[test]
fn project_undo_and_redo_are_independent_of_output_lifetime() {
    let mut app = DemoApp::new();
    let xml = xml_result(&mut app);
    let original = project_document::to_json(&app.project).unwrap();
    let mut edited = demo_project();
    edited.graph.nodes.insert(
        1,
        mapping::Node::Const {
            value: ir::Value::String(" | ".into()),
        },
    );
    let edited = project_document::to_json(&edited).unwrap();
    app.project_json = edited.clone();
    app.apply_project_json();
    assert_eq!(app.status, "Project applied");
    assert_eq!(app.output_freshness(), Some(OutputFreshness::InputsChanged));

    app.target_format = DataFormat::Csv;
    app.run();
    assert_eq!(app.status, "Run failed");
    app.restore_project(false);
    assert_eq!(app.status, "Project undone");
    assert_eq!(project_document::to_json(&app.project).unwrap(), original);
    assert_eq!(
        app.output_download(),
        Some(("mapped-output.xml".to_owned(), xml.as_str()))
    );
    assert_eq!(app.output_freshness(), Some(OutputFreshness::InputsChanged));
    app.restore_project(true);
    assert_eq!(app.status, "Project redone");
    assert_eq!(project_document::to_json(&app.project).unwrap(), edited);

    app.target_format = DataFormat::Xml;
    let rerun = xml_result(&mut app);
    assert!(rerun.contains("<AllIds>A-1 | B-2</AllIds>"), "{rerun}");
    assert_eq!(app.output_freshness(), Some(OutputFreshness::Current));
    assert!(app.history.undo_json().is_some());
}

#[test]
fn project_boundaries_select_xbrl_without_leaking_previous_xbrl_state() {
    let mut project = demo_project();
    project.source_options.xbrl = XbrlBoundaryOptions::external_source("taxonomy.xsd").ok();

    assert_eq!(
        boundary_format(&project, DataSide::Source, DataFormat::Json),
        DataFormat::Xbrl
    );
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Xbrl),
        DataFormat::Xml
    );
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Csv),
        DataFormat::Csv
    );

    project.target_options.json_document = true;
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Xml),
        DataFormat::Json
    );

    project.target_options.json_document = false;
    project.target_options.json_lines = true;
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Xml),
        DataFormat::Json
    );

    project.target_options.json_lines = false;
    project.target_options.json5 = true;
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Xml),
        DataFormat::Json
    );

    project.target_options.json5 = false;
    project.target_options.xml_document = true;
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Json),
        DataFormat::Xml
    );

    project.target_options.xml_document = false;
    project.target_options.tabular_kind = Some(TabularBoundaryKind::Csv);
    assert_eq!(
        boundary_format(&project, DataSide::Target, DataFormat::Xml),
        DataFormat::Csv
    );
}
