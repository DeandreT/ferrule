//! Built-in function library (string, math, date, aggregate, node-set) used
//! by mapping graphs, plus hooks for user-defined functions.
//!
//! Covers the string/math/comparison/boolean core plus the scalar helpers
//! MapForce designs lean on (substring family, exists, round, ISO
//! date-from-datetime); more built-ins land alongside the formats/semantics
//! that need them. Aggregates (count/sum/...) are not here: they reduce
//! collections in scope context, so they live in the engine as
//! `mapping::Node::Aggregate`.

use ir::Value;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FunctionError {
    #[error("unknown function `{0}`")]
    UnknownFunction(String),
    #[error("`{function}` expected {expected} argument(s), got {got}")]
    ArityMismatch {
        function: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("`{function}` cannot accept a {got} argument")]
    TypeMismatch {
        function: &'static str,
        got: &'static str,
    },
    #[error("division by zero")]
    DivideByZero,
    #[error("`{function}` {message}")]
    InvalidArgument {
        function: &'static str,
        message: &'static str,
    },
}

/// Scalar builtin names accepted by [`call`], in editor display order.
pub const BUILTIN_NAMES: &[&str] = &[
    "concat",
    "upper",
    "lower",
    "trim",
    "left_trim",
    "right_trim",
    "length",
    "starts_with",
    "contains",
    "pad_string_left",
    "pad_string_right",
    "add",
    "subtract",
    "multiply",
    "divide",
    "equal",
    "not_equal",
    "less_than",
    "greater_than",
    "less_or_equal",
    "greater_or_equal",
    "and",
    "or",
    "not",
    "substring",
    "substring_before",
    "substring_after",
    "string",
    "format_number",
    "exists",
    "round",
    "date_from_datetime",
];

/// Whether `name` identifies a scalar builtin accepted by [`call`].
pub fn is_known(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "trim" => unary_string(args, "trim", |s| s.trim().to_string()),
        "left_trim" => unary_string(args, "left_trim", |s| {
            s.trim_start_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "right_trim" => unary_string(args, "right_trim", |s| {
            s.trim_end_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "length" => length(args),
        "starts_with" => binary_string(args, "starts_with", |a, b| a.starts_with(b)),
        "contains" => binary_string(args, "contains", |a, b| a.contains(b)),
        "pad_string_left" => pad_string(args, "pad_string_left", true),
        "pad_string_right" => pad_string(args, "pad_string_right", false),
        "add" => numeric(args, "add", |a, b| a + b, |a, b| a + b),
        "subtract" => numeric(args, "subtract", |a, b| a - b, |a, b| a - b),
        "multiply" => numeric(args, "multiply", |a, b| a * b, |a, b| a * b),
        "divide" => divide(args),
        "equal" => comparison(args, "equal", |o| o == std::cmp::Ordering::Equal),
        "not_equal" => comparison(args, "not_equal", |o| o != std::cmp::Ordering::Equal),
        "less_than" => comparison(args, "less_than", |o| o == std::cmp::Ordering::Less),
        "greater_than" => comparison(args, "greater_than", |o| o == std::cmp::Ordering::Greater),
        "less_or_equal" => comparison(args, "less_or_equal", |o| o != std::cmp::Ordering::Greater),
        "greater_or_equal" => {
            comparison(args, "greater_or_equal", |o| o != std::cmp::Ordering::Less)
        }
        "and" => binary_bool(args, "and", |a, b| a && b),
        "or" => binary_bool(args, "or", |a, b| a || b),
        "not" => unary_bool(args, "not", |a| !a),
        "substring" => substring(args),
        "substring_before" => split_string(args, "substring_before", true),
        "substring_after" => split_string(args, "substring_after", false),
        "string" => string(args),
        "format_number" => format_number(args),
        "exists" => exists(args),
        "round" => round(args),
        "date_from_datetime" => date_from_datetime(args),
        other => Err(FunctionError::UnknownFunction(other.to_string())),
    }
}

fn concat(args: &[Value]) -> Value {
    let mut out = String::new();
    for arg in args {
        match arg {
            Value::Null => {}
            Value::Bool(b) => out.push_str(&b.to_string()),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Float(f) => out.push_str(&f.to_string()),
            Value::String(s) => out.push_str(s),
        }
    }
    Value::String(out)
}

fn unary_string(
    args: &[Value],
    name: &'static str,
    f: impl Fn(&str) -> String,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::String(f(s))),
        [other] => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 1,
            got: args.len(),
        }),
    }
}

