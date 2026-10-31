//! `round_half_ceiling`: the reference application's `round` and
//! `round-precision`, which round exact halves toward positive infinity on
//! decimal values.
//!
//! Rounding works on decimal digits, never on a scaled binary float. A Float
//! operand contributes its shortest round-trip text, so `1.005` rounds as the
//! decimal `1.005`; text operands contribute their own digits. Integers stay
//! integers; other results are the nearest binary float to the rounded decimal.

use ir::Value;

use crate::FunctionError;

const FUNCTION: &str = "round_half_ceiling";
const NOT_FINITE: &str = "requires a finite numeric value";
/// Exponents beyond this already put any nonzero digit far outside the finite
/// binary64 range, so clamping them cannot change a result.
const EXPONENT_LIMIT: i64 = 1_000_000_000_000;
/// Decimal magnitudes beyond binary64 overflow to infinity or underflow to zero.
const FLOAT_MAGNITUDE_LIMIT: i64 = 400;

pub(super) fn round_half_ceiling(args: &[Value]) -> Result<Value, FunctionError> {
    let (value, precision) = match args {
        [value] => (value, 0),
        [value, precision] => (value, precision_arg(precision)?),
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: FUNCTION,
                expected: 1,
                got: args.len(),
            });
        }
    };
    match operand(value)? {
        Operand::Integer(integer) => round_integer(integer, precision).map(Value::Int),
        Operand::NonFinite(value) => Ok(Value::Float(value)),
        Operand::Decimal(decimal) => Ok(Value::Float(decimal.round(precision).to_f64())),
    }
}

enum Operand {
    Integer(i64),
    NonFinite(f64),
    Decimal(DecimalDigits),
}

fn operand(value: &Value) -> Result<Operand, FunctionError> {
    match value {
        Value::Int(integer) => Ok(Operand::Integer(*integer)),
        Value::Float(value) if !value.is_finite() => Ok(Operand::NonFinite(*value)),
        Value::Float(value) => DecimalDigits::parse(&format!("{value:e}"))
            .map(Operand::Decimal)
            .ok_or_else(invalid),
        Value::String(text) => {
            let text = text.trim();
            if let Ok(integer) = text.parse::<i64>() {
                return Ok(Operand::Integer(integer));
            }
            // The binary64 grammar decides which text is numeric, as it does
            // for `to_number`; the decimal digits then come from the text.
            if !text.parse::<f64>().is_ok_and(f64::is_finite) {
                return Err(invalid());
            }
            DecimalDigits::parse(text)
                .map(Operand::Decimal)
                .ok_or_else(invalid)
        }
        other => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: other.type_name(),
        }),
    }
}

/// Decimal places, read as the native `round` reads them: integers directly,
/// floats rounded half away from zero with NaN as zero, both saturating.
fn precision_arg(value: &Value) -> Result<i32, FunctionError> {
    let saturate = |integer: i64| integer.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    match value {
        Value::Int(integer) => Ok(saturate(*integer)),
        Value::Float(value) => Ok(value.round() as i32),
        Value::String(text) => {
            let text = text.trim();
            if let Ok(integer) = text.parse::<i64>() {
                return Ok(saturate(integer));
            }
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(value.round() as i32),
                _ => Err(invalid()),
            }
        }
        other => Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: other.type_name(),
        }),
    }
}

fn invalid() -> FunctionError {
    FunctionError::InvalidArgument {
        function: FUNCTION,
        message: NOT_FINITE,
    }
}

fn round_integer(integer: i64, precision: i32) -> Result<i64, FunctionError> {
    if precision >= 0 {
        return Ok(integer);
    }
    let decimal = DecimalDigits::parse(&integer.to_string()).ok_or_else(invalid)?;
    let rounded = decimal.round(precision);
    let overflow = || FunctionError::IntegerOverflow { function: FUNCTION };
    if rounded.digits.is_empty() {
        return Ok(0);
    }
    // A nonzero result keeps `precision`'s exponent, so it has at least
    // `-precision` trailing zeros; more than 19 cannot fit an i64.
    let zeros = u32::try_from(rounded.exponent).map_err(|_| overflow())?;
    let digits = std::str::from_utf8(&rounded.digits).map_err(|_| invalid())?;
    let magnitude = digits.parse::<i128>().map_err(|_| overflow())?;
    let scaled = 10_i128
        .checked_pow(zeros)
        .and_then(|power| magnitude.checked_mul(power))
        .ok_or_else(overflow)?;
    let signed = if rounded.negative { -scaled } else { scaled };
    i64::try_from(signed).map_err(|_| overflow())
}

