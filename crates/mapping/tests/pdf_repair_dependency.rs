use std::collections::BTreeMap;

use mapping::{
    FormatOptions, Graph, NamedSource, PdfCapture, PdfCommand, PdfLayout, PdfPageSelection,
    PdfRegion, PdfRepairDependency, Project, RuntimeBoundary, Scope,
};

fn layout() -> PdfLayout {
    PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
}

#[test]
fn legacy_layout_wire_is_unchanged_and_repair_state_is_closed() {
    let plain = layout();
    let encoded = serde_json::to_string(&plain).unwrap();
    assert!(!encoded.contains("repair_dependency"));
    assert_eq!(serde_json::from_str::<PdfLayout>(&encoded).unwrap(), plain);

    let repair = plain.with_repair_dependency(PdfRepairDependency::ObjectFind);
    let encoded = serde_json::to_string(&repair).unwrap();
    assert!(encoded.contains(r#""repair_dependency":{"kind":"object_find"}"#));
    assert_eq!(serde_json::from_str::<PdfLayout>(&encoded).unwrap(), repair);
    for invalid in [
        r#"{"kind":"unknown"}"#,
        r#"{"kind":"object_find","template":"sensitive"}"#,
        r#"{}"#,
        r#""object_find""#,
    ] {
        let mut value = serde_json::to_value(&repair).unwrap();
        value["repair_dependency"] = serde_json::from_str(invalid).unwrap();
        assert!(
            serde_json::from_value::<PdfLayout>(value).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn file_codecs_and_dependency_reports_preserve_repair_after_reload() {
    let repair = layout().with_repair_dependency(PdfRepairDependency::ObjectFind);
    let encoded = mapping::pdf_layout_file::encode_pretty(&repair).unwrap();
    assert_eq!(
        mapping::pdf_layout_file::decode_str(&encoded).unwrap(),
        repair
    );
    let mut project = Project {
        source: repair.schema(),
        target: repair.schema(),
        source_path: Some("source.pdf".into()),
        target_path: None,
        source_options: FormatOptions {
            pdf: Some(repair.clone()),
            ..Default::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: vec![NamedSource {
            name: "catalog".into(),
            path: "catalog.pdf".into(),
            schema: repair.schema(),
            options: FormatOptions {
                pdf: Some(repair.clone()),
                ..Default::default()
            },
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    for _ in 0..3 {
        project = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&project).unwrap(),
        )
        .unwrap();
        let dependencies = project.pdf_runtime_dependencies();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].boundary, RuntimeBoundary::PrimarySource);
        assert_eq!(
            dependencies[1].boundary,
            RuntimeBoundary::NamedSource("catalog".into())
        );
        assert!(
            dependencies
                .iter()
                .all(|dependency| dependency.dependency == PdfRepairDependency::ObjectFind)
        );
        assert!(
            project.runtime_dependencies().is_empty(),
            "legacy EDI API remains unchanged"
        );
    }
}
