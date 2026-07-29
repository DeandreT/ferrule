use serde::{Deserialize, Serialize};

use crate::FiniteF64;

/// One normalized, non-empty inclusive interval in Ferrule's signed-integer
/// runtime domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IntegerRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minimum: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum: Option<i64>,
}

impl IntegerRange {
    pub fn new(minimum: Option<i64>, maximum: Option<i64>) -> Option<Self> {
        (minimum.is_some() || maximum.is_some())
            .then_some(())
            .and_then(|()| {
                minimum
                    .zip(maximum)
                    .is_none_or(|(minimum, maximum)| minimum <= maximum)
                    .then_some(Self { minimum, maximum })
            })
    }

    pub fn minimum(self) -> Option<i64> {
        self.minimum
    }

    pub fn maximum(self) -> Option<i64> {
        self.maximum
    }

    pub fn contains(self, value: i64) -> bool {
        self.minimum.is_none_or(|minimum| value >= minimum)
            && self.maximum.is_none_or(|maximum| value <= maximum)
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let minimum = match (self.minimum, other.minimum) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        let maximum = match (self.maximum, other.maximum) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        Self::new(minimum, maximum)
    }
}

impl<'de> Deserialize<'de> for IntegerRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            #[serde(default)]
            minimum: Option<i64>,
            #[serde(default)]
            maximum: Option<i64>,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.minimum, repr.maximum)
            .ok_or_else(|| serde::de::Error::custom("integer range must be non-empty and ordered"))
    }
}

/// One finite endpoint of a number interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberBound {
    value: FiniteF64,
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    exclusive: bool,
}

impl NumberBound {
    pub fn inclusive(value: FiniteF64) -> Self {
        Self {
            value,
            exclusive: false,
        }
    }

    pub fn exclusive(value: FiniteF64) -> Self {
        Self {
            value,
            exclusive: true,
        }
    }

    pub fn value(self) -> FiniteF64 {
        self.value
    }

    pub fn is_exclusive(self) -> bool {
        self.exclusive
    }
}

/// One normalized, non-empty interval in Ferrule's finite-number runtime
/// domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NumberRange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minimum: Option<NumberBound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum: Option<NumberBound>,
}

impl NumberRange {
    pub fn new(minimum: Option<NumberBound>, maximum: Option<NumberBound>) -> Option<Self> {
        if minimum.is_none() && maximum.is_none() {
            return None;
        }
        let first = match minimum {
            Some(bound) if bound.exclusive => next_finite(bound.value.get())?,
            Some(bound) => bound.value.get(),
            None => -f64::MAX,
        };
        let last = match maximum {
            Some(bound) if bound.exclusive => previous_finite(bound.value.get())?,
            Some(bound) => bound.value.get(),
            None => f64::MAX,
        };
        if first > last {
            return None;
        }
        Some(Self { minimum, maximum })
    }

    pub fn minimum(self) -> Option<NumberBound> {
        self.minimum
    }

    pub fn maximum(self) -> Option<NumberBound> {
        self.maximum
    }

    pub fn contains(self, value: f64) -> bool {
        value.is_finite()
            && self.minimum.is_none_or(|minimum| {
                value > minimum.value.get() || (!minimum.exclusive && value == minimum.value.get())
            })
            && self.maximum.is_none_or(|maximum| {
                value < maximum.value.get() || (!maximum.exclusive && value == maximum.value.get())
            })
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let minimum = stricter_minimum(self.minimum, other.minimum);
        let maximum = stricter_maximum(self.maximum, other.maximum);
        Self::new(minimum, maximum)
    }
}

fn next_finite(value: f64) -> Option<f64> {
    if value == f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits + 1
    } else {
        bits - 1
    }))
}

fn previous_finite(value: f64) -> Option<f64> {
    if value == -f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(f64::from_bits((1_u64 << 63) | 1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits - 1
    } else {
        bits + 1
    }))
}

