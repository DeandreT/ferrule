use super::{Decimal96, DecimalError, MAX_DECIMAL_LEXICAL_BYTES};

impl Decimal96 {
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