fn binary_string(
    args: &[Value],
    name: &'static str,
    f: impl Fn(&str, &str) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(a), Value::String(b)] => Ok(Value::Bool(f(a, b))),
        [a, b] => {
            let bad = if matches!(a, Value::String(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn binary_bool(
    args: &[Value],
    name: &'static str,
    f: impl Fn(bool, bool) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Bool(a), Value::Bool(b)] => Ok(Value::Bool(f(*a, *b))),
        [a, b] => {
            let bad = if matches!(a, Value::Bool(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn unary_bool(
    args: &[Value],
    name: &'static str,
    f: impl Fn(bool) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Bool(a)] => Ok(Value::Bool(f(*a))),
        [other] => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 1,
            got: args.len(),
        }),
    }
}

fn length(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::Int(s.chars().count() as i64)),
        [other] => Err(FunctionError::TypeMismatch {
            function: "length",
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: "length",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

fn string(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [value] => Ok(Value::String(scalar_text(value))),
        _ => Err(FunctionError::ArityMismatch {
            function: "string",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn format_number(args: &[Value]) -> Result<Value, FunctionError> {
    let (value, format, decimal_point, grouping_separator) = match args {
        [value, Value::String(format)] => (value, format.as_str(), '.', ','),
        [value, Value::String(format), decimal_point] => (
            value,
            format.as_str(),
            single_char(decimal_point, "format_number")?,
            ',',
        ),
        [
            value,
            Value::String(format),
            decimal_point,
            grouping_separator,
        ] => (
            value,
            format.as_str(),
            single_char(decimal_point, "format_number")?,
            single_char(grouping_separator, "format_number")?,
        ),
        [_, format, ..] if !matches!(format, Value::String(_)) => {
            return Err(FunctionError::TypeMismatch {
                function: "format_number",
                got: format.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "format_number",
                expected: 2,
                got: args.len(),
            });
        }
    };
    if decimal_point == grouping_separator {
        return Err(FunctionError::InvalidArgument {
            function: "format_number",
            message: "requires distinct decimal and grouping separators",
        });
    }
    let collides = |separator: char| {
        separator.is_ascii_digit() || ['#', ';', '%', '\u{2030}'].contains(&separator)
    };
    if collides(decimal_point) || collides(grouping_separator) {
        return Err(FunctionError::InvalidArgument {
            function: "format_number",
            message: "separator collides with a picture character",
        });
    }

    let subformats: Vec<_> = format.split(';').collect();
    if subformats.is_empty()
        || subformats.len() > 2
        || subformats.iter().any(|part| part.is_empty())
    {
        return Err(FunctionError::InvalidArgument {
            function: "format_number",
            message: "format requires one or two non-empty subformats",
        });
    }
    let pictures: Vec<_> = subformats
        .iter()
        .map(|part| NumberPicture::parse(part, decimal_point, grouping_separator))
        .collect::<Result<_, _>>()?;

    let negative = match value {
        Value::Int(value) => value.is_negative(),
        Value::Float(value) if value.is_finite() => value.is_sign_negative(),
        Value::Float(_) => {
            return Err(FunctionError::InvalidArgument {
                function: "format_number",
                message: "requires a finite number",
            });
        }
        other => {
            return Err(FunctionError::TypeMismatch {
                function: "format_number",
                got: other.type_name(),
            });
        }
    };
    let has_negative_subformat = pictures.len() == 2;
    let picture = if negative && has_negative_subformat {
        &pictures[1]
    } else {
        &pictures[0]
    };
    let (integer, mut fraction) = render_decimal(
        value,
        picture.multiplier_digits,
        picture.max_fraction_digits,
    );
    while fraction.len() > picture.min_fraction_digits && fraction.ends_with('0') {
        fraction.pop();
    }
    let mut integer = if integer == "0" && picture.min_integer_digits == 0 && !fraction.is_empty() {
        String::new()
    } else {
        format!("{integer:0>width$}", width = picture.min_integer_digits)
    };
    if integer.is_empty() && fraction.is_empty() {
        integer.push('0');
    }
    if let Some(size) = picture.grouping_size {
        integer = group_digits(&integer, size, grouping_separator);
    }

    let mut output = String::new();
    if negative && !has_negative_subformat {
        output.push('-');
    }
    output.push_str(picture.prefix);
    output.push_str(&integer);
    if !fraction.is_empty() {
        output.push(decimal_point);
        output.push_str(&fraction);
    }
    output.push_str(picture.suffix);
    Ok(Value::String(output))
}

struct NumberPicture<'a> {
    prefix: &'a str,
    suffix: &'a str,
    min_integer_digits: usize,
    min_fraction_digits: usize,
    max_fraction_digits: usize,
    grouping_size: Option<usize>,
    multiplier_digits: usize,
}

impl<'a> NumberPicture<'a> {
    fn parse(
        subformat: &'a str,
        decimal_point: char,
        grouping_separator: char,
    ) -> Result<Self, FunctionError> {
        let first = subformat
            .char_indices()
            .find(|(_, c)| {
                matches!(c, '0' | '#') || *c == decimal_point || *c == grouping_separator
            })
            .map(|(index, _)| index)
            .ok_or(FunctionError::InvalidArgument {
                function: "format_number",
                message: "format must contain a digit placeholder",
            })?;
        let last = subformat
            .char_indices()
            .rfind(|(_, c)| {
                matches!(c, '0' | '#') || *c == decimal_point || *c == grouping_separator
            })
            .map(|(index, c)| index + c.len_utf8())
            .expect("a first picture character implies a last one");
        let prefix = &subformat[..first];
        let suffix = &subformat[last..];
        let body = &subformat[first..last];

        if !body.chars().any(|c| matches!(c, '0' | '#'))
            || body
                .chars()
                .any(|c| !matches!(c, '0' | '#') && c != decimal_point && c != grouping_separator)
        {
            return Err(FunctionError::InvalidArgument {
                function: "format_number",
                message: "format contains an invalid numeric picture",
            });
        }
        let mut parts = body.split(decimal_point);
        let integer_picture = parts.next().unwrap_or_default();
        let fraction_picture = parts.next().unwrap_or_default();
        if parts.next().is_some() || fraction_picture.contains(grouping_separator) {
            return Err(FunctionError::InvalidArgument {
                function: "format_number",
                message: "format contains invalid decimal or grouping separators",
            });
        }

        let integer_digits: String = integer_picture
            .chars()
            .filter(|c| *c != grouping_separator)
            .collect();
        if !valid_integer_picture(&integer_digits) || !valid_fraction_picture(fraction_picture) {
            return Err(FunctionError::InvalidArgument {
                function: "format_number",
                message: "format contains placeholders in an invalid order",
            });
        }
        let grouping_size = parse_grouping(integer_picture, grouping_separator)?;

        let percent_count = subformat.chars().filter(|c| *c == '%').count();
        let per_mille_count = subformat.chars().filter(|c| *c == '\u{2030}').count();
        if percent_count > 1 || per_mille_count > 1 || percent_count + per_mille_count > 1 {
            return Err(FunctionError::InvalidArgument {
                function: "format_number",
                message: "format allows one percent or per-mille character",
            });
        }

        Ok(Self {
            prefix,
            suffix,
            min_integer_digits: integer_digits.chars().filter(|c| *c == '0').count(),
            min_fraction_digits: fraction_picture.chars().filter(|c| *c == '0').count(),
            max_fraction_digits: fraction_picture.chars().count(),
            grouping_size,
            multiplier_digits: if percent_count == 1 {
                2
            } else if per_mille_count == 1 {
                3
            } else {
                0
            },
        })
    }
}

fn valid_integer_picture(picture: &str) -> bool {
    let mut mandatory = false;
    picture.chars().all(|c| match c {
        '#' if !mandatory => true,
        '0' => {
            mandatory = true;
            true
        }
        _ => false,
    })
}

fn valid_fraction_picture(picture: &str) -> bool {
    let mut optional = false;
    picture.chars().all(|c| match c {
        '0' if !optional => true,
        '#' => {
            optional = true;
            true
        }
        _ => false,
    })
}

fn parse_grouping(picture: &str, separator: char) -> Result<Option<usize>, FunctionError> {
    if !picture.contains(separator) {
        return Ok(None);
    }
    let groups: Vec<_> = picture.split(separator).collect();
    if groups.iter().any(|group| group.is_empty()) {
        return Err(FunctionError::InvalidArgument {
            function: "format_number",
            message: "format contains misplaced grouping separators",
        });
    }
    let size = groups.last().map_or(0, |group| group.chars().count());
    Ok(Some(size))
}

fn render_decimal(
    value: &Value,
    multiplier_digits: usize,
    fraction_digits: usize,
) -> (String, String) {
    let magnitude = match value {
        Value::Int(value) => value.unsigned_abs().to_string(),
        Value::Float(value) => value.abs().to_string(),
        _ => unreachable!("format_number checks its numeric argument"),
    };
    let (mantissa, exponent) = magnitude
        .split_once(['e', 'E'])
        .map_or((magnitude.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap_or(0))
        });
    let (whole, fractional) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = String::with_capacity(whole.len() + fractional.len());
    digits.push_str(whole);
    digits.push_str(fractional);
    let decimal_position = whole.len() as i32 + exponent + multiplier_digits as i32;

    let (mut integer, fraction) = if decimal_position <= 0 {
        let mut fraction = String::with_capacity((-decimal_position) as usize + digits.len());
        fraction.extend(std::iter::repeat_n('0', (-decimal_position) as usize));
        fraction.push_str(&digits);
        ("0".to_string(), fraction)
    } else if decimal_position as usize >= digits.len() {
        digits.extend(std::iter::repeat_n(
            '0',
            decimal_position as usize - digits.len(),
        ));
        (digits, String::new())
    } else {
        let fraction = digits.split_off(decimal_position as usize);
        (digits, fraction)
    };
    while integer.len() > 1 && integer.starts_with('0') {
        integer.remove(0);
    }

    let mut combined = integer;
    combined.push_str(&fraction);
    let integer_len = combined.len() - fraction.len();
    let keep = integer_len + fraction_digits;
    let round_up = combined
        .as_bytes()
        .get(keep)
        .is_some_and(|digit| *digit >= b'5');
    combined.truncate(keep);
    combined.extend(std::iter::repeat_n(
        '0',
        keep.saturating_sub(combined.len()),
    ));
    if round_up {
        increment_decimal(&mut combined);
    }
    let integer_len = if combined.len() > keep {
        integer_len + 1
    } else {
        integer_len
    };
    if combined.len() < integer_len {
        combined.extend(std::iter::repeat_n('0', integer_len - combined.len()));
    }
    let fraction = combined.split_off(integer_len);
    (combined, fraction)
}

fn increment_decimal(digits: &mut String) {
    let mut bytes = std::mem::take(digits).into_bytes();
    for digit in bytes.iter_mut().rev() {
        if *digit < b'9' {
            *digit += 1;
            *digits = String::from_utf8(bytes).expect("decimal digits are ASCII");
            return;
        }
        *digit = b'0';
    }
    bytes.insert(0, b'1');
    *digits = String::from_utf8(bytes).expect("decimal digits are ASCII");
}

fn single_char(value: &Value, name: &'static str) -> Result<char, FunctionError> {
    let Value::String(value) = value else {
        return Err(FunctionError::TypeMismatch {
            function: name,
            got: value.type_name(),
        });
    };
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "separator must be one character",
        });
    };
    if chars.next().is_some() {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "separator must be one character",
        });
    }
    Ok(first)
}

