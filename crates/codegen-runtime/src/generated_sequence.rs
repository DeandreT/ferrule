use crate::{FunctionError, RuntimeError, Value};
use regex::RegexBuilder;

pub const MAX_GENERATED_SEQUENCE_ITEMS: u128 = 1_000_000;
pub const MAX_RECURSIVE_SEQUENCE_DEPTH: usize = 256;
const MAX_TOKENIZE_REGEX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_TOKENIZE_REGEX_COMPILED_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct RecursiveCollectPaths<'a> {
    pub collection: &'a [&'a str],
    pub children: &'a [&'a str],
    pub descent_value: &'a [&'a str],
    pub values: &'a [&'a str],
    pub value: &'a [&'a str],
}

pub fn recursive_sequence_parameter(value: Value) -> Result<String, RuntimeError> {
    match value {
        Value::Null => Ok(String::new()),
        value => recursive_scalar_text(&value),
    }
}

pub fn recursive_collect(
    context: &crate::ScopeContext<'_>,
    paths: RecursiveCollectPaths<'_>,
    prefix: &str,
    separator: &str,
) -> Result<Vec<Value>, RuntimeError> {
    context.recursive_collect(paths, prefix, separator)
}

pub(crate) fn recursive_scalar_text(value: &Value) -> Result<String, RuntimeError> {
    match value {
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Null | Value::XmlNil(_) | Value::Float(_) => Err(FunctionError::TypeMismatch {
            function: "recursive-collect",
            got: value.type_name(),
        }
        .into()),
    }
}

/// Splits a string around one literal delimiter while preserving empty items.
pub fn tokenize(input: Value, delimiter: Value) -> Result<Vec<Value>, RuntimeError> {
    let input = sequence_string(input, "tokenize")?;
    let delimiter = sequence_string(delimiter, "tokenize")?;
    if delimiter.is_empty() {
        return Err(FunctionError::InvalidArgument {
            function: "tokenize",
            message: "requires a non-empty delimiter",
        }
        .into());
    }
    Ok(input
        .split(&delimiter)
        .map(|value| Value::String(value.to_string()))
        .collect())
}

/// Chunks a string by Unicode scalar count, retaining a final short item.
pub fn tokenize_by_length(input: Value, length: Value) -> Result<Vec<Value>, RuntimeError> {
    let input = sequence_string(input, "tokenize-by-length")?;
    let length = match length {
        Value::Int(value) => Some(value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse().ok(),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) | Value::Float(_) => None,
    }
    .filter(|length| *length > 0)
    .ok_or(FunctionError::InvalidArgument {
        function: "tokenize-by-length",
        message: "requires a positive integer length",
    })? as usize;

    let chars = input.chars().collect::<Vec<_>>();
    Ok(chars
        .chunks(length)
        .map(|chunk| Value::String(chunk.iter().collect()))
        .collect())
}

/// Splits text with the bounded XPath-compatible regular-expression flags.
pub fn tokenize_regex(
    input: Value,
    pattern: Value,
    flags: Option<Value>,
) -> Result<Vec<Value>, RuntimeError> {
    tokenize_regex_with_limit(input, pattern, flags, MAX_GENERATED_SEQUENCE_ITEMS as usize)
}

fn tokenize_regex_with_limit(
    input: Value,
    pattern: Value,
    flags: Option<Value>,
    max_items: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let input = sequence_string(input, "tokenize-regexp")?;
    let pattern = sequence_string(pattern, "tokenize-regexp")?;
    let flags = flags
        .map(|value| sequence_string(value, "tokenize-regexp"))
        .transpose()?
        .unwrap_or_default();
    if pattern.len() > MAX_TOKENIZE_REGEX_PATTERN_BYTES {
        return Err(RuntimeError::TokenizeRegexPatternTooLarge {
            bytes: pattern.len(),
            max: MAX_TOKENIZE_REGEX_PATTERN_BYTES,
        });
    }

    let mut builder = RegexBuilder::new(&pattern);
    for flag in flags.chars() {
        let apply: fn(&mut RegexBuilder, bool) -> &mut RegexBuilder = match flag {
            'i' => RegexBuilder::case_insensitive,
            'm' => RegexBuilder::multi_line,
            's' => RegexBuilder::dot_matches_new_line,
            'x' => RegexBuilder::ignore_whitespace,
            _ => return Err(RuntimeError::InvalidTokenizeRegexFlags { flags }),
        };
        apply(&mut builder, true);
    }
    let regex = builder
        .size_limit(MAX_TOKENIZE_REGEX_COMPILED_BYTES)
        .dfa_size_limit(MAX_TOKENIZE_REGEX_COMPILED_BYTES)
        .build()
        .map_err(|error| RuntimeError::InvalidTokenizeRegex {
            message: error.to_string(),
        })?;
    if regex.is_match("")
        || regex
            .find_iter(&input)
            .any(|matched| matched.start() == matched.end())
    {
        return Err(RuntimeError::ZeroWidthTokenizeRegex);
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let values = regex
        .split(&input)
        .take(max_items.saturating_add(1))
        .map(|value| Value::String(value.to_string()))
        .collect::<Vec<_>>();
    if values.len() > max_items {
        return Err(RuntimeError::TokenizeRegexTooLarge {
            max: max_items as u128,
        });
    }
    Ok(values)
}

/// Generates an inclusive integer range with the engine's one-million-item
/// materialization bound. An absent lower bound defaults to one.
pub fn generate_sequence(from: Option<Value>, to: Value) -> Result<Vec<Value>, RuntimeError> {
    let from = from.map_or(Ok(1), sequence_integer)?;
    let to = sequence_integer(to)?;
    if from > to {
        return Ok(Vec::new());
    }
    let requested = (i128::from(to) - i128::from(from) + 1) as u128;
    if requested > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(RuntimeError::GeneratedSequenceTooLarge {
            requested,
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    Ok((from..=to).map(Value::Int).collect())
}

fn sequence_string(value: Value, function: &'static str) -> Result<String, RuntimeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(FunctionError::TypeMismatch {
            function,
            got: value.type_name(),
        }
        .into()),
    }
}

