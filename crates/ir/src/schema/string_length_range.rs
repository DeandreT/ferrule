use serde::{Deserialize, Serialize};

/// One non-empty interval over JSON string lengths in Unicode scalar values.
///
/// An absent minimum means zero. The unconstrained interval `[0, +inf)` is
/// represented by absent
/// [`SchemaNode::string_length_range`](crate::SchemaNode::string_length_range)
/// metadata rather than by this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StringLengthRange {
    #[serde(default, skip_serializing_if = "is_zero")]
    minimum: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum: Option<u64>,
}

impl StringLengthRange {
    pub fn new(minimum: u64, maximum: Option<u64>) -> Option<Self> {
        (minimum > 0 || maximum.is_some())
            .then_some(())
            .and_then(|()| {
                maximum
                    .is_none_or(|maximum| minimum <= maximum)
                    .then_some(Self { minimum, maximum })
            })
    }

    pub fn minimum(self) -> u64 {
        self.minimum
    }

    pub fn maximum(self) -> Option<u64> {
        self.maximum
    }

    pub fn contains_str(self, value: &str) -> bool {
        self.contains_len(value.chars().count())
    }

    pub fn contains_len(self, length: usize) -> bool {
        match u64::try_from(length) {
            Ok(length) => self.contains_count(length),
            Err(_) => self.maximum.is_none(),
        }
    }

    pub fn contains_count(self, count: u64) -> bool {
        count >= self.minimum && self.maximum.is_none_or(|maximum| count <= maximum)
    }

    pub fn contains_range(self, other: Self) -> bool {
        self.minimum <= other.minimum
            && match (self.maximum, other.maximum) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(maximum), Some(other_maximum)) => maximum >= other_maximum,
            }
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let minimum = self.minimum.max(other.minimum);
        let maximum = match (self.maximum, other.maximum) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        Self::new(minimum, maximum)
    }

    pub fn contiguous_union(self, other: Self) -> Option<Option<Self>> {
        let (left, right) = if self.minimum <= other.minimum {
            (self, other)
        } else {
            (other, self)
        };
        let touches = left.maximum.is_none_or(|maximum| {
            right.minimum <= maximum || maximum.checked_add(1) == Some(right.minimum)
        });
        if !touches {
            return None;
        }
        let maximum = match (left.maximum, right.maximum) {
            (None, _) | (_, None) => None,
            (Some(left), Some(right)) => Some(left.max(right)),
        };
        Some(Self::new(left.minimum, maximum))
    }
}

impl<'de> Deserialize<'de> for StringLengthRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            #[serde(default)]
            minimum: u64,
            #[serde(default)]
            maximum: Option<u64>,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.minimum, repr.maximum).ok_or_else(|| {
            serde::de::Error::custom(
                "string-length range must be constrained, non-empty, and ordered",
            )
        })
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::StringLengthRange;

    #[test]
    fn measures_unicode_scalar_values_and_combines_exact_intervals() {
        let Some(left) = StringLengthRange::new(1, Some(3)) else {
            panic!("left range is valid");
        };
        assert!(left.contains_str("a"));
        assert!(left.contains_str("é😀"));
        let Some(one) = StringLengthRange::new(1, Some(1)) else {
            panic!("single-scalar range is valid");
        };
        assert!(one.contains_str("😀"));
        assert!(!one.contains_str("e\u{301}"));
        let Some(two) = StringLengthRange::new(2, Some(2)) else {
            panic!("two-scalar range is valid");
        };
        assert!(two.contains_str("e\u{301}"));
        assert!(!two.contains_str("😀"));
        assert!(!left.contains_str(""));
        assert!(!left.contains_str("abcd"));

        let Some(right) = StringLengthRange::new(3, Some(5)) else {
            panic!("right range is valid");
        };
        let Some(intersection) = left.intersection(right) else {
            panic!("ranges overlap at three");
        };
        assert_eq!(intersection.minimum(), 3);
        assert_eq!(intersection.maximum(), Some(3));
        let Some(Some(union)) = left.contiguous_union(right) else {
            panic!("ranges form one contiguous interval");
        };
        assert_eq!(union.minimum(), 1);
        assert_eq!(union.maximum(), Some(5));
    }
}
