use json_pattern::{CompileErrorKind, DEFAULT_MATCH_WORK_LIMIT, PortableJsonPattern};
use serde::{Deserialize, Serialize};

pub const MAX_JSON_PATTERN_ALTERNATIVES: usize = 32;
pub const MAX_JSON_PATTERN_TERMS: usize = 64;
pub const MAX_DISTINCT_JSON_PATTERNS: usize = 64;
pub const MAX_JSON_PATTERN_SOURCE_BYTES: usize = 256 * 1024;
pub const MAX_JSON_PATTERN_INSTRUCTIONS: usize = 65_536;

/// A bounded disjunction of conjunctions over JSON string patterns.
///
/// Sources remain in declaration order. Programmatic construction omits
/// duplicate terms and alternatives while serialized IR must already be
/// canonical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonPatternConstraints {
    any_of: Vec<Vec<String>>,
}

impl JsonPatternConstraints {
    pub fn new<I, A, S>(alternatives: I) -> Result<Self, JsonPatternConstraintsError>
    where
        I: IntoIterator<Item = A>,
        A: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut any_of = Vec::new();
        for alternative in alternatives {
            let mut terms = Vec::new();
            for source in alternative {
                let source = source.into();
                if !terms.contains(&source) {
                    terms.push(source);
                }
            }
            if terms.is_empty() {
                return Err(JsonPatternConstraintsError::EmptyAlternative);
            }
            if !any_of.contains(&terms) {
                any_of.push(terms);
            }
        }
        Self::from_programmatic(any_of)
    }

    pub fn any_of(&self) -> &[Vec<String>] {
        &self.any_of
    }

    pub fn into_any_of(self) -> Vec<Vec<String>> {
        self.any_of
    }

    /// Matches the DNF under one shared deterministic work budget.
    ///
    /// Construction guarantees that every source compiles. A false result can
    /// therefore mean either that no alternative matched or that the fixed
    /// work budget was insufficient for this unusually large input.
    pub fn matches(&self, value: &str) -> bool {
        let mut remaining_work = DEFAULT_MATCH_WORK_LIMIT;
        for alternative in &self.any_of {
            let mut matched = true;
            for source in alternative {
                let Ok(compiled) = PortableJsonPattern::compile(source) else {
                    return false;
                };
                match compiled.is_match_with_budget(value, &mut remaining_work) {
                    Ok(true) => {}
                    Ok(false) => {
                        matched = false;
                        break;
                    }
                    Err(_) => return false,
                }
            }
            if matched {
                return true;
            }
        }
        false
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, JsonPatternConstraintsError> {
        if self.is_tautology() {
            return Ok(other.clone());
        }
        if other.is_tautology() {
            return Ok(self.clone());
        }
        let mut any_of = Vec::new();
        for left in &self.any_of {
            for right in &other.any_of {
                let mut terms = Vec::new();
                for source in left.iter().chain(right) {
                    if !terms.contains(source) {
                        if terms.len() == MAX_JSON_PATTERN_TERMS {
                            return Err(JsonPatternConstraintsError::TooManyTerms);
                        }
                        terms.push(source.clone());
                    }
                }
                if !any_of.contains(&terms) {
                    if any_of.len() == MAX_JSON_PATTERN_ALTERNATIVES {
                        return Err(JsonPatternConstraintsError::TooManyAlternatives);
                    }
                    any_of.push(terms);
                }
            }
        }
        Self::from_programmatic(any_of)
    }

    pub fn union(&self, other: &Self) -> Result<Self, JsonPatternConstraintsError> {
        if self.is_tautology() {
            return Ok(self.clone());
        }
        if other.is_tautology() {
            return Ok(other.clone());
        }
        let mut any_of = self.any_of.clone();
        for alternative in &other.any_of {
            if !any_of.contains(alternative) {
                if any_of.len() == MAX_JSON_PATTERN_ALTERNATIVES {
                    return Err(JsonPatternConstraintsError::TooManyAlternatives);
                }
                any_of.push(alternative.clone());
            }
        }
        Self::from_programmatic(any_of)
    }

    pub fn is_tautology(&self) -> bool {
        matches!(
            self.any_of.as_slice(),
            [alternative]
                if matches!(alternative.as_slice(), [source] if source.is_empty())
        )
    }

    fn from_programmatic(any_of: Vec<Vec<String>>) -> Result<Self, JsonPatternConstraintsError> {
        let mut normalized = Vec::new();
        let mut tautology = false;
        for mut alternative in any_of {
            if alternative.is_empty() {
                return Err(JsonPatternConstraintsError::EmptyAlternative);
            }
            alternative.retain(|source| !source.is_empty());
            if alternative.is_empty() {
                tautology = true;
                continue;
            }
            if !normalized.contains(&alternative) {
                normalized.push(alternative);
            }
        }
        if tautology {
            return Self::from_canonical(vec![vec![String::new()]]);
        }
        Self::from_canonical(normalized)
    }

    fn from_canonical(any_of: Vec<Vec<String>>) -> Result<Self, JsonPatternConstraintsError> {
        validate_canonical(&any_of)?;
        Ok(Self { any_of })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonPatternConstraintsError {
    Empty,
    EmptyAlternative,
    DuplicateTerm,
    DuplicateAlternative,
    NonCanonicalTautology,
    TooManyAlternatives,
    TooManyTerms,
    TooManyDistinctPatterns,
    TooManySourceBytes,
    TooManyInstructions,
    InvalidPattern {
        alternative: usize,
        term: usize,
        kind: CompileErrorKind,
        byte_offset: usize,
    },
}

impl core::fmt::Display for JsonPatternConstraintsError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "JSON pattern constraints are empty"),
            Self::EmptyAlternative => {
                write!(formatter, "a JSON pattern alternative has no terms")
            }
            Self::DuplicateTerm => {
                write!(
                    formatter,
                    "a JSON pattern alternative contains a duplicate term"
                )
            }
            Self::DuplicateAlternative => {
                write!(
                    formatter,
                    "JSON pattern constraints contain a duplicate alternative"
                )
            }
            Self::NonCanonicalTautology => write!(
                formatter,
                "empty JSON pattern terms are canonical only as the sole tautological alternative"
            ),
            Self::TooManyAlternatives => write!(
                formatter,
                "JSON pattern constraints exceed the {MAX_JSON_PATTERN_ALTERNATIVES}-alternative limit"
            ),
            Self::TooManyTerms => write!(
                formatter,
                "JSON pattern constraints exceed the {MAX_JSON_PATTERN_TERMS}-term limit"
            ),
            Self::TooManyDistinctPatterns => write!(
                formatter,
                "JSON pattern constraints exceed the {MAX_DISTINCT_JSON_PATTERNS}-distinct-pattern limit"
            ),
            Self::TooManySourceBytes => write!(
                formatter,
                "JSON pattern constraints exceed the {MAX_JSON_PATTERN_SOURCE_BYTES}-byte source limit"
            ),
            Self::TooManyInstructions => write!(
                formatter,
                "JSON pattern constraints exceed the {MAX_JSON_PATTERN_INSTRUCTIONS}-instruction limit"
            ),
            Self::InvalidPattern {
                alternative,
                term,
                kind,
                byte_offset,
            } => {
                let alternative = alternative + 1;
                let term = term + 1;
                write!(
                    formatter,
                    "JSON pattern alternative {alternative}, term {term} is invalid at byte {byte_offset}: {kind}"
                )
            }
        }
    }
}

