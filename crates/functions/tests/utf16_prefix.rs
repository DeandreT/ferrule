use ir::Value;

#[test]
fn explicit_utf16_prefix_keeps_ordinary_character_slicing_unchanged() {
    for (input, count, expected) in [
        ("😀A", 2, "😀"),
        ("😀A", 1, "�"),
        ("A😀B", 2, "A�"),
        ("A😀B", 3, "A😀"),
        ("ABC", 2, "AB"),
        ("A\u{301}B", 2, "A\u{301}"),
    ] {
        assert_eq!(
            functions::call(
                "left_utf16_lossy",
                &[Value::String(input.into()), Value::Int(count)]
            )
            .unwrap(),
            Value::String(expected.into())
        );
    }
    assert_eq!(
        functions::call("left", &[Value::String("😀A".into()), Value::Int(2)]).unwrap(),
        Value::String("😀A".into())
    );
}
