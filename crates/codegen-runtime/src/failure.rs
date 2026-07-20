use crate::{RuntimeError, Value};

/// Creates the typed failure raised by one generated mapping rule.
///
/// `None` preserves an absent message. A present absent/XML-nil scalar is an
/// explicitly empty message, matching interpreted execution.
pub fn mapping_failure(rule: usize, message: Option<Value>) -> RuntimeError {
    RuntimeError::MappingFailure {
        rule,
        message: message.map(scalar_text),
    }
}

fn scalar_text(value: Value) -> String {
    match value {
        Value::Null | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_message_uses_the_default_display_text() {
        let error = mapping_failure(2, None);

        assert_eq!(
            error,
            RuntimeError::MappingFailure {
                rule: 2,
                message: None,
            }
        );
        assert_eq!(
            error.to_string(),
            "mapping failure rule 2: mapping exception was raised"
        );
    }

    #[test]
    fn present_scalars_use_engine_message_lexicals() {
        let cases = [
            (Value::Null, ""),
            (Value::xml_nil(), ""),
            (Value::Bool(true), "true"),
            (Value::Int(-7), "-7"),
            (Value::Float(1.25), "1.25"),
            (Value::String("message".into()), "message"),
        ];

        for (value, expected) in cases {
            assert_eq!(
                mapping_failure(1, Some(value)),
                RuntimeError::MappingFailure {
                    rule: 1,
                    message: Some(expected.to_string()),
                }
            );
        }
    }

    #[test]
    fn explicit_empty_message_does_not_use_the_default() {
        assert_eq!(
            mapping_failure(3, Some(Value::Null)).to_string(),
            "mapping failure rule 3: "
        );
    }
}
