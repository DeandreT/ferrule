//! Explicit bounded decimal arithmetic backing the boxed decimal scalar.

use std::{cmp::Ordering, error::Error, fmt};

#[path = "decimal_float.rs"]
mod float;
#[path = "decimal_number.rs"]
mod number;

pub const MAX_DECIMAL_LEXICAL_BYTES: usize = 4096;
pub const MAX_DECIMAL_SCALE: u8 = 28;
pub const MAX_DECIMAL_COEFFICIENT: u128 = (1_u128 << 96) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalError {
    InvalidLexical,
    InputTooLong,
    InvalidScale,
    Overflow,
    PrecisionLoss,
    InvalidNumberLexical,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLexical => "expected a plain ASCII decimal without whitespace or exponent",
            Self::InputTooLong => "decimal lexical exceeds the byte limit",
            Self::InvalidScale => "decimal scale must not exceed 28",
            Self::Overflow => "decimal coefficient exceeds the 96-bit range",
            Self::PrecisionLoss => "decimal cannot be represented without rounding",
            Self::InvalidNumberLexical => {
                "expected an ASCII decimal number with an optional exponent"
            }
        })
    }
}
impl Error for DecimalError {}

/// A canonical signed coefficient times `10^-scale`. Coefficients are at most
/// 96 bits; scale is at most 28. Zero has no negative sign or scale, and nonzero
/// coefficients have no removable fractional trailing zeroes.
///
/// There are no implicit Int/Float conversions. The boxed scalar has a separate
/// canonical persistence tag; parsing and rounded arithmetic are explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal96 {
    negative: bool,
    coefficient: u128,
    scale: u8,
}

impl Decimal96 {
    pub const ZERO: Self = Self {
        negative: false,
        coefficient: 0,
        scale: 0,
    };

    pub const fn from_parts(
        negative: bool,
        coefficient: u128,
        scale: u8,
    ) -> Result<Self, DecimalError> {
        if scale > MAX_DECIMAL_SCALE {
            return Err(DecimalError::InvalidScale);
        }
        if coefficient > MAX_DECIMAL_COEFFICIENT {
            return Err(DecimalError::Overflow);
        }
        if coefficient == 0 {
            return Ok(Self::ZERO);
        }
        let mut result = Self {
            negative,
            coefficient,
            scale,
        };
        while result.scale > 0 && result.coefficient.is_multiple_of(10) {
            result.coefficient /= 10;
            result.scale -= 1;
        }
        Ok(result)
    }

    pub const fn coefficient(self) -> u128 {
        self.coefficient
    }
    pub const fn scale(self) -> u8 {
        self.scale
    }
    pub const fn is_negative(self) -> bool {
        self.negative
    }

    /// Parse `[+-]?(digits[.digits?]?|.digits)` exactly, without trimming.
    pub fn parse_exact(text: &str) -> Result<Self, DecimalError> {
        Self::parse(text, false)
    }

    /// Parse plain decimal text and round once to the most precise representable
    /// scale, nearest with ties to even. Exponents and whitespace are not accepted.
    pub fn parse_rounded(text: &str) -> Result<Self, DecimalError> {
        Self::parse(text, true)
    }

    fn parse(text: &str, rounding: bool) -> Result<Self, DecimalError> {
        if text.len() > MAX_DECIMAL_LEXICAL_BYTES {
            return Err(DecimalError::InputTooLong);
        }
        Self::parse_digits(text, rounding)
    }

