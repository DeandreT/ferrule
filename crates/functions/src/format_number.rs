use ir::Value;

use crate::FunctionError;

pub(super) fn format_number(args: &[Value]) -> Result<Value, FunctionError> {
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
