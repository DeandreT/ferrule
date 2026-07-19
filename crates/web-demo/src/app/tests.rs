use mapping::XbrlBoundaryOptions;

use super::*;

#[test]
fn demo_project_runs_on_the_sample_input() {
    let mut app = DemoApp::new();
    app.run();
    assert!(
        app.output.contains("<AllIds>A-1, B-2</AllIds>"),
        "{}",
        app.output
    );
    assert!(
        app.output.contains("<ItemCount>2</ItemCount>"),
        "{}",
        app.output
    );
    assert!(app.output.contains("<Total>10</Total>"), "{}", app.output);
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
