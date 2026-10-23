use ir::Value;

#[test]
fn adjacent_variable_date_components_require_the_unambiguous_maximum_width() {
    let parse = |text: &str| {
        functions::call(
            "parse_date",
            &[
                Value::String(text.into()),
                Value::String("[Y,2-4][M,1-2][D,1-2]".into()),
            ],
        )
    };
    assert_eq!(
        parse("20240229").unwrap(),
        Value::String("2024-02-29".into())
    );
    assert_eq!(
        parse("20271031").unwrap(),
        Value::String("2027-10-31".into())
    );
    for invalid in ["20260229", "20271331", "2027011", "202710310", "abcdefgh"] {
        assert!(parse(invalid).is_err(), "{invalid}");
    }
}
