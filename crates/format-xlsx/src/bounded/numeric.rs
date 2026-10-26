/// Returns the exact integer denoted by a decimal/scientific XML token, only
/// inside the same contiguous exact-f64 integer range used by the writer.
/// Never parses through f64: fractional rounding and underflow must not turn
/// non-integral source values into integers. Work and storage are linear and
/// constant respectively, including tokens with many zeroes or exponent digits.
pub(super) fn exact_integer(token: &str) -> Option<i64> {
    let negative = token.starts_with('-');
    let unsigned = token.strip_prefix(['-', '+']).unwrap_or(token);
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let exponent_digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
    if exponent_digits.is_empty() || !exponent_digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut dot = None;
    let mut digits = 0usize;
    let mut nonzero = false;
    for (i, byte) in mantissa.bytes().enumerate() {
        match byte {
            b'.' if dot.is_none() => dot = Some(i),
            b'0'..=b'9' => {
                digits += 1;
                nonzero |= byte != b'0';
            }
            _ => return None,
        }
    }
    if digits == 0 {
        return None;
    }
    if !nonzero {
        return Some(0);
    }
    let fraction = dot.map_or(0, |i| mantissa.len() - i - 1);
    let shift = exponent
        .parse::<i64>()
        .ok()?
        .checked_sub(i64::try_from(fraction).ok()?)?;
    let keep = if shift < 0 {
        let remove = usize::try_from(shift.unsigned_abs()).ok()?;
        let trailing = mantissa
            .bytes()
            .rev()
            .filter(|b| *b != b'.')
            .take_while(|b| *b == b'0')
            .count();
        if remove > trailing {
            return None;
        }
        digits.checked_sub(remove)?
    } else {
        // A nonzero integer with more than 16 appended zeroes cannot fit.
        if shift > 16 {
            return None;
        }
        digits
    };
    let max = crate::MAX_EXACT_F64_INTEGER as u64;
    let mut value = 0u64;
    for byte in mantissa.bytes().filter(|b| *b != b'.').take(keep) {
        value = value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
        if value > max {
            return None;
        }
    }
    for _ in 0..shift.max(0) {
        value = value.checked_mul(10)?;
        if value > max {
            return None;
        }
    }
    Some(if negative {
        -(value as i64)
    } else {
        value as i64
    })
}
