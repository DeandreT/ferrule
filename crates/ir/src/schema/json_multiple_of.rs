use serde::{Deserialize, Serialize};

pub const MAX_JSON_MULTIPLE_OF_ALTERNATIVES: usize = 32;
pub const MAX_JSON_MULTIPLE_OF_TERMS: usize = 64;

/// One exact positive decimal JSON Schema `multipleOf` divisor.
///
/// The represented value is `coefficient * 10^decimal_exponent`.
/// Canonical values have a non-zero coefficient with no trailing decimal
/// zero. The represented decimal must remain finite and non-zero in the JSON
/// number domain supported by ferrule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct JsonMultipleOf {
    coefficient: u64,
    decimal_exponent: i16,
}

impl JsonMultipleOf {
    pub fn new(mut coefficient: u64, mut decimal_exponent: i16) -> Option<Self> {
        if coefficient == 0 {
            return None;
        }
        while coefficient.is_multiple_of(10) {
            coefficient /= 10;
            decimal_exponent = decimal_exponent.checked_add(1)?;
        }
        let divisor = Self {
            coefficient,
            decimal_exponent,
        };
        divisor.is_positive_finite().then_some(divisor)
    }

    pub fn from_decimal_lexical(source: &str) -> Option<Self> {
        let (digits, fractional_digits, explicit_exponent) = parse_decimal(source)?;
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            return None;
        }
        let trailing_zeros = significant
            .len()
            .checked_sub(significant.trim_end_matches('0').len())?;
        let coefficient = significant[..significant.len() - trailing_zeros]
            .parse::<u64>()
            .ok()?;
        let exponent = explicit_exponent
            .checked_sub(i32::try_from(fractional_digits).ok()?)?
            .checked_add(i32::try_from(trailing_zeros).ok()?)?;
        Self::new(coefficient, i16::try_from(exponent).ok()?)
    }

    pub fn coefficient(self) -> u64 {
        self.coefficient
    }

    pub fn decimal_exponent(self) -> i16 {
        self.decimal_exponent
    }

    pub fn to_decimal_lexical(self) -> String {
        decimal_lexical(self.coefficient, self.decimal_exponent)
    }

    pub fn divides_i64(self, value: i64) -> bool {
        if value == 0 {
            return true;
        }
        divides_decimal(
            value.unsigned_abs(),
            0,
            self.coefficient,
            self.decimal_exponent,
        )
    }

    /// Tests a finite float through its canonical Rust decimal display.
    ///
    /// This deliberately does not use an epsilon. Values such as `0.3` and
    /// `0.30000000000000004` remain distinct exact decimal rationals.
    pub fn divides_f64(self, value: f64) -> bool {
        if value == 0.0 {
            return true;
        }
        let Some(value) = decimal_from_f64(value) else {
            return false;
        };
        divides_decimal(
            value.coefficient,
            value.decimal_exponent,
            self.coefficient,
            self.decimal_exponent,
        )
    }

    fn is_positive_finite(self) -> bool {
        self.coefficient > 0
            && !self.coefficient.is_multiple_of(10)
            && format!("{}e{}", self.coefficient, self.decimal_exponent)
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value > 0.0)
    }
}

impl<'de> Deserialize<'de> for JsonMultipleOf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            coefficient: u64,
            decimal_exponent: i16,
        }

        let repr = Repr::deserialize(deserializer)?;
        let Some(value) = Self::new(repr.coefficient, repr.decimal_exponent) else {
            return Err(serde::de::Error::custom(
                "JSON multipleOf divisor must be a positive finite decimal",
            ));
        };
        if value.coefficient != repr.coefficient || value.decimal_exponent != repr.decimal_exponent
        {
            return Err(serde::de::Error::custom(
                "JSON multipleOf divisor must use canonical coefficient and exponent",
            ));
        }
        Ok(value)
    }
}

/// A bounded disjunction of conjunctions over exact decimal divisors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonMultipleOfConstraints {
    any_of: Vec<Vec<JsonMultipleOf>>,
}

