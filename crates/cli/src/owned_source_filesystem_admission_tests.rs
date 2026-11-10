use super::*;

#[test]
fn owned_refusal_precedes_input_open_and_publication_callback() {
    let project = closed_owned_source_test_project();
    let published = std::cell::Cell::new(0);
    let publish = || {
        published.set(published.get() + 1);
        true
    };
    let options = RunOptions {
        before_publish: Some(&publish),
        ..RunOptions::default()
    };
    let error = run_project_value_with_filesystem_policy(
        &project,
        Path::new("nonexistent-owned-input-parent/project.json"),
        &options,
        None,
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<engine::EngineError>(),
        Some(engine::EngineError::UnsupportedSourceOwnership {
            node: 9,
            function: Some(function),
            kind: "owned_source_field"
        }) if *function == mapping::FunctionId::new(7)
    ));
    assert_eq!(published.get(), 0);
}
