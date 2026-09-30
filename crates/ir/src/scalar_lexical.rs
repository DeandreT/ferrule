/// Parses an integer-valued decimal lexical without passing through `f64`.
/// This preserves exact values beyond the floating-point mantissa range and
/// rejects fractional or out-of-range results.
pub fn parse_exact_decimal_i64(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(value) = value.parse::<i64>() {
        return Some(value);
    }
    let (negative, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        Some(_) => (false, value),
        None => return None,
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => {
            let exponent = unsigned.get(index + 1..)?.parse::<i64>().ok()?;
            if unsigned[index + 1..].contains(['e', 'E']) {
                return None;
            }
            (&unsigned[..index], exponent)
        }
        None => (unsigned, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
        Some(_) => return None,
        None => (mantissa, ""),
    };
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    let mut digits = format!("{whole}{fraction}");
    let first_nonzero = digits.bytes().position(|byte| byte != b'0');
    let Some(first_nonzero) = first_nonzero else {
        return Some(0);
    };
    digits.drain(..first_nonzero);
    let scale = i128::try_from(fraction.len()).ok()? - i128::from(exponent);
    if scale > 0 {
        let scale = usize::try_from(scale).ok()?;
        let integer_length = digits.len().checked_sub(scale)?;
        if !digits[integer_length..].bytes().all(|byte| byte == b'0') {
            return None;
        }
        digits.truncate(integer_length);
    } else if scale < 0 {
        let zeros = usize::try_from(-scale).ok()?;
        if digits.len().checked_add(zeros)? > 19 {
            return None;
        }
        digits.extend(std::iter::repeat_n('0', zeros));
    }
    if digits.is_empty() {
        return Some(0);
    }
    if negative {
        digits.insert(0, '-');
    }
    digits.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_exact_decimal_i64;

    #[test]
    fn exact_decimal_integer_lexicals() {
        for (lexical, expected) in [
            ("1.000", 1),
            (" 2.0e2 ", 200),
            ("-9223372036854775808.000", i64::MIN),
            ("9007199254740993.0", 9_007_199_254_740_993),
            ("0e9223372036854775807", 0),
        ] {
            assert_eq!(parse_exact_decimal_i64(lexical), Some(expected));
        }
        for lexical in [
            "1.001",
            "1e-1",
            "9223372036854775808.0",
            "1e9223372036854775807",
            "1.0.0",
        ] {
            assert_eq!(parse_exact_decimal_i64(lexical), None, "{lexical}");
        }
    }
}
