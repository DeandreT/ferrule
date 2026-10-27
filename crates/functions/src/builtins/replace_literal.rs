use crate::FunctionError;
use ir::Value;

const FUNCTION: &str = "replace_literal";
const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn replace(args: &[Value]) -> Result<Value, FunctionError> {
    let [input, search, replacement] = args else {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 3,
            got: args.len(),
        });
    };
    let input = text(input)?;
    let search = text(search)?;
    let replacement = match replacement {
        Value::Null | Value::JsonNull(_) => "",
        value => text(value)?,
    };
    if search.is_empty() {
        return Err(invalid("search must not be empty"));
    }
    if [input.len(), search.len(), replacement.len()]
        .into_iter()
        .any(|len| len > MAX_BYTES)
    {
        return Err(invalid("text exceeds 64 MiB"));
    }
    let count = input.match_indices(search).count();
    let bytes = input.len() - count * search.len();
    let bytes = count
        .checked_mul(replacement.len())
        .and_then(|added| bytes.checked_add(added))
        .filter(|bytes| *bytes <= MAX_BYTES)
        .ok_or_else(|| invalid("result exceeds 64 MiB"))?;
    let mut result = String::with_capacity(bytes);
    let mut end = 0;
    for (index, _) in input.match_indices(search) {
        result.push_str(&input[end..index]);
        result.push_str(replacement);
        end = index + search.len();
    }
    result.push_str(&input[end..]);
    Ok(Value::String(result))
}

fn invalid(message: &'static str) -> FunctionError {
    FunctionError::InvalidArgument {
        function: FUNCTION,
        message,
    }
}

fn text(value: &Value) -> Result<&str, FunctionError> {
    match value {
        Value::String(text) => Ok(text),
        other => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: other.type_name(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(values: [&str; 3]) -> Vec<Value> {
        values.map(|value| Value::String(value.into())).into()
    }
    #[test]
    fn literal_metacharacters_and_ordinal_nonoverlapping_matches() {
        for (input, search, replacement, expected) in [
            (r"a\b\c", r"\", "_", "a_b_c"),
            ("a.a", ".", "$1\\", "a$1\\a"),
            ("aaaaa", "aa", "b", "bba"),
            ("AaA", "a", "x", "AxA"),
            ("é😀é", "é", "🦀", "🦀😀🦀"),
            ("e\u{301}", "é", "x", "e\u{301}"),
            ("", "x", "y", ""),
        ] {
            assert_eq!(
                replace(&strings([input, search, replacement])).unwrap(),
                Value::String(expected.into())
            );
        }
    }
    #[test]
    fn null_replacement_removes_but_null_operands_and_empty_search_fail() {
        assert_eq!(
            replace(&[
                Value::String("aba".into()),
                Value::String("a".into()),
                Value::Null,
            ])
            .unwrap(),
            Value::String("b".into())
        );
        assert!(replace(&strings(["", "", "x"])).is_err());
        assert!(replace(&[Value::Null, Value::String("a".into()), Value::Null]).is_err());
        assert!(replace(&[Value::String("a".into()), Value::Null, Value::Null]).is_err());
    }
    #[test]
    fn expansion_is_bounded_before_allocation() {
        assert!(replace(&strings(["aa", "a", &"x".repeat(MAX_BYTES / 2 + 1)])).is_err());
    }
}
