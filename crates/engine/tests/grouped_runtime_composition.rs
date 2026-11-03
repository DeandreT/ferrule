//! Native Ferrule table lifetime only; this does not admit MFD auto-number.
use std::path::Path;

#[path = "grouped_runtime_composition/project.rs"]
mod fixture;

#[test]
fn native_counter_table_spans_group_members_and_ordered_named_target_then_resets() {
    let project = fixture::project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let context = engine::ExecutionContext::new(Path::new("native-counter.ferrule"))
        .with_invocation_counter_limits(engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 16,
        });
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("grouped_runtime_composition/cases.json")).unwrap();
    for index in [0, 0, 1, 0, 1] {
        let case = &cases[index];
        let source = format_json::from_str(&case["input"].to_string(), &project.source).unwrap();
        let output =
            engine::run_outputs_with_sources_and_context(&project, &source, vec![], &context)
                .unwrap();
        assert_eq!(
            output.primary,
            format_json::from_str(&case["primary"].to_string(), &project.target).unwrap(),
            "{case}"
        );
        assert_eq!(output.extras.len(), 1);
        assert_eq!(output.extras[0].name, "audit");
        assert_eq!(
            output.extras[0].instance,
            format_json::from_str(&case["audit"].to_string(), &project.extra_targets[0].schema)
                .unwrap(),
            "{case}"
        );
        assert_eq!(
            engine::run_outputs(&project, &source).unwrap_err(),
            engine::EngineError::MissingInvocationCounters { node: 1 }
        );
    }
}
