#[path = "../../../conformance/document-clocks/project.rs"]
mod base;
#[path = "../../../conformance/distinct-text-map/project.rs"]
mod fixture;

#[test]
fn distinct_text_mapping_export_refuses_before_creating_artifacts() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "distinct-map-export-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut project = fixture::project(base::project());
    project.root.iteration = mapping::ScopeIteration::None;
    project.source_options = Default::default();
    project.target_options = Default::default();
    let result = mfd::export(&project, &root.join("mapping.mfd"));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("ordered text selection")
    );
    project.root.bindings[0].node = 60;
    let result = mfd::export(&project, &root.join("unused.mfd"));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("ordered text selection")
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    std::fs::remove_dir(root).unwrap();
}
