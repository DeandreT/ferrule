use super::{Decimal96, DecimalError, MAX_DECIMAL_LEXICAL_BYTES};

impl Decimal96 {
    /// Mapping decimal text: trim whitespace and shift exponent digits exactly
    /// before the single ties-to-even rounding. No binary64 conversion occurs.
    pub fn parse_decimal_text(text: &str) -> Result<Self, DecimalError> {
        if text.len() > MAX_DECIMAL_LEXICAL_BYTES {
            return Err(DecimalError::InputTooLong);
        }
        let text = text.trim();
        if !has_exponent(text)? {
            return Self::parse_rounded(text).map_err(|error| match error {
                DecimalError::InvalidLexical => DecimalError::InvalidNumberLexical,
                error => error,
            });
        }
        let (mantissa, exponent) = text
            .split_once(['e', 'E'])
            .ok_or(DecimalError::InvalidNumberLexical)?;
        // Past twice the input bound, no mantissa in this bounded input can
        // counteract the exponent. Saturation also admits arbitrarily long
        // zero-padded exponent spellings without integer overflow.
        let magnitude = exponent
            .trim_start_matches(['+', '-'])
            .bytes()
            .fold(0_i32, |value, digit| {
                (value * 10 + i32::from(digit - b'0')).min(8192)
            });
        let exponent = if exponent.starts_with('-') {
            -magnitude
        } else {
            magnitude
        };
        let negative = mantissa.starts_with('-');
        let mantissa = mantissa.trim_start_matches(['+', '-']);
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = [whole, fraction].concat();
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            return Ok(Self::ZERO);
        }
        let point = whole.len() as i32 + exponent - (digits.len() - significant.len()) as i32;
        if point > 29 {
            return Err(DecimalError::Overflow);
        }
        if point < -28 {
            return Ok(Self::ZERO);
        }
        let mut plain = String::new();
        if negative {
            plain.push('-');
        }
        if point <= 0 {
            plain.push_str("0.");
            plain.extend(std::iter::repeat_n('0', (-point) as usize));
            plain.push_str(significant);
        } else if point as usize >= significant.len() {
            plain.push_str(significant);
            plain.extend(std::iter::repeat_n('0', point as usize - significant.len()));
        } else {
            let (whole, fraction) = significant.split_at(point as usize);
            plain.push_str(whole);
            plain.push('.');
            plain.push_str(fraction);
        }
        // The original lexical bound is already checked; shifting the decimal
        // point may add at most 30 bytes to this private, validated spelling.
        Self::parse_digits(&plain, true)
    }

    /// Explicit numeric-text policy: trim Unicode whitespace, keep plain text
    /// in the rounded decimal domain, and route exponent forms through binary64.
    /// This does not change the stricter plain-text parsers or mapping admission.
    pub fn parse_number(text: &str) -> Result<Self, DecimalError> {
        if text.len() > MAX_DECIMAL_LEXICAL_BYTES {
            return Err(DecimalError::InputTooLong);
        }
        let text = text.trim();
        if matches!(text, "NaN" | "INF" | "-INF") {
            return Err(DecimalError::Overflow);
        }
        if has_exponent(text)? {
            let value = text
                .parse::<f64>()
                .map_err(|_| DecimalError::InvalidNumberLexical)?;
            Self::from_f64(value)
        } else {
            Self::parse_rounded(text).map_err(|error| match error {
                DecimalError::InvalidLexical => DecimalError::InvalidNumberLexical,
                error => error,
            })
        }
    }
}

fn has_exponent(text: &str) -> Result<bool, DecimalError> {
    let bytes = text.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let start = index;
    while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
        index += 1;
    }
    let mut digits = index - start;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
        }
        digits += index - start;
    }
    if digits == 0 {
        return Err(DecimalError::InvalidNumberLexical);
    }
    let exponent = matches!(bytes.get(index), Some(b'e' | b'E'));
    if exponent {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
        }
        if index == start {
            return Err(DecimalError::InvalidNumberLexical);
        }
    }
    if index != bytes.len() {
        return Err(DecimalError::InvalidNumberLexical);
    }
    Ok(exponent)
}
