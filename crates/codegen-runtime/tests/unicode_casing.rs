use codegen_runtime::call;
use ir::Value;

fn text(value: &str) -> Value {
    Value::String(value.into())
}

// Generated Rust dispatches through the shared portable casing contract.
#[test]
fn generated_dispatch_applies_portable_full_casing() {
    for (function, input, expected) in [
        ("upper", "straße ﬁ", "STRASSE FI"),
        ("upper", "ΐ ǅ ı", "\u{399}\u{308}\u{301} Ǆ I"),
        ("upper", "𐐨\u{16EBB}", "𐐀\u{16EA0}"),
        ("lower", "İ ẞ", "i\u{307} ß"),
        ("lower", "ΟΔΟΣ ΣΑ A'Σ", "οδος σα a'ς"),
        ("lower", "𐐀Σ ΑΣ𐐀", "𐐨ς ασ𐐨"),
    ] {
        assert_eq!(
            call(function, &[text(input)]),
            Ok(text(expected)),
            "{function}({input:?})"
        );
    }
}