    fn parse_digits(text: &str, rounding: bool) -> Result<Self, DecimalError> {
        let negative = text.starts_with('-');
        let text = text.strip_prefix(['-', '+']).unwrap_or(text);
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if (whole.is_empty() && fraction.is_empty())
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return Err(DecimalError::InvalidLexical);
        }
        let digits = [whole, fraction].concat();
        let mut scale = fraction.len().min(usize::from(MAX_DECIMAL_SCALE));
        loop {
            let keep = digits.len() - (fraction.len() - scale);
            let (kept, discarded) = digits.as_bytes().split_at(keep);
            let coefficient = kept.iter().try_fold(0_u128, |value, digit| {
                value
                    .checked_mul(10)?
                    .checked_add(u128::from(digit - b'0'))
                    .filter(|value| *value <= MAX_DECIMAL_COEFFICIENT)
            });
            if let Some(coefficient) = coefficient {
                let round_up = discarded.first().is_some_and(|digit| {
                    *digit > b'5'
                        || (*digit == b'5'
                            && (coefficient % 2 == 1 || discarded[1..].iter().any(|d| *d != b'0')))
                });
                let rounded = coefficient + u128::from(round_up);
                if rounded <= MAX_DECIMAL_COEFFICIENT {
                    if !rounding && discarded.iter().any(|digit| *digit != b'0') {
                        return Err(DecimalError::PrecisionLoss);
                    }
                    return Self::from_parts(negative, rounded, scale as u8);
                }
            }
            if scale == 0 {
                return Err(DecimalError::Overflow);
            }
            scale -= 1;
        }
    }

    pub const fn negated(self) -> Self {
        Self {
            negative: self.coefficient != 0 && !self.negative,
            ..self
        }
    }

    /// Add exact represented operands, rounding the sum only when its coefficient
    /// cannot fit. The same nearest/ties-to-even rule is used, without a Float.
    pub fn add_rounded(self, other: Self) -> Result<Self, DecimalError> {
        let scale = self.scale.max(other.scale);
        let left = self.aligned(scale);
        let right = other.aligned(scale);
        let (magnitude, negative) = if self.negative == other.negative {
            (left.add(right), self.negative)
        } else if left >= right {
            (left.subtract(right), self.negative)
        } else {
            (right.subtract(left), other.negative)
        };
        Self::round_wide(magnitude, negative, scale)
    }

    pub fn subtract_rounded(self, other: Self) -> Result<Self, DecimalError> {
        self.add_rounded(other.negated())
    }

    fn aligned(self, scale: u8) -> Wide {
        let mut value = Wide::from_coefficient(self.coefficient);
        for _ in self.scale..scale {
            value = value.times_ten();
        }
        value
    }

    fn round_wide(mut value: Wide, negative: bool, mut scale: u8) -> Result<Self, DecimalError> {
        if let Some(coefficient) = value.coefficient() {
            return Self::from_parts(negative, coefficient, scale);
        }
        let mut sticky = false;
        while scale > 0 {
            let (quotient, digit) = value.divide_ten();
            value = quotient;
            scale -= 1;
            let round_up = digit > 5 || (digit == 5 && (sticky || value.is_odd()));
            let rounded = if round_up {
                value.add(Wide::from_coefficient(1))
            } else {
                value
            };
            if let Some(coefficient) = rounded.coefficient() {
                return Self::from_parts(negative, coefficient, scale);
            }
            sticky |= digit != 0;
        }
        Err(DecimalError::Overflow)
    }
}

impl fmt::Display for Decimal96 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            formatter.write_str("-")?;
        }
        let digits = self.coefficient.to_string();
        let scale = usize::from(self.scale);
        if scale == 0 {
            return formatter.write_str(&digits);
        }
        if digits.len() <= scale {
            formatter.write_str("0.")?;
            for _ in digits.len()..scale {
                formatter.write_str("0")?;
            }
            formatter.write_str(&digits)
        } else {
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            write!(formatter, "{whole}.{fraction}")
        }
    }
}

impl Ord for Decimal96 {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.negative != other.negative {
            return other.negative.cmp(&self.negative);
        }
        let scale = self.scale.max(other.scale);
        let result = self.aligned(scale).cmp(&other.aligned(scale));
        if self.negative {
            result.reverse()
        } else {
            result
        }
    }
}
impl PartialOrd for Decimal96 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// A 96-bit coefficient aligned by at most 28 powers of ten, plus a second such
// value, fits in 192 bits. All constructors are private and preserve this bound.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Wide([u64; 3]);

impl Wide {
    fn from_coefficient(value: u128) -> Self {
        Self([value as u64, (value >> 64) as u64, 0])
    }
    fn coefficient(self) -> Option<u128> {
        (self.0[2] == 0 && self.0[1] <= u64::from(u32::MAX))
            .then_some(u128::from(self.0[0]) | (u128::from(self.0[1]) << 64))
    }
    fn is_odd(self) -> bool {
        self.0[0] % 2 == 1
    }
    fn times_ten(self) -> Self {
        let mut result = [0; 3];
        let mut carry = 0_u128;
        for (out, limb) in result.iter_mut().zip(self.0) {
            let product = u128::from(limb) * 10 + carry;
            *out = product as u64;
            carry = product >> 64;
        }
        debug_assert_eq!(carry, 0);
        Self(result)
    }
    fn add(self, other: Self) -> Self {
        let mut result = [0; 3];
        let mut carry = 0_u128;
        for (index, out) in result.iter_mut().enumerate() {
            let sum = u128::from(self.0[index]) + u128::from(other.0[index]) + carry;
            *out = sum as u64;
            carry = sum >> 64;
        }
        debug_assert_eq!(carry, 0);
        Self(result)
    }
    fn subtract(self, other: Self) -> Self {
        debug_assert!(self >= other);
        let mut result = [0; 3];
        let mut borrow = false;
        for (index, out) in result.iter_mut().enumerate() {
            let (value, first) = self.0[index].overflowing_sub(other.0[index]);
            let (value, second) = value.overflowing_sub(u64::from(borrow));
            *out = value;
            borrow = first || second;
        }
        debug_assert!(!borrow);
        Self(result)
    }
    fn divide_ten(self) -> (Self, u8) {
        let mut result = [0; 3];
        let mut remainder = 0_u128;
        for index in (0..3).rev() {
            let value = (remainder << 64) | u128::from(self.0[index]);
            result[index] = (value / 10) as u64;
            remainder = value % 10;
        }
        (Self(result), remainder as u8)
    }
}
impl Ord for Wide {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }
}
impl PartialOrd for Wide {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
