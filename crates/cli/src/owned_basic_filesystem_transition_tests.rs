use super::*;
#[test]
fn basic_field_and_collection_execute_with_publication_and_malformed_precedence()
-> anyhow::Result<()> {
    for collection in [false, true] {
        let directory = owned_host_test_directory("filesystem")?;
        let input = directory.join("input.json");
        let output = directory.join("output.json");
        let path = directory.join("project.json");
        let project = if collection {
            owned_collection_host_positive_project()
        } else {
            owned_source_test_project()
        };
        let bytes: &[u8] = if collection {
            br#"[{"Value":"one"},{"Value":"two"}]"#
        } else {
            br#"{"Value":"supported"}"#
        };
        std::fs::write(&input, bytes)?;
        std::fs::write(&output, b"old-output")?;
        let calls = std::cell::Cell::new(0);
        let publish = || {
            calls.set(calls.get() + 1);
            true
        };
        let options = RunOptions::new().with_before_publish(&publish);
        let outcome = run_project_value_with_filesystem_policy(&project, &path, &options, None)?;
        let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&output)?)?;
        let expected = if collection {
            serde_json::json!([{"Value":"legacy"},{"Value":"legacy"}])
        } else {
            serde_json::json!({"Value":"supported"})
        };
        assert_eq!(actual, expected);
        assert_eq!(outcome.records_written, if collection { 2 } else { 1 });
        assert_eq!(calls.get(), 1);
        std::fs::write(&output, b"old-output")?;
        std::fs::write(&input, b"not JSON")?;
        calls.set(0);
        let error =
            run_project_value_with_filesystem_policy(&project, &path, &options, None).unwrap_err();
        assert!(error.downcast_ref::<engine::EngineError>().is_none());
        assert_eq!(
            error.to_string(),
            format!(
                "{} {}",
                if collection {
                    "reading JSON input"
                } else {
                    "reading input"
                },
                input.display()
            )
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
        std::fs::write(&input, bytes)?;
        let cancel = || false;
        let error = run_project_value_with_filesystem_policy(
            &project,
            &path,
            &RunOptions::new().with_before_publish(&cancel),
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<engine::EngineError>(),
            Some(&engine::EngineError::DebugCancelled)
        );
        assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
        std::fs::remove_file(input)?;
        std::fs::remove_file(output)?;
        directory.remove_empty()?;
    }
    Ok(())
}
