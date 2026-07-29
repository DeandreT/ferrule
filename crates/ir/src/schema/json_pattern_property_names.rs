use json_pattern::{DEFAULT_MATCH_WORK_LIMIT, PortableJsonPattern};
use serde::{Deserialize, Serialize};

use super::{JsonPatternConstraints, JsonPatternConstraintsError};

/// An ordered, bounded disjunction of portable patterns selecting dynamic
/// JSON object property names.
///
/// Unlike [`JsonPatternConstraints`], every source is an independent
/// alternative. This type cannot represent conjunctions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonPatternPropertyNames {
    sources: Vec<String>,
    #[serde(skip)]
    patterns: JsonPatternConstraints,
}

impl JsonPatternPropertyNames {
    pub fn new<I, S>(sources: I) -> Result<Self, JsonPatternConstraintsError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut canonical = Vec::new();
        for source in sources {
            let source = source.into();
            if source.is_empty() {
                canonical.clear();
                canonical.push(source);
                break;
            }
            if !canonical.contains(&source) {
                canonical.push(source);
            }
        }
        let patterns = JsonPatternConstraints::new(
            canonical
                .iter()
                .map(|source| std::iter::once(source.clone())),
        )?;
        let sources = canonical;
        Ok(Self { sources, patterns })
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    pub fn matches(&self, name: &str) -> bool {
        self.all_match(std::iter::once(name))
    }

    /// Checks every property name under one shared deterministic work budget.
    ///
    /// Each selector is compiled once for the complete batch.
    pub fn all_match<'a, I>(&self, names: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.matches_expected(names.into_iter().map(|name| (name, true)))
    }

    pub(crate) fn matches_expected<'a, I>(&self, names: I) -> bool
    where
        I: IntoIterator<Item = (&'a str, bool)>,
    {
        let Ok(programs) = self
            .sources
            .iter()
            .map(|source| PortableJsonPattern::compile(source))
            .collect::<Result<Vec<_>, _>>()
        else {
            return false;
        };
        let mut remaining_work = DEFAULT_MATCH_WORK_LIMIT;
        for (name, expected) in names {
            let mut matched = false;
            for program in &programs {
                match program.is_match_with_budget(name, &mut remaining_work) {
                    Ok(true) => {
                        matched = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(_) => return false,
                }
            }
            if matched != expected {
                return false;
            }
        }
        true
    }

    pub(crate) fn patterns(&self) -> &JsonPatternConstraints {
        &self.patterns
    }
}

impl<'de> Deserialize<'de> for JsonPatternPropertyNames {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            sources: Vec<String>,
        }

        let repr = Repr::deserialize(deserializer)?;
        let selectors = Self::new(repr.sources.clone()).map_err(serde::de::Error::custom)?;
        if selectors.sources != repr.sources {
            return Err(serde::de::Error::custom(
                "JSON pattern property-name selectors must be canonical and contain no duplicates",
            ));
        }
        Ok(selectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        JsonPatternConstraintsError, MAX_JSON_PATTERN_ALTERNATIVES, MAX_JSON_PATTERN_SOURCE_BYTES,
    };

    #[test]
    fn construction_preserves_order_deduplicates_and_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let selectors = JsonPatternPropertyNames::new(["^x-", "^meta-", "^x-"])?;
        assert_eq!(
            selectors.sources(),
            &["^x-".to_string(), "^meta-".to_string()]
        );
        assert!(selectors.matches("x-id"));
        assert!(selectors.matches("meta-created"));
        assert!(!selectors.matches("ordinary"));
        assert!(selectors.all_match(["x-id", "meta-created"]));
        assert!(!selectors.all_match(["x-id", "ordinary"]));
        assert_eq!(
            serde_json::to_string(&selectors)?,
            r#"{"sources":["^x-","^meta-"]}"#
        );
        Ok(())
    }

    #[test]
    fn empty_pattern_canonicalizes_to_the_only_selector() -> Result<(), Box<dyn std::error::Error>>
    {
        let selectors = JsonPatternPropertyNames::new(["^x-", "", "^meta-"])?;
        assert_eq!(selectors.sources(), &[String::new()]);
        assert!(selectors.matches("anything"));
        Ok(())
    }

    #[test]
    fn serialized_selectors_must_be_canonical() {
        for invalid in [
            r#"{}"#,
            r#"{"sources":[]}"#,
            r#"{"sources":["^x-","^x-"]}"#,
            r#"{"sources":["^x-",""]}"#,
            r#"{"sources":["["]}"#,
            r#"{"sources":"^x-"}"#,
            r#"{"sources":["^x-"],"extra":true}"#,
        ] {
            assert!(
                serde_json::from_str::<JsonPatternPropertyNames>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn construction_reuses_portable_pattern_limits() {
        let too_many = (0..=MAX_JSON_PATTERN_ALTERNATIVES)
            .map(|index| format!("^{index}$"))
            .collect::<Vec<_>>();
        assert_eq!(
            JsonPatternPropertyNames::new(too_many),
            Err(JsonPatternConstraintsError::TooManyAlternatives)
        );
        assert_eq!(
            JsonPatternPropertyNames::new([
                String::from("x").repeat(MAX_JSON_PATTERN_SOURCE_BYTES + 1)
            ]),
            Err(JsonPatternConstraintsError::TooManySourceBytes)
        );
        assert!(matches!(
            JsonPatternPropertyNames::new(["(?=x)"]),
            Err(JsonPatternConstraintsError::InvalidPattern { .. })
        ));
    }
}