impl JsonMultipleOfConstraints {
    pub fn new<I, A>(alternatives: I) -> Result<Self, JsonMultipleOfConstraintsError>
    where
        I: IntoIterator<Item = A>,
        A: IntoIterator<Item = JsonMultipleOf>,
    {
        let mut any_of = Vec::new();
        let mut total_terms = 0_usize;
        for alternative in alternatives {
            let mut terms = Vec::new();
            for divisor in alternative {
                if !terms.contains(&divisor) {
                    if terms.len() == MAX_JSON_MULTIPLE_OF_TERMS {
                        return Err(JsonMultipleOfConstraintsError::TooManyTerms);
                    }
                    terms.push(divisor);
                }
            }
            if terms.is_empty() {
                return Err(JsonMultipleOfConstraintsError::EmptyAlternative);
            }
            if !any_of.contains(&terms) {
                if any_of.len() == MAX_JSON_MULTIPLE_OF_ALTERNATIVES {
                    return Err(JsonMultipleOfConstraintsError::TooManyAlternatives);
                }
                total_terms = total_terms
                    .checked_add(terms.len())
                    .ok_or(JsonMultipleOfConstraintsError::TooManyTerms)?;
                if total_terms > MAX_JSON_MULTIPLE_OF_TERMS {
                    return Err(JsonMultipleOfConstraintsError::TooManyTerms);
                }
                any_of.push(terms);
            }
        }
        Self::from_canonical(any_of)
    }

    pub fn any_of(&self) -> &[Vec<JsonMultipleOf>] {
        &self.any_of
    }

    pub fn into_any_of(self) -> Vec<Vec<JsonMultipleOf>> {
        self.any_of
    }

    pub fn matches_i64(&self, value: i64) -> bool {
        self.any_of
            .iter()
            .any(|alternative| alternative.iter().all(|divisor| divisor.divides_i64(value)))
    }

    pub fn matches_f64(&self, value: f64) -> bool {
        if value == 0.0 {
            return true;
        }
        let Some(value) = decimal_from_f64(value) else {
            return false;
        };
        self.any_of.iter().any(|alternative| {
            alternative.iter().all(|divisor| {
                divides_decimal(
                    value.coefficient,
                    value.decimal_exponent,
                    divisor.coefficient,
                    divisor.decimal_exponent,
                )
            })
        })
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, JsonMultipleOfConstraintsError> {
        let mut any_of = Vec::new();
        let mut total_terms = 0_usize;
        for left in &self.any_of {
            for right in &other.any_of {
                let mut terms = Vec::new();
                for divisor in left.iter().chain(right) {
                    if !terms.contains(divisor) {
                        if terms.len() == MAX_JSON_MULTIPLE_OF_TERMS {
                            return Err(JsonMultipleOfConstraintsError::TooManyTerms);
                        }
                        terms.push(*divisor);
                    }
                }
                if !any_of.contains(&terms) {
                    if any_of.len() == MAX_JSON_MULTIPLE_OF_ALTERNATIVES {
                        return Err(JsonMultipleOfConstraintsError::TooManyAlternatives);
                    }
                    total_terms = total_terms
                        .checked_add(terms.len())
                        .ok_or(JsonMultipleOfConstraintsError::TooManyTerms)?;
                    if total_terms > MAX_JSON_MULTIPLE_OF_TERMS {
                        return Err(JsonMultipleOfConstraintsError::TooManyTerms);
                    }
                    any_of.push(terms);
                }
            }
        }
        Self::from_canonical(any_of)
    }

    pub fn union(&self, other: &Self) -> Result<Self, JsonMultipleOfConstraintsError> {
        let mut any_of = self.any_of.clone();
        let mut total_terms = any_of.iter().map(Vec::len).sum::<usize>();
        for alternative in &other.any_of {
            if !any_of.contains(alternative) {
                if any_of.len() == MAX_JSON_MULTIPLE_OF_ALTERNATIVES {
                    return Err(JsonMultipleOfConstraintsError::TooManyAlternatives);
                }
                total_terms = total_terms
                    .checked_add(alternative.len())
                    .ok_or(JsonMultipleOfConstraintsError::TooManyTerms)?;
                if total_terms > MAX_JSON_MULTIPLE_OF_TERMS {
                    return Err(JsonMultipleOfConstraintsError::TooManyTerms);
                }
                any_of.push(alternative.clone());
            }
        }
        Self::from_canonical(any_of)
    }

    fn from_canonical(
        any_of: Vec<Vec<JsonMultipleOf>>,
    ) -> Result<Self, JsonMultipleOfConstraintsError> {
        validate_canonical(&any_of)?;
        Ok(Self { any_of })
    }
}

impl<'de> Deserialize<'de> for JsonMultipleOfConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            any_of: Vec<Vec<JsonMultipleOf>>,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::from_canonical(repr.any_of).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonMultipleOfConstraintsError {
    Empty,
    EmptyAlternative,
    DuplicateTerm,
    DuplicateAlternative,
    TooManyAlternatives,
    TooManyTerms,
}

impl core::fmt::Display for JsonMultipleOfConstraintsError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "JSON multipleOf constraints are empty"),
            Self::EmptyAlternative => {
                write!(formatter, "a JSON multipleOf alternative has no terms")
            }
            Self::DuplicateTerm => write!(
                formatter,
                "a JSON multipleOf alternative contains a duplicate divisor"
            ),
            Self::DuplicateAlternative => write!(
                formatter,
                "JSON multipleOf constraints contain a duplicate alternative"
            ),
            Self::TooManyAlternatives => write!(
                formatter,
                "JSON multipleOf constraints exceed the {MAX_JSON_MULTIPLE_OF_ALTERNATIVES}-alternative limit"
            ),
            Self::TooManyTerms => write!(
                formatter,
                "JSON multipleOf constraints exceed the {MAX_JSON_MULTIPLE_OF_TERMS}-term limit"
            ),
        }
    }
}

