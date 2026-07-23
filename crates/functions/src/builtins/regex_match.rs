use ir::Value;
use regex::{Regex, RegexBuilder};

use crate::{FunctionError, scalar::text as scalar_text};

const MAX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_COMPILED_BYTES: usize = 10 * 1024 * 1024;
const MAX_REPLACEMENT_BYTES: usize = 64 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn matches(args: &[Value]) -> Result<Value, FunctionError> {
    let (input, pattern, flags) = match args {
        [input, pattern] => (input, pattern, String::new()),
        [input, pattern, flags] => (input, pattern, scalar_text(flags)),
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "matches",
                expected: 2,
                got: args.len(),
            });
        }
    };
    let input = scalar_text(input);
    let pattern = scalar_text(pattern);
    let regex = compile("matches", &pattern, &flags)?;
    Ok(Value::Bool(regex.is_match(&input)))
}

pub(super) fn replace(args: &[Value]) -> Result<Value, FunctionError> {
    let (input, pattern, replacement, flags) = match args {
        [input, pattern, replacement] => (input, pattern, replacement, String::new()),
        [input, pattern, replacement, flags] => (input, pattern, replacement, scalar_text(flags)),
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "replace",
                expected: 3,
                got: args.len(),
            });
        }
    };
    let input = scalar_text(input);
    let pattern = scalar_text(pattern);
    let replacement = scalar_text(replacement);
    if replacement.len() > MAX_REPLACEMENT_BYTES {
        return Err(FunctionError::InvalidArgument {
            function: "replace",
            message: "replacement exceeds 64 KiB",
        });
    }
    let regex = compile("replace", &pattern, &flags)?;
    if regex.is_match("") {
        return Err(FunctionError::InvalidArgument {
            function: "replace",
            message: "pattern matches a zero-length string",
        });
    }
    let replacement = parse_replacement(&replacement, regex.captures_len() - 1)?;
    let mut output = String::new();
    let mut end = 0;
    for captures in regex.captures_iter(&input) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        if whole.start() == whole.end() {
            return Err(FunctionError::InvalidArgument {
                function: "replace",
                message: "pattern produced a zero-length match",
            });
        }
        push_bounded(&mut output, &input[end..whole.start()])?;
        for token in &replacement {
            match token {
                ReplacementToken::Literal(value) => push_bounded(&mut output, value)?,
                ReplacementToken::Group { index, suffix } => {
                    if let Some(value) = captures.get(*index) {
                        push_bounded(&mut output, value.as_str())?;
                    }
                    push_bounded(&mut output, suffix)?;
                }
            }
        }
        end = whole.end();
    }
    push_bounded(&mut output, &input[end..])?;
    Ok(Value::String(output))
}

fn compile(function: &'static str, pattern: &str, flags: &str) -> Result<Regex, FunctionError> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(FunctionError::InvalidArgument {
            function,
            message: "pattern exceeds 64 KiB",
        });
    }
    let mut builder = RegexBuilder::new(pattern);
    builder.size_limit(MAX_COMPILED_BYTES);
    for flag in flags.chars() {
        match flag {
            'i' => builder.case_insensitive(true),
            'm' => builder.multi_line(true),
            's' => builder.dot_matches_new_line(true),
            'x' => builder.ignore_whitespace(true),
            _ => {
                return Err(FunctionError::InvalidArgument {
                    function,
                    message: "flags contain an unsupported value",
                });
            }
        };
    }
    builder.build().map_err(|_| FunctionError::InvalidArgument {
        function,
        message: "pattern is invalid or exceeds the compiled-size limit",
    })
}

#[derive(Debug, PartialEq, Eq)]
enum ReplacementToken {
    Literal(String),
    Group { index: usize, suffix: String },
}

