use serde::{Deserialize, Serialize};

/// One non-empty interval over JSON object property counts.
///
/// An absent minimum means zero. The unconstrained interval `[0, +inf)` is
/// represented by absent
/// [`SchemaNode::property_count_range`](crate::SchemaNode::property_count_range)
/// metadata rather than by this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PropertyCountRange {
    #[serde(default, skip_serializing_if = "is_zero")]
    minimum: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximum: Option<u64>,
}

impl PropertyCountRange {
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

    pub fn contains_len(self, length: usize) -> bool {
        match u64::try_from(length) {
            Ok(length) => self.contains_count(length),
            Err(_) => self.maximum.is_none(),
        }
    }

    pub fn contains_count(self, count: u64) -> bool {
        count >= self.minimum && self.maximum.is_none_or(|maximum| count <= maximum)
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let minimum = self.minimum.max(other.minimum);
        let maximum = match (self.maximum, other.maximum) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        Self::new(minimum, maximum)
    }
}

impl<'de> Deserialize<'de> for PropertyCountRange {
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
                "property-count range must be constrained, non-empty, and ordered",
            )
        })
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::PropertyCountRange;

    #[test]
    fn intersections_preserve_exact_count_sets() {
        let Some(left) = PropertyCountRange::new(1, Some(3)) else {
            panic!("left range is valid");
        };
        let Some(right) = PropertyCountRange::new(3, Some(5)) else {
            panic!("right range is valid");
        };
        let Some(intersection) = left.intersection(right) else {
            panic!("ranges overlap at three");
        };
        assert_eq!(intersection.minimum(), 3);
        assert_eq!(intersection.maximum(), Some(3));

        let Some(disjoint) = PropertyCountRange::new(5, Some(6)) else {
            panic!("disjoint range is valid");
        };
        assert!(left.intersection(disjoint).is_none());
    }
}