impl std::error::Error for JsonMultipleOfConstraintsError {}

fn validate_canonical(
    any_of: &[Vec<JsonMultipleOf>],
) -> Result<(), JsonMultipleOfConstraintsError> {
    if any_of.is_empty() {
        return Err(JsonMultipleOfConstraintsError::Empty);
    }
    if any_of.len() > MAX_JSON_MULTIPLE_OF_ALTERNATIVES {
        return Err(JsonMultipleOfConstraintsError::TooManyAlternatives);
    }
    let mut terms = 0_usize;
    for (alternative_index, alternative) in any_of.iter().enumerate() {
        if alternative.is_empty() {
            return Err(JsonMultipleOfConstraintsError::EmptyAlternative);
        }
        if any_of[..alternative_index].contains(alternative) {
            return Err(JsonMultipleOfConstraintsError::DuplicateAlternative);
        }
        for (term_index, divisor) in alternative.iter().enumerate() {
            if alternative[..term_index].contains(divisor) {
                return Err(JsonMultipleOfConstraintsError::DuplicateTerm);
            }
            terms = terms
                .checked_add(1)
                .ok_or(JsonMultipleOfConstraintsError::TooManyTerms)?;
            if terms > MAX_JSON_MULTIPLE_OF_TERMS {
                return Err(JsonMultipleOfConstraintsError::TooManyTerms);
            }
        }
    }
    Ok(())
}