fn group_digits(digits: &str, size: usize, separator: char) -> String {
    let mut reversed = String::with_capacity(digits.len() + digits.len() / size);
    for (index, digit) in digits.chars().rev().enumerate() {
        if index > 0 && index % size == 0 {
            reversed.push(separator);
        }
        reversed.push(digit);
    }
    reversed.chars().rev().collect()
}

fn pad_string(args: &[Value], name: &'static str, left: bool) -> Result<Value, FunctionError> {
    let [value, desired_length, padding] = args else {
        return Err(FunctionError::ArityMismatch {
            function: name,
            expected: 3,
            got: args.len(),
        });
    };
    let desired_length = number_arg(desired_length, name)? as i64;
    let padding = scalar_text(padding);
    let mut padding = padding.chars();
    let Some(padding_char) = padding.next() else {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "requires one padding character",
        });
    };
    if padding.next().is_some() {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "requires one padding character",
        });
    }

    let value = scalar_text(value);
    let count = desired_length
        .saturating_sub(value.chars().count() as i64)
        .max(0) as usize;
    let padding: String = std::iter::repeat_n(padding_char, count).collect();
    Ok(Value::String(if left {
        padding + &value
    } else {
        value + &padding
    }))
}

fn numeric(
    args: &[Value],
    name: &'static str,
    f_int: impl Fn(i64, i64) -> i64,
    f_float: impl Fn(f64, f64) -> f64,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int(f_int(*a, *b))),
        [Value::Float(a), Value::Float(b)] => Ok(Value::Float(f_float(*a, *b))),
        [Value::Int(a), Value::Float(b)] => Ok(Value::Float(f_float(*a as f64, *b))),
        [Value::Float(a), Value::Int(b)] => Ok(Value::Float(f_float(*a, *b as f64))),
        [Value::Int(_) | Value::Float(_), b] => Err(FunctionError::TypeMismatch {
            function: name,
            got: b.type_name(),
        }),
        [a, _] => Err(FunctionError::TypeMismatch {
            function: name,
            got: a.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn divide(args: &[Value]) -> Result<Value, FunctionError> {
    let (a, b) = match args {
        [Value::Int(a), Value::Int(b)] => (*a as f64, *b as f64),
        [Value::Float(a), Value::Float(b)] => (*a, *b),
        [Value::Int(a), Value::Float(b)] => (*a as f64, *b),
        [Value::Float(a), Value::Int(b)] => (*a, *b as f64),
        [Value::Int(_) | Value::Float(_), b] => {
            return Err(FunctionError::TypeMismatch {
                function: "divide",
                got: b.type_name(),
            });
        }
        [a, _] => {
            return Err(FunctionError::TypeMismatch {
                function: "divide",
                got: a.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "divide",
                expected: 2,
                got: args.len(),
            });
        }
    };
    if b == 0.0 {
        return Err(FunctionError::DivideByZero);
    }
    Ok(Value::Float(a / b))
}

fn number_arg(value: &Value, name: &'static str) -> Result<f64, FunctionError> {
    match value {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        other => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
    }
}

/// XPath-style `substring(string, start[, length])`: positions are 1-based
/// and both numbers round to the nearest integer.
fn substring(args: &[Value]) -> Result<Value, FunctionError> {
    let (s, start, len) = match args {
        [Value::String(s), start] => (s, start, None),
        [Value::String(s), start, len] => (s, start, Some(len)),
        [other, ..] if !matches!(other, Value::String(_)) => {
            return Err(FunctionError::TypeMismatch {
                function: "substring",
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "substring",
                expected: 2,
                got: args.len(),
            });
        }
    };
    let start = number_arg(start, "substring")?.round() as i64;
    let end = match len {
        Some(len) => Some(start + number_arg(len, "substring")?.round() as i64),
        None => None,
    };
    let out: String = s
        .chars()
        .enumerate()
        .filter(|(i, _)| {
            let pos = *i as i64 + 1;
            pos >= start && end.is_none_or(|e| pos < e)
        })
        .map(|(_, c)| c)
        .collect();
    Ok(Value::String(out))
}

/// `substring-before` / `substring-after`: the part of the string on one
/// side of the first occurrence of the separator; no match yields "".
fn split_string(args: &[Value], name: &'static str, before: bool) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s), Value::String(sep)] => {
            let part = match s.find(sep.as_str()) {
                Some(idx) if before => &s[..idx],
                Some(idx) => &s[idx + sep.len()..],
                None => "",
            };
            Ok(Value::String(part.to_string()))
        }
        [a, b] => {
            let bad = if matches!(a, Value::String(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

/// Whether a value is present -- with the lenient readers, an absent
/// source node arrives as `Null`.
fn exists(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [value] => Ok(Value::Bool(!matches!(value, Value::Null))),
        _ => Err(FunctionError::ArityMismatch {
            function: "exists",
            expected: 1,
            got: args.len(),
        }),
    }
}

/// `round(x)` rounds to the nearest integer; `round(x, digits)` to that
/// many decimal places (MapForce's `round-precision`).
fn round(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(i)] => Ok(Value::Int(*i)),
        [x] => Ok(Value::Float(number_arg(x, "round")?.round())),
        [x, digits] => {
            let x = number_arg(x, "round")?;
            let factor = 10f64.powi(number_arg(digits, "round")?.round() as i32);
            Ok(Value::Float((x * factor).round() / factor))
        }
        _ => Err(FunctionError::ArityMismatch {
            function: "round",
            expected: 1,
            got: args.len(),
        }),
    }
}

/// The date part of an ISO datetime string ("2024-03-01T10:30:00" ->
/// "2024-03-01"); values without a time part pass through unchanged.
fn date_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::String(
            s.split('T').next().unwrap_or(s).trim().to_string(),
        )),
        [other] => Err(FunctionError::TypeMismatch {
            function: "date_from_datetime",
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: "date_from_datetime",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn value_ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn comparison(
    args: &[Value],
    name: &'static str,
    matches: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [a, b] => match value_ordering(a, b) {
            Some(ordering) => Ok(Value::Bool(matches(ordering))),
            None => Err(FunctionError::TypeMismatch {
                function: name,
                got: b.type_name(),
            }),
        },
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_joins_mixed_scalar_types() {
        let result = call(
            "concat",
            &[
                Value::String("Jane".to_string()),
                Value::String(" ".to_string()),
                Value::String("Doe".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(result, Value::String("Jane Doe".to_string()));
    }

    #[test]
    fn add_promotes_int_and_float() {
        assert_eq!(
            call("add", &[Value::Int(29), Value::Int(1)]).unwrap(),
            Value::Int(30)
        );
        assert_eq!(
            call("add", &[Value::Int(1), Value::Float(1.5)]).unwrap(),
            Value::Float(2.5)
        );
    }

    #[test]
    fn upper_rejects_non_string_argument() {
        assert_eq!(
            call("upper", &[Value::Int(1)]),
            Err(FunctionError::TypeMismatch {
                function: "upper",
                got: "int"
            })
        );
    }

    #[test]
    fn unknown_function_is_reported() {
        assert_eq!(
            call("nope", &[]),
            Err(FunctionError::UnknownFunction("nope".to_string()))
        );
        assert!(!is_known("nope"));
        assert!(is_known("concat"));
        for name in BUILTIN_NAMES {
            assert!(
                !matches!(call(name, &[]), Err(FunctionError::UnknownFunction(_))),
                "catalog entry `{name}` is not dispatched"
            );
        }
    }

    #[test]
    fn comparisons_coerce_int_and_float() {
        assert_eq!(
            call("greater_or_equal", &[Value::Int(65), Value::Int(60)]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            call("less_than", &[Value::Int(1), Value::Float(1.5)]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            call(
                "equal",
                &[Value::String("a".into()), Value::String("b".into())]
            )
            .unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn divide_by_zero_is_an_error() {
        assert_eq!(
            call("divide", &[Value::Int(1), Value::Int(0)]),
            Err(FunctionError::DivideByZero)
        );
    }

    #[test]
    fn string_predicates() {
        assert_eq!(
            call(
                "starts_with",
                &[
                    Value::String("Jane Doe".into()),
                    Value::String("Jane".into())
                ]
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            call("length", &[Value::String("Jane".into())]).unwrap(),
            Value::Int(4)
        );
    }

    #[test]
    fn padding_and_directional_trim_follow_character_semantics() {
        assert_eq!(
            call(
                "pad_string_left",
                &[Value::Int(7), Value::Float(3.0), Value::String("0".into())]
            )
            .unwrap(),
            Value::String("007".into())
        );
        assert_eq!(
            call(
                "pad_string_right",
                &[
                    Value::String("AP".into()),
                    Value::Int(4),
                    Value::String("Z".into())
                ]
            )
            .unwrap(),
            Value::String("APZZ".into())
        );
        assert_eq!(
            call(
                "pad_string_left",
                &[
                    Value::String("AP".into()),
                    Value::Int(-3),
                    Value::String("Z".into())
                ]
            )
            .unwrap(),
            Value::String("AP".into())
        );
        assert!(matches!(
            call(
                "pad_string_left",
                &[
                    Value::String("AP".into()),
                    Value::Int(3),
                    Value::String("YZ".into())
                ]
            ),
            Err(FunctionError::InvalidArgument { .. })
        ));
        assert_eq!(
            call("left_trim", &[Value::String(" \t\nvalue\u{a0}".into())]).unwrap(),
            Value::String("value\u{a0}".into())
        );
        assert_eq!(
            call("right_trim", &[Value::String("\u{a0}value\r\n ".into())]).unwrap(),
            Value::String("\u{a0}value".into())
        );
    }

    #[test]
    fn string_converts_every_scalar_value() {
        assert_eq!(
            call("string", &[Value::String("ferrule".into())]).unwrap(),
            Value::String("ferrule".into())
        );
        assert_eq!(
            call("string", &[Value::Bool(true)]).unwrap(),
            Value::String("true".into())
        );
        assert_eq!(
            call("string", &[Value::Int(42)]).unwrap(),
            Value::String("42".into())
        );
        assert_eq!(
            call("string", &[Value::Null]).unwrap(),
            Value::String(String::new())
        );
    }

    #[test]
    fn format_number_applies_decimal_grouping_and_optional_digits() {
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(1234.5), Value::String("#,##0.00".into())]
            )
            .unwrap(),
            Value::String("1,234.50".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(123.456), Value::String("#,##0.00".into())]
            )
            .unwrap(),
            Value::String("123.46".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(0.00025), Value::String("###0.0###".into())]
            )
            .unwrap(),
            Value::String("0.0003".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Int(25), Value::String("00000.00".into())]
            )
            .unwrap(),
            Value::String("00025.00".into())
        );
    }

    #[test]
    fn format_number_supports_subformats_scaling_and_custom_separators() {
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(0.736), Value::String("#00%".into())]
            )
            .unwrap(),
            Value::String("74%".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(-3.12), Value::String("#.00;(#.00)".into())]
            )
            .unwrap(),
            Value::String("(3.12)".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[
                    Value::Float(1234.5),
                    Value::String("#.##0,00".into()),
                    Value::String(",".into()),
                    Value::String(".".into()),
                ]
            )
            .unwrap(),
            Value::String("1.234,50".into())
        );
    }

    #[test]
    fn format_number_preserves_integers_and_rounds_shortest_decimals_half_up() {
        assert_eq!(
            call(
                "format_number",
                &[
                    Value::Int(9_007_199_254_740_993),
                    Value::String("#,##0".into()),
                ]
            )
            .unwrap(),
            Value::String("9,007,199,254,740,993".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Int(i64::MIN), Value::String("0".into())]
            )
            .unwrap(),
            Value::String("-9223372036854775808".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(1.005), Value::String("0.00".into())]
            )
            .unwrap(),
            Value::String("1.01".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Float(0.00035), Value::String("###0.0###".into()),]
            )
            .unwrap(),
            Value::String("0.0004".into())
        );
    }

    #[test]
    fn format_number_handles_optional_zero_and_extreme_finite_values() {
        assert_eq!(
            call(
                "format_number",
                &[Value::Int(0), Value::String("#.##".into())]
            )
            .unwrap(),
            Value::String("0".into())
        );
        assert_eq!(
            call(
                "format_number",
                &[Value::Int(0), Value::String("$#.## USD".into())]
            )
            .unwrap(),
            Value::String("$0 USD".into())
        );

        let precision = 400;
        let format = format!("0.{}", "0".repeat(precision));
        let Value::String(rendered) =
            call("format_number", &[Value::Float(1.0), Value::String(format)]).unwrap()
        else {
            panic!("format_number must return a string");
        };
        assert_eq!(rendered, format!("1.{}", "0".repeat(precision)));

        let Value::String(rendered) = call(
            "format_number",
            &[Value::Float(f64::MAX), Value::String("0%".into())],
        )
        .unwrap() else {
            panic!("format_number must return a string");
        };
        assert!(rendered.ends_with('%'));
        assert!(!rendered.contains("inf"));
        assert!(!rendered.contains("NaN"));
    }

    #[test]
    fn format_number_rejects_invalid_pictures_and_separator_collisions() {
        for format in ["#0#", ".#0", "0;0;0", "0;;0", "0%%", "0%\u{2030}", "#,,##0"] {
            assert!(matches!(
                call(
                    "format_number",
                    &[Value::Int(1), Value::String(format.into())]
                ),
                Err(FunctionError::InvalidArgument { .. })
            ));
        }
        assert!(matches!(
            call(
                "format_number",
                &[Value::Int(1), Value::String("0;#0#".into())]
            ),
            Err(FunctionError::InvalidArgument { .. })
        ));
        assert!(matches!(
            call(
                "format_number",
                &[
                    Value::Int(1),
                    Value::String("0".into()),
                    Value::String("#".into()),
                ]
            ),
            Err(FunctionError::InvalidArgument { .. })
        ));
        assert_eq!(
            call(
                "format_number",
                &[Value::Int(1_234_567), Value::String("####,##0".into()),]
            )
            .unwrap(),
            Value::String("1,234,567".into())
        );
    }

    #[test]
    fn substring_family_follows_xpath_conventions() {
        assert_eq!(
            call(
                "substring",
                &[Value::String("motor car".into()), Value::Int(6)]
            )
            .unwrap(),
            Value::String(" car".into())
        );
        assert_eq!(
            call(
                "substring",
                &[
                    Value::String("metadata".into()),
                    Value::Int(4),
                    Value::Int(3)
                ]
            )
            .unwrap(),
            Value::String("ada".into())
        );
        assert_eq!(
            call(
                "substring_before",
                &[
                    Value::String("1999-04-01".into()),
                    Value::String("-".into())
                ]
            )
            .unwrap(),
            Value::String("1999".into())
        );
        assert_eq!(
            call(
                "substring_after",
                &[
                    Value::String("1999-04-01".into()),
                    Value::String("-".into())
                ]
            )
            .unwrap(),
            Value::String("04-01".into())
        );
        // No match yields the empty string, not an error.
        assert_eq!(
            call(
                "substring_before",
                &[Value::String("abc".into()), Value::String("|".into())]
            )
            .unwrap(),
            Value::String("".into())
        );
    }

    #[test]
    fn exists_distinguishes_null() {
        assert_eq!(call("exists", &[Value::Null]).unwrap(), Value::Bool(false));
        assert_eq!(
            call("exists", &[Value::String("".into())]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn round_supports_precision() {
        assert_eq!(
            call("round", &[Value::Float(2.5)]).unwrap(),
            Value::Float(3.0)
        );
        assert_eq!(call("round", &[Value::Int(7)]).unwrap(), Value::Int(7));
        assert_eq!(
            call("round", &[Value::Float(1.23456), Value::Int(2)]).unwrap(),
            Value::Float(1.23)
        );
    }

    #[test]
    fn date_from_datetime_takes_the_date_part() {
        assert_eq!(
            call(
                "date_from_datetime",
                &[Value::String("2024-03-01T10:30:00".into())]
            )
            .unwrap(),
            Value::String("2024-03-01".into())
        );
        assert_eq!(
            call("date_from_datetime", &[Value::String("2024-03-01".into())]).unwrap(),
            Value::String("2024-03-01".into())
        );
    }

    #[test]
    fn boolean_combinators() {
        assert_eq!(
            call("and", &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            call("or", &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            call("not", &[Value::Bool(true)]).unwrap(),
            Value::Bool(false)
        );
    }
}
