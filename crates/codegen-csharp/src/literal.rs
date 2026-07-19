use std::fmt::Write;

use ir::Value;

use crate::EmitError;

pub(crate) fn string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for unit in value.encode_utf16() {
        match unit {
            0x22 => output.push_str("\\\""),
            0x5c => output.push_str("\\\\"),
            0x0a => output.push_str("\\n"),
            0x0d => output.push_str("\\r"),
            0x09 => output.push_str("\\t"),
            0x20..=0x7e => output.push(char::from(unit as u8)),
            _ => {
                let _ = write!(output, "\\u{unit:04X}");
            }
        }
    }
    output.push('"');
    output
}

pub(crate) fn value(_node: u32, value: &Value) -> Result<String, EmitError> {
    Ok(match value {
        Value::Null => "global::Ferrule.Runtime.FerruleValue.Null".to_string(),
        Value::XmlNil(_) => "global::Ferrule.Runtime.FerruleValue.XmlNil".to_string(),
        Value::Bool(value) => format!(
            "global::Ferrule.Runtime.FerruleValue.FromBoolean({})",
            if *value { "true" } else { "false" }
        ),
        Value::Int(value) => format!(
            "global::Ferrule.Runtime.FerruleValue.FromInt64(unchecked((long)0x{:016X}UL))",
            *value as u64
        ),
        Value::Float(value) => {
            format!(
                "global::Ferrule.Runtime.FerruleValue.FromDouble(global::System.BitConverter.Int64BitsToDouble(unchecked((long)0x{:016X}UL)))",
                value.to_bits()
            )
        }
        Value::String(value) => format!(
            "global::Ferrule.Runtime.FerruleValue.FromString({})",
            string(value)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_ascii_only_and_preserve_utf16_content() {
        let literal = string("quote \" slash \\ line\n nul\0 snowman \u{2603} emoji \u{1f642}");

        assert!(literal.is_ascii());
        assert!(literal.contains("\\\""));
        assert!(literal.contains("\\\\"));
        assert!(literal.contains("\\n"));
        assert!(literal.contains("\\u0000"));
        assert!(literal.contains("\\u2603"));
        assert!(literal.contains("\\uD83D\\uDE42"));
    }

    #[test]
    fn scalar_literals_cover_all_tags_and_exact_numeric_bits() {
        assert!(value(0, &Value::Null).is_ok_and(|value| value.ends_with(".Null")));
        assert!(value(0, &Value::xml_nil()).is_ok_and(|value| value.ends_with(".XmlNil")));
        assert!(value(0, &Value::Bool(true)).is_ok_and(|value| value.contains("true")));
        assert!(
            value(0, &Value::Int(i64::MIN)).is_ok_and(|value| value.contains("8000000000000000"))
        );
        assert!(
            value(0, &Value::Float(-0.0)).is_ok_and(|value| value.contains("8000000000000000"))
        );
        assert!(value(0, &Value::String("x".into())).is_ok_and(|value| value.ends_with("(\"x\")")));
    }

    #[test]
    fn nonfinite_float_preserves_exact_bits() {
        assert!(
            value(19, &Value::Float(f64::INFINITY))
                .is_ok_and(|value| value.contains("7FF0000000000000"))
        );
    }
}
