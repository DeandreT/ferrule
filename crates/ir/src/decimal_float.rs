// Binary64 conversion follows System.Decimal.DecCalc.VarDecFromR8 from
// dotnet/runtime v6.0.36, adapted to the canonical Decimal96 representation.
// Copyright (c) .NET Foundation and Contributors. MIT license; see
// ../THIRD-PARTY-NOTICES.md.

use super::{Decimal96, DecimalError, MAX_DECIMAL_SCALE};

// Literal rounding is deliberate: repeated multiplication or powi need not
// produce the same binary64 values used by the reference conversion.
const POWERS_OF_TEN: [f64; 29] = [
    1.0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28,
];

impl Decimal96 {
    /// Explicit .NET-compatible binary64 conversion, rounded toward fifteen
    /// significant decimal digits with nearest/ties-to-even rounding. Very small
    /// values become canonical zero; nonfinite or out-of-range values overflow.
    /// This does not parse exponent text or change ordinary Int/Float behavior.
    pub fn from_f64(value: f64) -> Result<Self, DecimalError> {
        let exponent = ((value.to_bits() >> 52) & 0x7ff) as i32 - 1022;
        if exponent < -94 {
            return Ok(Self::ZERO);
        }
        if exponent > 96 {
            return Err(DecimalError::Overflow);
        }

        let negative = value < 0.0;
        let mut scaled = value.abs();
        // floor(exponent * log10(2)), using the reference's fixed-point bound.
        let mut scale = 14 - ((exponent * 19728) >> 16);
        if scale >= 0 {
            scale = scale.min(i32::from(MAX_DECIMAL_SCALE));
            scaled *= POWERS_OF_TEN[scale as usize];
        } else if scale != -1 || scaled >= 1e15 {
            scaled /= POWERS_OF_TEN[(-scale) as usize];
        } else {
            scale = 0;
        }
        if scaled < 1e14 && scale < i32::from(MAX_DECIMAL_SCALE) {
            scaled *= 10.0;
            scale += 1;
        }
        let coefficient = scaled.round_ties_even() as u128;
        if scale < 0 {
            let coefficient = coefficient
                .checked_mul(10_u128.pow(scale.unsigned_abs()))
                .ok_or(DecimalError::Overflow)?;
            Self::from_parts(negative, coefficient, 0)
        } else {
            Self::from_parts(negative, coefficient, scale as u8)
        }
    }
}