fn sequence_integer(value: Value) -> Result<i64, RuntimeError> {
    let coerced = match &value {
        Value::Int(value) => Some(*value),
        Value::Float(value) => exact_float_integer(*value),
        Value::String(value) => value.trim().parse::<i64>().ok().or_else(|| {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(exact_float_integer)
        }),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) => None,
    };
    coerced.ok_or_else(|| {
        FunctionError::TypeMismatch {
            function: "generate-sequence",
            got: value.type_name(),
        }
        .into()
    })
}

fn exact_float_integer(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < i64::MAX as f64)
        .then_some(value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instance, ScopeContext, field, group, repeated, scalar};
    use ir::XmlNil;

    #[test]
    fn literal_tokenize_preserves_empty_items_and_typed_failures() {
        assert_eq!(
            tokenize(Value::String("a,,b,".into()), Value::String(",".into())),
            Ok(vec![
                Value::String("a".into()),
                Value::String(String::new()),
                Value::String("b".into()),
                Value::String(String::new()),
            ])
        );
        assert_eq!(
            tokenize(Value::String(String::new()), Value::String("/".into())),
            Ok(vec![Value::String(String::new())])
        );
        assert!(matches!(
            tokenize(Value::Int(1), Value::String(",".into())),
            Err(RuntimeError::Function(FunctionError::TypeMismatch {
                function: "tokenize",
                got: "int"
            }))
        ));
        assert!(matches!(
            tokenize(Value::String("a".into()), Value::String(String::new())),
            Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "tokenize",
                ..
            }))
        ));
    }

    #[test]
    fn length_tokenize_uses_unicode_scalars_and_engine_coercions() {
        assert_eq!(
            tokenize_by_length(Value::String("aé🙂z".into()), Value::Float(2.9)),
            Ok(vec![
                Value::String("aé".into()),
                Value::String("🙂z".into()),
            ])
        );
        assert_eq!(
            tokenize_by_length(Value::String(String::new()), Value::Int(2)),
            Ok(Vec::new())
        );
        assert!(matches!(
            tokenize_by_length(Value::String("abc".into()), Value::String("2.0".into())),
            Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "tokenize-by-length",
                ..
            }))
        ));
    }

    #[test]
    fn regex_tokenize_is_bounded_and_preserves_typed_failures() {
        assert_eq!(
            tokenize_regex(
                Value::String("Alpha--beta---GAMMA".into()),
                Value::String("-+ BETA -+".into()),
                Some(Value::String("ix".into())),
            ),
            Ok(vec![
                Value::String("Alpha".into()),
                Value::String("GAMMA".into()),
            ])
        );
        assert_eq!(
            tokenize_regex(
                Value::String("--a--".into()),
                Value::String("-+".into()),
                None,
            ),
            Ok(vec![
                Value::String(String::new()),
                Value::String("a".into()),
                Value::String(String::new()),
            ])
        );
        assert_eq!(
            tokenize_regex(
                Value::String(String::new()),
                Value::String(",".into()),
                None,
            ),
            Ok(Vec::new())
        );
        assert!(matches!(
            tokenize_regex(Value::Int(1), Value::String(",".into()), None),
            Err(RuntimeError::Function(FunctionError::TypeMismatch {
                function: "tokenize-regexp",
                got: "int",
            }))
        ));
        assert!(matches!(
            tokenize_regex(
                Value::String("abc".into()),
                Value::String("a".into()),
                Some(Value::String("q".into())),
            ),
            Err(RuntimeError::InvalidTokenizeRegexFlags { .. })
        ));
        assert!(matches!(
            tokenize_regex(Value::String("abc".into()), Value::String("(".into()), None,),
            Err(RuntimeError::InvalidTokenizeRegex { .. })
        ));
        assert_eq!(
            tokenize_regex(
                Value::String("abc".into()),
                Value::String(r"\b".into()),
                None,
            ),
            Err(RuntimeError::ZeroWidthTokenizeRegex)
        );
        assert!(matches!(
            tokenize_regex(
                Value::String("abc".into()),
                Value::String("a".repeat(MAX_TOKENIZE_REGEX_PATTERN_BYTES + 1)),
                None,
            ),
            Err(RuntimeError::TokenizeRegexPatternTooLarge { .. })
        ));
        assert_eq!(
            tokenize_regex_with_limit(
                Value::String("a,b,c".into()),
                Value::String(",".into()),
                None,
                2,
            ),
            Err(RuntimeError::TokenizeRegexTooLarge { max: 2 })
        );
    }

    #[test]
    fn inclusive_ranges_default_descend_and_bound_without_overflow() {
        assert_eq!(
            generate_sequence(None, Value::Int(3)),
            Ok(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(
            generate_sequence(Some(Value::String("-2.0".into())), Value::Float(0.0)),
            Ok(vec![Value::Int(-2), Value::Int(-1), Value::Int(0)])
        );
        assert_eq!(
            generate_sequence(Some(Value::Int(3)), Value::Int(2)),
            Ok(Vec::new())
        );
        assert_eq!(
            generate_sequence(Some(Value::Int(i64::MIN)), Value::Int(i64::MAX)),
            Err(RuntimeError::GeneratedSequenceTooLarge {
                requested: 1_u128 << 64,
                max: MAX_GENERATED_SEQUENCE_ITEMS,
            })
        );
    }

    #[test]
    fn recursive_collect_is_preorder_and_preserves_parent_prefixes() {
        let directory =
            |name: &str, files: &[&str], children: Vec<Instance>| {
                group([
                    field("name", scalar(Value::String(name.into()))),
                    field(
                        "file",
                        repeated(files.iter().map(|file| {
                            group([field("name", scalar(Value::String((*file).into())))])
                        })),
                    ),
                    field("directory", repeated(children)),
                ])
            };
        let source = directory(
            "root",
            &["top.txt", "second.txt"],
            vec![directory("child", &["nested.txt"], Vec::new())],
        );
        let context = ScopeContext::new(&source);

        assert_eq!(
            recursive_collect(
                &context,
                RecursiveCollectPaths {
                    collection: &[],
                    children: &["directory"],
                    descent_value: &["name"],
                    values: &["file"],
                    value: &["name"],
                },
                "",
                "\\",
            ),
            Ok(vec![
                Value::String("\\root\\top.txt".into()),
                Value::String("\\root\\second.txt".into()),
                Value::String("\\root\\child\\nested.txt".into()),
            ])
        );
    }

    #[test]
    fn recursive_parameters_default_null_and_reject_non_scalars() {
        assert_eq!(recursive_sequence_parameter(Value::Null), Ok(String::new()));
        assert_eq!(
            recursive_sequence_parameter(Value::Bool(false)),
            Ok("false".into())
        );
        assert_eq!(
            recursive_sequence_parameter(Value::Float(12.5)),
            Ok("12.5".into())
        );
        assert!(matches!(
            recursive_sequence_parameter(Value::XmlNil(XmlNil)),
            Err(RuntimeError::Function(FunctionError::TypeMismatch {
                function: "recursive-collect",
                got: "xml nil"
            }))
        ));
    }

    #[test]
    fn recursive_collect_enforces_depth_before_pruning() {
        let mut source = group([
            field("name", scalar(Value::String("leaf".into()))),
            field("file", repeated(Vec::<Instance>::new())),
            field("directory", repeated(Vec::<Instance>::new())),
        ]);
        for depth in 0..MAX_RECURSIVE_SEQUENCE_DEPTH {
            source = group([
                field("name", scalar(Value::String(format!("level-{depth}")))),
                field("file", repeated(Vec::<Instance>::new())),
                field("directory", repeated([source])),
            ]);
        }
        let context = ScopeContext::new(&source);
        assert_eq!(
            recursive_collect(
                &context,
                RecursiveCollectPaths {
                    collection: &[],
                    children: &["directory"],
                    descent_value: &["name"],
                    values: &["file"],
                    value: &["name"],
                },
                "",
                "/",
            ),
            Err(RuntimeError::RecursiveSequenceDepth {
                limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
            })
        );
    }
}