impl std::error::Error for JsonPatternConstraintsError {}

impl<'de> Deserialize<'de> for JsonPatternConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            any_of: Vec<Vec<String>>,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::from_canonical(repr.any_of).map_err(serde::de::Error::custom)
    }
}

fn validate_canonical(any_of: &[Vec<String>]) -> Result<(), JsonPatternConstraintsError> {
    if any_of.is_empty() {
        return Err(JsonPatternConstraintsError::Empty);
    }
    if any_of.len() > MAX_JSON_PATTERN_ALTERNATIVES {
        return Err(JsonPatternConstraintsError::TooManyAlternatives);
    }
    if any_of.len() != 1
        && any_of.iter().any(|alternative| {
            alternative.len() == 1 && alternative.first().is_some_and(String::is_empty)
        })
    {
        return Err(JsonPatternConstraintsError::NonCanonicalTautology);
    }

    let mut terms = 0_usize;
    let mut source_bytes = 0_usize;
    let mut instructions = 0_usize;
    let mut distinct_patterns: Vec<&str> = Vec::new();
    for (alternative_index, alternative) in any_of.iter().enumerate() {
        if alternative.is_empty() {
            return Err(JsonPatternConstraintsError::EmptyAlternative);
        }
        if any_of[..alternative_index].contains(alternative) {
            return Err(JsonPatternConstraintsError::DuplicateAlternative);
        }
        if alternative.len() != 1 && alternative.iter().any(String::is_empty) {
            return Err(JsonPatternConstraintsError::NonCanonicalTautology);
        }
        for (term_index, source) in alternative.iter().enumerate() {
            if alternative[..term_index].contains(source) {
                return Err(JsonPatternConstraintsError::DuplicateTerm);
            }
            terms = terms
                .checked_add(1)
                .ok_or(JsonPatternConstraintsError::TooManyTerms)?;
            if terms > MAX_JSON_PATTERN_TERMS {
                return Err(JsonPatternConstraintsError::TooManyTerms);
            }
            if !distinct_patterns.contains(&source.as_str()) {
                if distinct_patterns.len() == MAX_DISTINCT_JSON_PATTERNS {
                    return Err(JsonPatternConstraintsError::TooManyDistinctPatterns);
                }
                source_bytes = source_bytes
                    .checked_add(source.len())
                    .ok_or(JsonPatternConstraintsError::TooManySourceBytes)?;
                if source_bytes > MAX_JSON_PATTERN_SOURCE_BYTES {
                    return Err(JsonPatternConstraintsError::TooManySourceBytes);
                }
                let validation = PortableJsonPattern::validate(source).map_err(|error| {
                    JsonPatternConstraintsError::InvalidPattern {
                        alternative: alternative_index,
                        term: term_index,
                        kind: error.kind(),
                        byte_offset: error.byte_offset(),
                    }
                })?;
                instructions = instructions
                    .checked_add(validation.instruction_count())
                    .ok_or(JsonPatternConstraintsError::TooManyInstructions)?;
                if instructions > MAX_JSON_PATTERN_INSTRUCTIONS {
                    return Err(JsonPatternConstraintsError::TooManyInstructions);
                }
                distinct_patterns.push(source.as_str());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_preserves_order_and_deduplicates_programmatic_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let constraints = JsonPatternConstraints::new([
            ["^A".to_string(), "Z$".to_string(), "^A".to_string()],
            ["^B".to_string(), "^B".to_string(), "^B".to_string()],
            ["^B".to_string(), "^B".to_string(), "^B".to_string()],
        ])?;
        assert_eq!(
            constraints.any_of(),
            &[
                vec!["^A".to_string(), "Z$".to_string()],
                vec!["^B".to_string()],
            ]
        );
        assert_eq!(
            serde_json::to_string(&constraints)?,
            r#"{"any_of":[["^A","Z$"],["^B"]]}"#
        );
        assert!(constraints.matches("ABZ"));
        assert!(constraints.matches("Beta"));
        assert!(!constraints.matches("other"));
        Ok(())
    }

    #[test]
    fn programmatic_empty_terms_are_canonicalized_as_boolean_identities()
    -> Result<(), Box<dyn std::error::Error>> {
        let conjunction = JsonPatternConstraints::new([["", "^A$", ""]])?;
        assert_eq!(conjunction.any_of(), &[vec!["^A$".to_string()]]);

        let tautology = JsonPatternConstraints::new([["^A$"], [""], ["^B$"]])?;
        assert_eq!(tautology.any_of(), &[vec![String::new()]]);

        let constrained = JsonPatternConstraints::new([["^A$"]])?;
        assert_eq!(
            tautology.intersection(&constrained)?.any_of(),
            constrained.any_of()
        );
        assert_eq!(
            tautology.union(&constrained)?.any_of(),
            &[vec![String::new()]]
        );

        let maximum_conjunction = JsonPatternConstraints::new([(0..MAX_JSON_PATTERN_TERMS)
            .map(|index| format!("^{index}$"))
            .collect::<Vec<_>>()])?;
        assert_eq!(
            tautology.intersection(&maximum_conjunction)?,
            maximum_conjunction
        );
        let maximum_disjunction = JsonPatternConstraints::new(
            (0..MAX_JSON_PATTERN_ALTERNATIVES).map(|index| vec![format!("^{index}$")]),
        )?;
        assert_eq!(tautology.union(&maximum_disjunction)?, tautology);
        Ok(())
    }

    #[test]
    fn intersection_and_union_retain_dnf_semantics_and_declaration_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let left = JsonPatternConstraints::new([["^A"], ["^B"]])?;
        let right = JsonPatternConstraints::new([["Z$"], ["^A"]])?;

        assert_eq!(
            left.intersection(&right)?.any_of(),
            &[
                vec!["^A".to_string(), "Z$".to_string()],
                vec!["^A".to_string()],
                vec!["^B".to_string(), "Z$".to_string()],
                vec!["^B".to_string(), "^A".to_string()],
            ]
        );
        assert_eq!(
            left.union(&right)?.any_of(),
            &[
                vec!["^A".to_string()],
                vec!["^B".to_string()],
                vec!["Z$".to_string()],
            ]
        );
        Ok(())
    }

    #[test]
    fn serialized_ir_must_be_nonempty_and_canonical() {
        for invalid in [
            r#"{}"#,
            r#"{"any_of":[]}"#,
            r#"{"any_of":[[]]}"#,
            r#"{"any_of":[["A","A"]]}"#,
            r#"{"any_of":[["A"],["A"]]}"#,
            r#"{"any_of":[["","A"]]}"#,
            r#"{"any_of":[[""],["A"]]}"#,
            r#"{"any_of":[["A"]],"extra":true}"#,
            r#"{"any_of":[["["]]}"#,
            r#"{"any_of":"A"}"#,
        ] {
            assert!(serde_json::from_str::<JsonPatternConstraints>(invalid).is_err());
        }
    }

    #[test]
    fn invalid_patterns_report_their_declaration_position_and_compile_reason() {
        let Err(error) = JsonPatternConstraints::new([["(?=A)"]]) else {
            panic!("lookahead is outside the portable pattern language");
        };
        let error = error.to_string();
        assert!(error.contains("alternative 1, term 1"));
        assert!(error.contains("invalid at byte"));
    }

    #[test]
    fn construction_enforces_alternative_and_term_budgets() {
        let alternatives = (0..=MAX_JSON_PATTERN_ALTERNATIVES)
            .map(|index| vec![format!("^{index}$")])
            .collect::<Vec<_>>();
        assert_eq!(
            JsonPatternConstraints::new(alternatives),
            Err(JsonPatternConstraintsError::TooManyAlternatives)
        );

        let terms = (0..=MAX_JSON_PATTERN_TERMS)
            .map(|index| format!("^{index}$"))
            .collect::<Vec<_>>();
        assert_eq!(
            JsonPatternConstraints::new([terms]),
            Err(JsonPatternConstraintsError::TooManyTerms)
        );

        assert_eq!(
            JsonPatternConstraints::new([[
                String::from("x").repeat(MAX_JSON_PATTERN_SOURCE_BYTES + 1)
            ]]),
            Err(JsonPatternConstraintsError::TooManySourceBytes)
        );

        let instruction_heavy = (0..14)
            .map(|index| format!("{}{index}", "a".repeat(5_000)))
            .collect::<Vec<_>>();
        assert_eq!(
            JsonPatternConstraints::new([instruction_heavy]),
            Err(JsonPatternConstraintsError::TooManyInstructions)
        );
    }

    #[test]
    fn cartesian_composition_stops_at_the_output_budget() -> Result<(), Box<dyn std::error::Error>>
    {
        let left = JsonPatternConstraints::new(
            (0..8)
                .map(|index| vec![format!("^L{index}")])
                .collect::<Vec<_>>(),
        )?;
        let right = JsonPatternConstraints::new(
            (0..8)
                .map(|index| vec![format!("R{index}$")])
                .collect::<Vec<_>>(),
        )?;
        assert_eq!(
            left.intersection(&right),
            Err(JsonPatternConstraintsError::TooManyAlternatives)
        );
        Ok(())
    }
}