impl<'de> Deserialize<'de> for NumberRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            #[serde(default)]
            minimum: Option<NumberBound>,
            #[serde(default)]
            maximum: Option<NumberBound>,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.minimum, repr.maximum)
            .ok_or_else(|| serde::de::Error::custom("number range must be non-empty and ordered"))
    }
}

fn stricter_minimum(left: Option<NumberBound>, right: Option<NumberBound>) -> Option<NumberBound> {
    match (left, right) {
        (None, bound) | (bound, None) => bound,
        (Some(left), Some(right)) => {
            let left_value = left.value.get();
            let right_value = right.value.get();
            if left_value > right_value {
                Some(left)
            } else if right_value > left_value {
                Some(right)
            } else if left.exclusive || right.exclusive {
                Some(NumberBound::exclusive(left.value))
            } else {
                Some(left)
            }
        }
    }
}

fn stricter_maximum(left: Option<NumberBound>, right: Option<NumberBound>) -> Option<NumberBound> {
    match (left, right) {
        (None, bound) | (bound, None) => bound,
        (Some(left), Some(right)) => {
            let left_value = left.value.get();
            let right_value = right.value.get();
            if left_value < right_value {
                Some(left)
            } else if right_value < left_value {
                Some(right)
            } else if left.exclusive || right.exclusive {
                Some(NumberBound::exclusive(left.value))
            } else {
                Some(left)
            }
        }
    }
}

/// One exact numeric interval attached to a concrete scalar schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "bounds", rename_all = "snake_case")]
pub enum NumericRange {
    Integer(IntegerRange),
    Number(NumberRange),
}

impl NumericRange {
    pub fn intersection(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => {
                left.intersection(right).map(Self::Integer)
            }
            (Self::Number(left), Self::Number(right)) => left.intersection(right).map(Self::Number),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NumberBound, NumberRange};
    use crate::FiniteF64;

    #[test]
    fn number_ranges_require_a_representable_finite_value() {
        let Some(maximum) = FiniteF64::new(f64::MAX) else {
            panic!("f64::MAX is finite");
        };
        let Some(minimum) = FiniteF64::new(-f64::MAX) else {
            panic!("-f64::MAX is finite");
        };
        assert!(NumberRange::new(Some(NumberBound::exclusive(maximum)), None).is_none());
        assert!(NumberRange::new(None, Some(NumberBound::exclusive(minimum))).is_none());
        assert!(
            NumberRange::new(
                Some(NumberBound::inclusive(maximum)),
                Some(NumberBound::inclusive(maximum)),
            )
            .is_some()
        );

        let Some(one) = FiniteF64::new(1.0) else {
            panic!("one is finite");
        };
        let Some(next) = FiniteF64::new(f64::from_bits(1.0_f64.to_bits() + 1)) else {
            panic!("successor of one is finite");
        };
        assert!(
            NumberRange::new(
                Some(NumberBound::exclusive(one)),
                Some(NumberBound::exclusive(next)),
            )
            .is_none()
        );
        assert!(
            NumberRange::new(
                Some(NumberBound::inclusive(one)),
                Some(NumberBound::exclusive(next)),
            )
            .is_some()
        );

        let Some(negative_zero) = FiniteF64::new(-0.0) else {
            panic!("negative zero is finite");
        };
        assert!(
            NumberRange::new(
                Some(NumberBound::inclusive(negative_zero)),
                Some(NumberBound::inclusive(negative_zero)),
            )
            .is_some()
        );
        let Some(least_positive) = FiniteF64::new(f64::from_bits(1)) else {
            panic!("least positive number is finite");
        };
        let Some(range) = NumberRange::new(
            Some(NumberBound::exclusive(negative_zero)),
            Some(NumberBound::inclusive(least_positive)),
        ) else {
            panic!("signed-zero interval contains the least positive number");
        };
        assert!(range.contains(least_positive.get()));
        assert!(!range.contains(0.0));
    }
}