fn parse_replacement(
    replacement: &str,
    group_count: usize,
) -> Result<Vec<ReplacementToken>, FunctionError> {
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut chars = replacement.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some((_, escaped @ ('\\' | '$'))) => literal.push(escaped),
                _ => return invalid_replacement(),
            },
            '$' => {
                let Some(&(start, digit)) = chars.peek() else {
                    return invalid_replacement();
                };
                if !digit.is_ascii_digit() {
                    return invalid_replacement();
                }
                if !literal.is_empty() {
                    tokens.push(ReplacementToken::Literal(std::mem::take(&mut literal)));
                }
                chars.next();
                let mut end = start + digit.len_utf8();
                while let Some(&(index, next)) = chars.peek()
                    && next.is_ascii_digit()
                {
                    chars.next();
                    end = index + next.len_utf8();
                }
                let digits = &replacement[start..end];
                let (index, suffix) = resolve_group(digits, group_count);
                tokens.push(ReplacementToken::Group { index, suffix });
            }
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        tokens.push(ReplacementToken::Literal(literal));
    }
    Ok(tokens)
}

fn resolve_group(digits: &str, group_count: usize) -> (usize, String) {
    let mut prefix_end = digits.len();
    loop {
        if let Ok(index) = digits[..prefix_end].parse::<usize>()
            && (index <= group_count || index <= 9)
        {
            return (index, digits[prefix_end..].to_string());
        }
        prefix_end -= 1;
    }
}

fn invalid_replacement<T>() -> Result<T, FunctionError> {
    Err(FunctionError::InvalidArgument {
        function: "replace",
        message: "replacement has an invalid dollar or backslash escape",
    })
}

fn push_bounded(output: &mut String, value: &str) -> Result<(), FunctionError> {
    let next = output
        .len()
        .checked_add(value.len())
        .filter(|size| *size <= MAX_RESULT_BYTES)
        .ok_or(FunctionError::InvalidArgument {
            function: "replace",
            message: "result exceeds 64 MiB",
        })?;
    output.reserve(next - output.len());
    output.push_str(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_coerces_scalars_and_rejects_bad_regex_inputs() {
        assert_eq!(
            matches(&[Value::Int(120), Value::String(r"2\d".into())]),
            Ok(Value::Bool(true))
        );
        for args in [
            vec![Value::String("value".into()), Value::String("[".into())],
            vec![
                Value::String("value".into()),
                Value::String("value".into()),
                Value::String("q".into()),
            ],
            vec![
                Value::String("value".into()),
                Value::String("a".repeat(MAX_PATTERN_BYTES + 1)),
            ],
        ] {
            assert!(matches!(
                matches(&args),
                Err(FunctionError::InvalidArgument {
                    function: "matches",
                    ..
                })
            ));
        }
    }

    #[test]
    fn replace_supports_xpath_groups_flags_and_escapes() {
        assert_eq!(
            replace(&[
                Value::String("abracadabra".into()),
                Value::String("a(.)".into()),
                Value::String("a$1$1".into()),
            ]),
            Ok(Value::String("abbraccaddabbra".into()))
        );
        assert_eq!(
            replace(&[
                Value::String("Ferrule 2026".into()),
                Value::String(r"(ferrule)\s+(\d+)".into()),
                Value::String(r"$2:\$1=\\$1".into()),
                Value::String("i".into()),
            ]),
            Ok(Value::String(r"2026:$1=\Ferrule".into()))
        );
        assert_eq!(
            replace(&[
                Value::String("ab".into()),
                Value::String("(a)".into()),
                Value::String("$10-$23-$9-$0".into()),
            ]),
            Ok(Value::String("a0-3--ab".into()))
        );
    }

    #[test]
    fn replace_rejects_zero_width_invalid_replacement_and_bounds() {
        for args in [
            vec![
                Value::String("abc".into()),
                Value::String("a*".into()),
                Value::String("x".into()),
            ],
            vec![
                Value::String("abc".into()),
                Value::String("a".into()),
                Value::String("$x".into()),
            ],
            vec![
                Value::String("abc".into()),
                Value::String("a".into()),
                Value::String("x".repeat(MAX_REPLACEMENT_BYTES + 1)),
            ],
        ] {
            assert!(matches!(
                replace(&args),
                Err(FunctionError::InvalidArgument {
                    function: "replace",
                    ..
                })
            ));
        }
    }
}
