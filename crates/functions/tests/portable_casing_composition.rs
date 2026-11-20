use functions::call;
use ir::Value;

#[test]
fn downstream_operations_observe_portable_casing_results() {
    // Expected values are literal Unicode scalars, not host casing results.
    // These distinguish three independent contracts: no Turkish tailoring,
    // full expansion, and no normalization before downstream scalar length.
    for (function, input, expected, length) in [
        ("upper", "i/ﬃ", "I/FFI", 5),
        ("lower", "I\u{301}/İ", "i\u{301}/i\u{307}", 5),
        ("lower", "I\u{307}Σ", "i\u{307}ς", 3),
        ("upper", "𐐨ß", "𐐀SS", 3),
    ] {
        let mapped = call(function, &[Value::String(input.into())]).unwrap();
        assert_eq!(
            mapped,
            Value::String(expected.into()),
            "{function}({input})"
        );
        assert_eq!(call("length", &[mapped]), Ok(Value::Int(length)));
    }
}
