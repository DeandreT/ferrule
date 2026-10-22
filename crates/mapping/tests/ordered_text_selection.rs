use mapping::AggregateOp;

#[test]
fn ordered_text_selection_has_explicit_stable_wire_names() {
    for (operation, wire) in [
        (AggregateOp::LastPresentText, "\"last_present_text\""),
        (AggregateOp::LastDistinctText, "\"last_distinct_text\""),
    ] {
        assert_eq!(serde_json::to_string(&operation).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<AggregateOp>(wire).unwrap(),
            operation
        );
    }
    assert!(serde_json::from_str::<AggregateOp>("\"last\"").is_err());
}
