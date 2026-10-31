use std::collections::HashMap;

use ir::Value;

use crate::{FunctionError, scalar::text as scalar_text};

/// XPath `fn:translate`: each Unicode scalar of `value` found in `search` is
/// replaced by the scalar at the same position in `replacement`, or removed
/// when `replacement` is shorter. The first occurrence of a repeated search
/// scalar wins. Arguments use lexical text, so absent values read as "".
pub(super) fn translate(args: &[Value]) -> Result<Value, FunctionError> {
    let [value, search, replacement] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "translate",
            expected: 3,
            got: args.len(),
        });
    };
    let replacement = scalar_text(replacement).chars().collect::<Vec<_>>();
    let mut table = HashMap::new();
    for (index, character) in scalar_text(search).chars().enumerate() {
        table
            .entry(character)
            .or_insert_with(|| replacement.get(index).copied());
    }
    let output = scalar_text(value)
        .chars()
        .filter_map(|character| match table.get(&character) {
            Some(mapped) => *mapped,
            None => Some(character),
        })
        .collect();
    Ok(Value::String(output))
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use crate::{FunctionError, call};

    fn translate(value: Value, search: &str, replacement: &str) -> Result<Value, FunctionError> {
        call(
            "translate",
            &[
                value,
                Value::String(search.into()),
                Value::String(replacement.into()),
            ],
        )
    }

    fn text(value: &str) -> Result<Value, FunctionError> {
        Ok(Value::String(value.into()))
    }

    #[test]
    fn replaces_characters_by_position() {
        assert_eq!(
            translate(Value::String("[12,3]".into()), "[,]", "(.)"),
            text("(12.3)")
        );
        assert_eq!(
            translate(Value::String("bar".into()), "abc", "ABC"),
            text("BAr")
        );
    }

    #[test]
    fn removes_characters_without_a_replacement() {
        assert_eq!(
            translate(Value::String("38ab8a7a65xkh3".into()), "0123456789", ""),
            text("abaaxkh")
        );
        assert_eq!(
            translate(Value::String("--aaa--".into()), "abc-", "ABC"),
            text("AAA")
        );
    }

    #[test]
    fn first_occurrence_of_a_repeated_search_character_wins() {
        assert_eq!(
            translate(Value::String("abcabc".into()), "aba", "xyz"),
            text("xycxyc")
        );
        // The later duplicate has no replacement, but the first mapping holds.
        assert_eq!(translate(Value::String("aa".into()), "aa", "x"), text("xx"));
        // A duplicate does not delete a character the first occurrence maps.
        assert_eq!(
            translate(Value::String("ab".into()), "abb", "xy"),
            text("xy")
        );
    }

    #[test]
    fn operates_on_unicode_scalars_including_non_bmp() {
        assert_eq!(
            translate(
                Value::String("a\u{1f642}b\u{10348}".into()),
                "\u{1f642}b",
                "\u{e9}"
            ),
            text("a\u{e9}\u{10348}")
        );
        assert_eq!(
            translate(Value::String("x\u{e9}y".into()), "\u{e9}", "\u{1f600}"),
            text("x\u{1f600}y")
        );
        // Combining sequences are separate scalars, not one grapheme.
        assert_eq!(
            translate(Value::String("e\u{301}".into()), "\u{301}", ""),
            text("e")
        );
    }

    #[test]
    fn empty_inputs_leave_or_produce_empty_text() {
        assert_eq!(
            translate(Value::String(String::new()), "abc", "x"),
            text("")
        );
        assert_eq!(
            translate(Value::String("abc".into()), "", "xyz"),
            text("abc")
        );
        assert_eq!(translate(Value::String("abc".into()), "", ""), text("abc"));
        // Extra replacement characters are ignored.
        assert_eq!(
            translate(Value::String("abc".into()), "a", "xyz"),
            text("xbc")
        );
    }

    #[test]
    fn absent_and_non_string_arguments_use_lexical_text() {
        assert_eq!(translate(Value::Null, "a", "b"), text(""));
        assert_eq!(translate(Value::json_null(), "a", "b"), text(""));
        assert_eq!(translate(Value::Int(1203), "0", ""), text("123"));
        assert_eq!(translate(Value::Bool(true), "t", "T"), text("True"));
        assert_eq!(
            call(
                "translate",
                &[Value::String("abc".into()), Value::Null, Value::Null]
            ),
            text("abc")
        );
        assert_eq!(
            call(
                "translate",
                &[
                    Value::String("a1".into()),
                    Value::String("a".into()),
                    Value::Int(9)
                ]
            ),
            text("91")
        );
    }

    #[test]
    fn requires_exactly_three_arguments() {
        for count in [0, 1, 2, 4] {
            assert_eq!(
                call("translate", &vec![Value::String("a".into()); count]),
                Err(FunctionError::ArityMismatch {
                    function: "translate",
                    expected: 3,
                    got: count,
                })
            );
        }
    }
}
