use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode};
use mapping::{
    CsvTextRepairCause as Cause, CsvTextRepairDependency as Dependency, FormatOptions, Graph,
    NamedSource, NamedTarget, Pipeline, PipelineInput, PipelineStage, Project, RuntimeBoundary,
    Scope,
};

fn dependency() -> Dependency {
    Dependency::new(Cause::Encoding)
        .with_cause(Cause::ByteOrder)
        .with_cause(Cause::ByteOrderMark)
}
fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}
fn project() -> Project {
    let options = FormatOptions {
        csv_text_repair_dependency: Some(dependency()),
        ..Default::default()
    };
    Project {
        source: schema(),
        target: schema(),
        source_path: None,
        target_path: None,
        source_options: options.clone(),
        target_options: options.clone(),
        extra_sources: vec![NamedSource {
            name: "lookup".into(),
            path: "missing.csv".into(),
            schema: schema(),
            options: options.clone(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.csv".into()),
            schema: schema(),
            options,
            root: Scope::default(),
        }],
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

#[test]
fn closed_cause_set_is_copy_canonical_and_rejects_ambiguous_metadata() {
    fn require_copy<T: Copy>(_: T) {}
    require_copy(dependency());
    assert_eq!(
        dependency().causes().collect::<Vec<_>>(),
        vec![Cause::Encoding, Cause::ByteOrder, Cause::ByteOrderMark,]
    );
    assert_eq!(
        serde_json::to_string(&dependency()).unwrap(),
        r#"{"causes":["unsupported_encoding","unsupported_byte_order","unsupported_byte_order_mark"]}"#
    );
    assert_eq!(
        serde_json::from_str::<Dependency>(
            r#"{"causes":["unsupported_byte_order","unsupported_encoding"]}"#
        )
        .unwrap(),
        Dependency::new(Cause::Encoding).with_cause(Cause::ByteOrder)
    );
    for invalid in [
        r#"{}"#,
        r#"{"causes":[]}"#,
        r#"{"causes":["unknown"]}"#,
        r#"{"causes":["unsupported_encoding","unsupported_encoding"]}"#,
        r#"{"causes":["unsupported_encoding"],"causes":["unsupported_byte_order"]}"#,
        r#"{"causes":["unsupported_encoding"],"code":"52"}"#,
        r#"["unsupported_encoding"]"#,
        r#"{"causes":[1]}"#,
    ] {
        assert!(
            serde_json::from_str::<Dependency>(invalid).is_err(),
            "{invalid}"
        );
    }
    let excessive = format!(
        "{{\"causes\":[\"unsupported_encoding\",\"unsupported_byte_order\",\"unsupported_byte_order_mark\",{}]}}",
        std::iter::repeat_n("\"unsupported_encoding\"", 100_000)
            .collect::<Vec<_>>()
            .join(",")
    );
    let error = serde_json::from_str::<Dependency>(&excessive).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("duplicate CSV text repair cause")
    );
    let unknown = format!("{{\"causes\":[\"{}\"]}}", "9".repeat(16_384));
    assert!(
        serde_json::from_str::<Dependency>(&unknown)
            .unwrap_err()
            .to_string()
            .len()
            < 100
    );
}

#[test]
fn project_and_pipeline_files_preserve_primary_and_named_blockers() {
    let mut project = project();
    for _ in 0..3 {
        project = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&project).unwrap(),
        )
        .unwrap();
        let dependencies = project.csv_runtime_dependencies();
        assert_eq!(
            dependencies
                .iter()
                .map(|d| d.boundary.clone())
                .collect::<Vec<_>>(),
            vec![
                RuntimeBoundary::PrimarySource,
                RuntimeBoundary::PrimaryTarget,
                RuntimeBoundary::NamedSource("lookup".into()),
                RuntimeBoundary::NamedTarget("audit".into()),
            ]
        );
        assert!(dependencies.iter().all(|d| d.dependency == dependency()));
        assert!(project.runtime_dependencies().is_empty());
        assert!(project.pdf_runtime_dependencies().is_empty());
    }
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "map".into(),
            mapping_path: None,
            project,
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
        }],
    };
    let reopened = mapping::pipeline_file::decode_str(
        &mapping::pipeline_file::encode_pretty(&pipeline).unwrap(),
    )
    .unwrap();
    assert_eq!(
        reopened.stages[0].project.csv_runtime_dependencies().len(),
        4
    );
}

#[test]
fn legacy_format_options_have_no_repair_marker() {
    let legacy: FormatOptions = serde_json::from_str("{}").unwrap();
    assert!(legacy.csv_text_repair_dependency.is_none());
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("csv_text_repair_dependency")
    );
}

#[test]
fn all_eight_causes_are_copy_canonical_bounded_and_persisted() {
    let expected = vec![
        Cause::Encoding,
        Cause::ByteOrder,
        Cause::ByteOrderMark,
        Cause::EmptyPolicy,
        Cause::Separator,
        Cause::Quote,
        Cause::HeaderRow,
        Cause::TypedEmptyCells,
    ];
    let full = expected
        .iter()
        .copied()
        .skip(1)
        .fold(Dependency::new(expected[0]), Dependency::with_cause);
    assert_eq!(full.causes().collect::<Vec<_>>(), expected);
    let unordered = r#"{"causes":["unsupported_typed_empty_cells","unsupported_header_row","unsupported_quote","unsupported_separator","unsupported_empty_policy","unsupported_byte_order_mark","unsupported_byte_order","unsupported_encoding"]}"#;
    assert_eq!(serde_json::from_str::<Dependency>(unordered).unwrap(), full);
    let canonical = r#"{"causes":["unsupported_encoding","unsupported_byte_order","unsupported_byte_order_mark","unsupported_empty_policy","unsupported_separator","unsupported_quote","unsupported_header_row","unsupported_typed_empty_cells"]}"#;
    assert_eq!(serde_json::to_string(&full).unwrap(), canonical);
    // The ninth cause fails before parsing the unbounded tail.
    let excessive = format!(
        "{},\"unsupported_encoding\",{}]}}",
        canonical.trim_end_matches("]}"),
        std::iter::repeat_n("\"unsupported_encoding\"", 100_000)
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(
        serde_json::from_str::<Dependency>(&excessive)
            .unwrap_err()
            .to_string()
            .contains("duplicate CSV text repair cause")
    );
    let mut project = project();
    project.source_options.csv_text_repair_dependency = Some(full);
    project.target_options.csv_text_repair_dependency = Some(full);
    project.extra_sources[0].options.csv_text_repair_dependency = Some(full);
    project.extra_targets[0].options.csv_text_repair_dependency = Some(full);
    let reopened =
        mapping::project_file::decode_str(&mapping::project_file::encode_pretty(&project).unwrap())
            .unwrap();
    assert_eq!(reopened.csv_runtime_dependencies().len(), 4);
    assert!(
        reopened
            .csv_runtime_dependencies()
            .iter()
            .all(|finding| finding.dependency == full)
    );
    for cause in expected {
        assert_eq!(
            serde_json::from_str::<Cause>(&serde_json::to_string(&cause).unwrap()).unwrap(),
            cause
        );
    }
}