/// `digits * 10^exponent` with ASCII digits and no leading or trailing zeros;
/// zero has no digits and no sign.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DecimalDigits {
    negative: bool,
    digits: Vec<u8>,
    exponent: i64,
}

impl DecimalDigits {
    /// Parses `[+-]?(d+(.d*)?|.d+)([eE][+-]?d+)?`.
    fn parse(text: &str) -> Option<Self> {
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, parse_exponent(exponent)?),
            None => (text, 0),
        };
        let negative = mantissa.starts_with('-');
        let mantissa = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if (whole.is_empty() && fraction.is_empty())
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let fraction_digits = i64::try_from(fraction.len()).ok()?;
        Some(
            Self {
                negative,
                digits: [whole.as_bytes(), fraction.as_bytes()].concat(),
                exponent: exponent - fraction_digits,
            }
            .normalized(),
        )
    }

    /// Rounds to `precision` decimal places, exact halves toward positive infinity.
    fn round(self, precision: i32) -> Self {
        let last = -i64::from(precision);
        if self.digits.is_empty() || self.exponent >= last {
            return self;
        }
        let dropped = last - self.exponent;
        let length = self.digits.len() as i64;
        let kept = usize::try_from(length - dropped).unwrap_or(0);
        // When every digit lies below the first dropped place, that place is
        // an implicit zero and the value rounds toward zero.
        let (first, rest) = if dropped > length {
            (0, false)
        } else {
            (
                self.digits[kept] - b'0',
                self.digits[kept + 1..].iter().any(|digit| *digit != b'0'),
            )
        };
        let up = first > 5 || first == 5 && (rest || !self.negative);
        let mut digits = self.digits[..kept].to_vec();
        if up {
            increment(&mut digits);
        }
        Self {
            negative: self.negative,
            digits,
            exponent: last,
        }
        .normalized()
    }

    fn to_f64(&self) -> f64 {
        if self.digits.is_empty() {
            return 0.0;
        }
        let magnitude = self.exponent.saturating_add(self.digits.len() as i64);
        let value = if magnitude > FLOAT_MAGNITUDE_LIMIT {
            f64::INFINITY
        } else if magnitude < -FLOAT_MAGNITUDE_LIMIT {
            0.0
        } else {
            let digits = String::from_utf8_lossy(&self.digits);
            format!("{digits}e{}", self.exponent)
                .parse::<f64>()
                .unwrap_or(f64::NAN)
        };
        if self.negative { -value } else { value }
    }

    fn normalized(mut self) -> Self {
        let leading = self
            .digits
            .iter()
            .take_while(|digit| **digit == b'0')
            .count();
        self.digits.drain(..leading);
        let trailing = self
            .digits
            .iter()
            .rev()
            .take_while(|digit| **digit == b'0')
            .count();
        self.digits.truncate(self.digits.len() - trailing);
        self.exponent = self.exponent.saturating_add(trailing as i64);
        if self.digits.is_empty() {
            self.negative = false;
            self.exponent = 0;
        }
        self
    }
}

fn parse_exponent(text: &str) -> Option<i64> {
    let negative = text.starts_with('-');
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.bytes().fold(0_i64, |value, digit| {
        (value * 10 + i64::from(digit - b'0')).min(EXPONENT_LIMIT)
    });
    Some(if negative { -magnitude } else { magnitude })
}

/// Adds one unit in the last place of a nonnegative digit string.
fn increment(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return;
        }
    }
    digits.insert(0, b'1');
}

#[cfg(test)]
mod tests {
    use super::DecimalDigits;

    #[test]
    fn decimal_digits_parse_the_float_grammar_exactly() {
        for (text, negative, digits, exponent) in [
            ("1.005", false, "1005", -3),
            ("-0.125", true, "125", -3),
            ("1.005e0", false, "1005", -3),
            ("1E+300", false, "1", 300),
            ("-1.5E-07", true, "15", -8),
            ("+.50", false, "5", -1),
            ("12.", false, "12", 0),
            ("1200", false, "12", 2),
            ("-0.0", false, "", 0),
        ] {
            assert_eq!(
                DecimalDigits::parse(text),
                Some(DecimalDigits {
                    negative,
                    digits: digits.as_bytes().to_vec(),
                    exponent,
                }),
                "{text}"
            );
        }
        for text in ["", ".", "-", "1e", "1e+", "1.2.3", "0x10", "1_000", "inf"] {
            assert_eq!(DecimalDigits::parse(text), None, "{text}");
        }
    }
}
