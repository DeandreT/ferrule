use std::path::Path;

use ir::ScalarType;
use mapping::{
    DelimitedDialect, DelimitedRecordField, FlexCommand, FlexLineEnding, FlexTextLayout,
};

fn layout() -> FlexTextLayout {
    FlexTextLayout::new(
        "document",
        FlexCommand::DelimitedRecords {
            name: "rows".into(),
            dialect: DelimitedDialect::new(';', "\n", '"', '\\').unwrap(),
            fields: vec![
                DelimitedRecordField::new("first_name", ScalarType::String).unwrap(),
                DelimitedRecordField::new("last_name", ScalarType::String).unwrap(),
                DelimitedRecordField::new("age", ScalarType::Int).unwrap(),
            ],
        },
        FlexLineEnding::Crlf,
        false,
    )
    .unwrap()
}

#[test]
fn flextext_input_overrides_extension_and_rejects_conflicting_options() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut project: mapping::Project =
        serde_json::from_slice(&std::fs::read(fixture_dir.join("project.json")).unwrap()).unwrap();
    let layout = layout();
    project.source = layout.schema();
    project.source_options.flextext = Some(layout);
    project.root.set_source(Some(vec!["rows".into()]));

    let tag = format!("flextext_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let input_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.ini"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.csv"));
    let write_project = |project: &mapping::Project| {
        std::fs::write(&project_path, serde_json::to_vec(project).unwrap()).unwrap();
    };
    write_project(&project);
    std::fs::write(&input_path, "Jane;Doe;29\r\nJohn;Smith;41").unwrap();

    let written = cli::run_project(&project_path, &input_path, &output_path).unwrap();
    assert_eq!(written, 2);
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "full_name,age_next_year\nJane Doe,30\nJohn Smith,42\n"
    );

    project.source_options.delimiter = Some(',');
    write_project(&project);
    let error = cli::run_project(&project_path, &input_path, &output_path).unwrap_err();
    assert!(error.to_string().contains("`flextext` cannot be combined"));
    assert_eq!(
        std::fs::read_to_string(&output_path).unwrap(),
        "full_name,age_next_year\nJane Doe,30\nJohn Smith,42\n"
    );

    for path in [project_path, input_path, output_path] {
        std::fs::remove_file(path).unwrap();
    }
}