fn parse_decimal(source: &str) -> Option<(String, usize, i32)> {
    if source.is_empty() || source.starts_with(['+', '-']) {
        return None;
    }
    let (mantissa, exponent) = source.find(['e', 'E']).map_or((source, "0"), |offset| {
        (&source[..offset], &source[offset + 1..])
    });
    if mantissa.is_empty() || exponent.is_empty() {
        return None;
    }
    let explicit_exponent = parse_exponent(exponent)?;
    let mut pieces = mantissa.split('.');
    let integer = pieces.next()?;
    let fraction = pieces.next();
    if pieces.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|fraction| {
            fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let fraction = fraction.map_or("", |fraction| fraction);
    let mut digits = String::with_capacity(integer.len().checked_add(fraction.len())?);
    digits.push_str(integer);
    digits.push_str(fraction);
    Some((digits, fraction.len(), explicit_exponent))
}

fn decimal_from_f64(value: f64) -> Option<JsonMultipleOf> {
    value
        .is_finite()
        .then(|| value.abs().to_string())
        .and_then(|canonical| JsonMultipleOf::from_decimal_lexical(&canonical))
}

fn parse_exponent(source: &str) -> Option<i32> {
    let (negative, digits) = match source.as_bytes().first() {
        Some(b'+') => (false, &source[1..]),
        Some(b'-') => (true, &source[1..]),
        _ => (false, source),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.parse::<i32>().ok()?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

fn decimal_lexical(coefficient: u64, decimal_exponent: i16) -> String {
    let digits = coefficient.to_string();
    if decimal_exponent >= 0 {
        let mut result = digits;
        result.extend(core::iter::repeat_n(
            '0',
            decimal_exponent.unsigned_abs().into(),
        ));
        return result;
    }
    let fractional = usize::from(decimal_exponent.unsigned_abs());
    if fractional < digits.len() {
        let split = digits.len() - fractional;
        return format!("{}.{}", &digits[..split], &digits[split..]);
    }
    let zeros = fractional - digits.len();
    format!("0.{}{}", "0".repeat(zeros), digits)
}

fn divides_decimal(
    value_coefficient: u64,
    value_exponent: i16,
    divisor_coefficient: u64,
    divisor_exponent: i16,
) -> bool {
    if value_coefficient == 0 {
        return true;
    }
    let shift = i32::from(value_exponent) - i32::from(divisor_exponent);
    if shift >= 0 {
        let common = gcd(value_coefficient, divisor_coefficient);
        let mut remainder = divisor_coefficient / common;
        let mut twos = 0_i32;
        while remainder.is_multiple_of(2) {
            remainder /= 2;
            twos += 1;
        }
        let mut fives = 0_i32;
        while remainder.is_multiple_of(5) {
            remainder /= 5;
            fives += 1;
        }
        return remainder == 1 && twos <= shift && fives <= shift;
    }

    if !value_coefficient.is_multiple_of(divisor_coefficient) {
        return false;
    }
    let mut quotient = value_coefficient / divisor_coefficient;
    for _ in 0..shift.unsigned_abs() {
        if !quotient.is_multiple_of(10) {
            return false;
        }
        quotient /= 10;
    }
    true
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divisor(source: &str) -> JsonMultipleOf {
        let Some(divisor) = JsonMultipleOf::from_decimal_lexical(source) else {
            panic!("test divisor `{source}` is representable");
        };
        divisor
    }

    #[test]
    fn decimal_divisors_are_canonical_and_bounded() {
        assert_eq!(
            divisor("001.2300e2"),
            JsonMultipleOf {
                coefficient: 123,
                decimal_exponent: 0,
            }
        );
        assert_eq!(divisor("0.001").to_decimal_lexical(), "0.001");
        assert_eq!(
            divisor("1e20").to_decimal_lexical(),
            "100000000000000000000"
        );
        assert!(JsonMultipleOf::from_decimal_lexical("0").is_none());
        assert!(JsonMultipleOf::from_decimal_lexical("-1").is_none());
        assert!(JsonMultipleOf::from_decimal_lexical("1e9999").is_none());
        assert!(JsonMultipleOf::from_decimal_lexical("1e-9999").is_none());
        assert!(JsonMultipleOf::from_decimal_lexical("0x1").is_none());

        assert_eq!(divisor("18446744073709551615").coefficient(), u64::MAX);
        assert_eq!(
            divisor("184467440737095516150"),
            JsonMultipleOf {
                coefficient: u64::MAX,
                decimal_exponent: 1,
            }
        );
        assert!(JsonMultipleOf::from_decimal_lexical("184467440737095516151").is_none());
        assert!(JsonMultipleOf::new(1, i16::MIN).is_none());
        assert!(JsonMultipleOf::new(1, i16::MAX).is_none());
        assert!(JsonMultipleOf::new(u64::MAX, 288).is_some());
        assert!(JsonMultipleOf::new(u64::MAX, 289).is_none());
    }

    #[test]
    fn divisibility_uses_exact_decimal_arithmetic() {
        let tenth = divisor("0.1");
        assert!(tenth.divides_i64(i64::MIN));
        assert!(tenth.divides_i64(0));
        assert!(tenth.divides_i64(7));
        assert!(tenth.divides_f64(0.3));
        assert!(!tenth.divides_f64(0.30000000000000004));

        let two_and_half = divisor("2.5");
        assert!(two_and_half.divides_i64(-5));
        assert!(!two_and_half.divides_i64(1));
        assert!(two_and_half.divides_f64(7.5));
        assert!(!two_and_half.divides_f64(7.500000000000001));

        let tiny = divisor("0.00000000000000001");
        assert!(tiny.divides_f64(0.00000000000000001));
        assert!(!tiny.divides_f64(f64::MIN_POSITIVE));
        assert!(!tiny.divides_f64(f64::from_bits(1)));
        let smallest_subnormal = divisor("5e-324");
        assert!(smallest_subnormal.divides_f64(f64::from_bits(1)));
        assert!(!divisor("1").divides_f64(0.01));
        assert!(divisor("0.01").divides_i64(100));

        let above_signed_domain = divisor("9223372036854775809");
        assert!(above_signed_domain.divides_i64(0));
        assert!(!above_signed_domain.divides_i64(i64::MIN));
        assert!(!above_signed_domain.divides_i64(i64::MAX));
        assert!(!tiny.divides_f64(f64::NAN));
        assert!(!tiny.divides_f64(f64::INFINITY));

        let signed_minimum = divisor("9223372036854775808");
        assert!(signed_minimum.divides_i64(i64::MIN));
        assert!(!signed_minimum.divides_i64(i64::MAX));

        assert!(divisor("1e308").divides_f64(1e308));
        assert!(!divisor("1e308").divides_f64(1e307));
        assert!(divisor("1e-308").divides_f64(1e-307));
        assert!(!divisor("2e-308").divides_f64(1e-308));
        assert!(divisor("3e-324").divides_f64(0.0));
        assert!(!divisor("3e-324").divides_f64(f64::from_bits(1)));
    }

    #[test]
    fn decimal_divisibility_matches_small_exact_rationals() {
        for value_coefficient in 1_u64..=40 {
            for divisor_coefficient in 1_u64..=40 {
                for value_exponent in -3_i16..=3 {
                    for divisor_exponent in -3_i16..=3 {
                        let common_exponent = value_exponent.min(divisor_exponent);
                        let value_scale =
                            u32::from((value_exponent - common_exponent).unsigned_abs());
                        let divisor_scale =
                            u32::from((divisor_exponent - common_exponent).unsigned_abs());
                        let numerator = u128::from(value_coefficient) * 10_u128.pow(value_scale);
                        let denominator =
                            u128::from(divisor_coefficient) * 10_u128.pow(divisor_scale);
                        assert_eq!(
                            divides_decimal(
                                value_coefficient,
                                value_exponent,
                                divisor_coefficient,
                                divisor_exponent,
                            ),
                            numerator.is_multiple_of(denominator),
                            "{value_coefficient}e{value_exponent} / \
                             {divisor_coefficient}e{divisor_exponent}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn constraints_compose_as_bounded_canonical_dnf() -> Result<(), Box<dyn std::error::Error>> {
        let two = JsonMultipleOfConstraints::new([[divisor("2")]])?;
        let three = JsonMultipleOfConstraints::new([[divisor("3")]])?;
        let either = two.union(&three)?;
        assert!(either.matches_i64(4));
        assert!(either.matches_i64(9));
        assert!(!either.matches_i64(5));

        let both = two.intersection(&three)?;
        assert!(both.matches_i64(12));
        assert!(!both.matches_i64(9));
        assert_eq!(
            serde_json::to_string(&both)?,
            r#"{"any_of":[[{"coefficient":2,"decimal_exponent":0},{"coefficient":3,"decimal_exponent":0}]]}"#
        );
        assert_eq!(
            serde_json::from_str::<JsonMultipleOfConstraints>(&serde_json::to_string(&both)?)?,
            both
        );
        Ok(())
    }

    #[test]
    fn serialized_constraints_reject_noncanonical_or_unbounded_values() {
        for invalid in [
            r#"{"any_of":[]}"#,
            r#"{"any_of":[[]]}"#,
            r#"{"any_of":[[{"coefficient":0,"decimal_exponent":0}]]}"#,
            r#"{"any_of":[[{"coefficient":20,"decimal_exponent":0}]]}"#,
            r#"{"any_of":[[{"coefficient":2,"decimal_exponent":0},{"coefficient":2,"decimal_exponent":0}]]}"#,
            r#"{"any_of":[[{"coefficient":2,"decimal_exponent":0}],[{"coefficient":2,"decimal_exponent":0}]]}"#,
        ] {
            assert!(
                serde_json::from_str::<JsonMultipleOfConstraints>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn construction_and_serde_enforce_dnf_budgets() {
        let divisors = (1_u64..=65)
            .filter_map(|coefficient| JsonMultipleOf::new(coefficient, 0))
            .collect::<Vec<_>>();
        assert_eq!(
            JsonMultipleOfConstraints::new([divisors.clone()]),
            Err(JsonMultipleOfConstraintsError::TooManyTerms)
        );

        let alternatives = divisors
            .iter()
            .take(33)
            .copied()
            .map(|divisor| [divisor])
            .collect::<Vec<_>>();
        assert_eq!(
            JsonMultipleOfConstraints::new(alternatives),
            Err(JsonMultipleOfConstraintsError::TooManyAlternatives)
        );

        let serialized = serde_json::json!({ "any_of": [divisors.clone()] });
        assert!(serde_json::from_value::<JsonMultipleOfConstraints>(serialized).is_err());

        let left = JsonMultipleOfConstraints::new([divisors[..32].to_vec()])
            .unwrap_or_else(|error| panic!("left constraints are bounded: {error}"));
        let right = JsonMultipleOfConstraints::new([divisors[32..].to_vec()])
            .unwrap_or_else(|error| panic!("right constraints are bounded: {error}"));
        assert_eq!(
            left.union(&right),
            Err(JsonMultipleOfConstraintsError::TooManyTerms)
        );
        assert_eq!(
            left.intersection(&right),
            Err(JsonMultipleOfConstraintsError::TooManyTerms)
        );
    }
}
