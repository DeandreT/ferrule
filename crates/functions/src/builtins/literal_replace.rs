use ir::Value;

use crate::{FunctionError, scalar::text as scalar_text};

// Shared with the C# runtime and the regex `replace` result bound.
const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;

/// Replaces every non-overlapping occurrence of the search text, scanning left
/// to right, with the replacement text. Neither text has pattern or escape
/// syntax. An empty search text is an error, as in native runs of the
/// reference application. Arguments use lexical text, so absent values read
/// as "".
pub(super) fn replace_literal(args: &[Value]) -> Result<Value, FunctionError> {
    let [value, search, replacement] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "replace_literal",
            expected: 3,
            got: args.len(),
        });
    };
    let value = scalar_text(value);
    let search = scalar_text(search);
    if search.is_empty() {
        return Err(FunctionError::InvalidArgument {
            function: "replace_literal",
            message: "search text is empty",
        });
    }
    let replacement = scalar_text(replacement);
    let mut output = String::new();
    let mut end = 0;
    for (start, _) in value.match_indices(search.as_str()) {
        push_bounded(&mut output, &value[end..start])?;
        push_bounded(&mut output, &replacement)?;
        end = start + search.len();
    }
    push_bounded(&mut output, &value[end..])?;
    Ok(Value::String(output))
}

fn push_bounded(output: &mut String, value: &str) -> Result<(), FunctionError> {
    if value.len() > MAX_RESULT_BYTES - output.len() {
        return Err(FunctionError::InvalidArgument {
            function: "replace_literal",
            message: "result exceeds 64 MiB",
        });
    }
    output.push_str(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use crate::{FunctionError, call};

    fn replace(value: Value, search: &str, replacement: &str) -> Result<Value, FunctionError> {
        call(
            "replace_literal",
            &[
                value,
                Value::String(search.into()),
                Value::String(replacement.into()),
            ],
        )
    }

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }

    #[test]
    fn search_and_replacement_text_are_literal() {
        for (value, search, replacement, expected) in [
            ("a.c abc", ".", "-", "a-c abc"),
            ("1*2(3)$4\\5", "*", "x", "1x2(3)$4\\5"),
            ("1*2(3)$4\\5", "(", "[", "1*2[3)$4\\5"),
            ("1*2(3)$4\\5", "$", "D", "1*2(3)D4\\5"),
            ("1*2(3)$4\\5", "\\", "/", "1*2(3)$4/5"),
            ("abc", "b", "$0$1\\", "a$0$1\\c"),
            ("aaa", "aa", "b", "ba"),
            ("a.b", ".", "", "ab"),
            ("ABC", "b", "x", "ABC"),
            ("a.b.c", ".", "..", "a..b..c"),
            ("a🙂b🙂", "🙂", "é", "aébé"),
        ] {
            assert_eq!(
                replace(text(value), search, replacement),
                Ok(text(expected)),
                "{value} {search} {replacement}"
            );
        }
    }

    #[test]
    fn empty_search_text_is_rejected() {
        for value in [text("abc"), text(""), Value::Null] {
            assert_eq!(
                replace(value, "", "-"),
                Err(FunctionError::InvalidArgument {
                    function: "replace_literal",
                    message: "search text is empty",
                })
            );
        }
        assert!(call("replace_literal", &[text("abc"), Value::Null, text("-")]).is_err());
    }

    #[test]
    fn arguments_use_lexical_text() {
        assert_eq!(replace(Value::Null, "a", "b"), Ok(text("")));
        assert_eq!(replace(Value::Int(1203), "0", ""), Ok(text("123")));
        assert_eq!(
            call("replace_literal", &[text("a1"), text("a"), Value::Int(9)]),
            Ok(text("91"))
        );
        assert_eq!(
            call("replace_literal", &[text("abc"), text("b"), Value::Null]),
            Ok(text("ac"))
        );
    }

    #[test]
    fn rejects_wrong_arity_and_oversized_results() {
        assert!(matches!(
            call("replace_literal", &[text("a"), text("b")]),
            Err(FunctionError::ArityMismatch {
                function: "replace_literal",
                expected: 3,
                got: 2
            })
        ));
        let value = "a".repeat(1024);
        let replacement = "b".repeat(64 * 1024 + 1);
        assert!(matches!(
            replace(text(&value), "a", &replacement),
            Err(FunctionError::InvalidArgument {
                function: "replace_literal",
                ..
            })
        ));
    }
}
