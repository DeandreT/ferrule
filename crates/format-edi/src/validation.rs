//! Bounded validation of lexical constraints retained from EDI catalogs.

use std::collections::BTreeSet;
use std::fmt;

use ir::{Instance, Value};
use mapping::EdiValueConstraint;

use crate::EdiFormatError;

const MAX_VALIDATION_ISSUES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdiConstraintViolation {
    TooShort { minimum: u16, actual: usize },
    TooLong { maximum: u16, actual: usize },
    NotAllowed { allowed_count: usize },
}

impl fmt::Display for EdiConstraintViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { minimum, actual } => {
                write!(
                    formatter,
                    "contains {actual} character(s), minimum is {minimum}"
                )
            }
            Self::TooLong { maximum, actual } => {
                write!(
                    formatter,
                    "contains {actual} character(s), maximum is {maximum}"
                )
            }
            Self::NotAllowed { allowed_count } => write!(
                formatter,
                "is not one of the {allowed_count} configured code-list value(s)"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdiValidationIssue {
    location: String,
    violation: EdiConstraintViolation,
}

impl EdiValidationIssue {
    pub fn location(&self) -> &str {
        &self.location
    }

    pub const fn violation(&self) -> EdiConstraintViolation {
        self.violation
    }
}

impl fmt::Display for EdiValidationIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EDI value `{}` {}",
            self.location, self.violation
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EdiValidationReport {
    issues: Vec<EdiValidationIssue>,
    truncated: bool,
}

impl EdiValidationReport {
    pub fn issues(&self) -> &[EdiValidationIssue] {
        &self.issues
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

impl fmt::Display for EdiValidationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EDI validation found {} issue(s)",
            self.issues.len()
        )?;
        for issue in self.issues.iter().take(8) {
            write!(formatter, "; {issue}")?;
        }
        if self.issues.len() > 8 || self.truncated {
            formatter.write_str("; additional issues omitted")?;
        }
        Ok(())
    }
}

/// Validates every present scalar selected by retained EDI constraints.
/// Null/absent values remain governed by segment/cardinality rules and are
/// not treated as lexical violations.
pub fn validate_values(
    instance: &Instance,
    constraints: &[EdiValueConstraint],
) -> Result<EdiValidationReport, EdiFormatError> {
    let mut paths = BTreeSet::new();
    let mut report = EdiValidationReport::default();
    for constraint in constraints {
        if !paths.insert(constraint.path()) {
            return Err(EdiFormatError::InvalidValueConstraintLayout {
                path: constraint.path().join("/"),
                reason: "duplicate path",
            });
        }
        validate_at(
            instance,
            constraint.path(),
            constraint.path(),
            &mut String::new(),
            constraint,
            &mut report,
        )?;
    }
    Ok(report)
}

fn validate_at(
    instance: &Instance,
    remaining: &[String],
    full_path: &[String],
    location: &mut String,
    constraint: &EdiValueConstraint,
    report: &mut EdiValidationReport,
) -> Result<(), EdiFormatError> {
    if report.truncated {
        return Ok(());
    }
    match instance {
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            let base_len = location.len();
            for (index, item) in items.iter().enumerate() {
                use std::fmt::Write as _;
                let _ = write!(location, "[{}]", index + 1);
                validate_at(item, remaining, full_path, location, constraint, report)?;
                location.truncate(base_len);
                if report.truncated {
                    break;
                }
            }
            Ok(())
        }
        Instance::Group(fields) => {
            let Some((segment, tail)) = remaining.split_first() else {
                return Err(EdiFormatError::InvalidValueConstraintLayout {
                    path: full_path.join("/"),
                    reason: "path ends at a group",
                });
            };
            let Some((_, value)) = fields.iter().find(|(name, _)| name == segment) else {
                return Ok(());
            };
            let base_len = location.len();
            if !location.is_empty() {
                location.push('/');
            }
            location.push_str(segment);
            let result = validate_at(value, tail, full_path, location, constraint, report);
            location.truncate(base_len);
            result
        }
        Instance::Scalar(value) if remaining.is_empty() => {
            let Some(lexical) = lexical_value(value) else {
                return Ok(());
            };
            let length = lexical.chars().count();
            if length < usize::from(constraint.min_chars()) {
                push_issue(
                    report,
                    location,
                    EdiConstraintViolation::TooShort {
                        minimum: constraint.min_chars(),
                        actual: length,
                    },
                );
            } else if length > usize::from(constraint.max_chars()) {
                push_issue(
                    report,
                    location,
                    EdiConstraintViolation::TooLong {
                        maximum: constraint.max_chars(),
                        actual: length,
                    },
                );
            }
            if !constraint.allowed_values().is_empty()
                && constraint
                    .allowed_values()
                    .binary_search_by(|candidate| candidate.as_str().cmp(lexical.as_ref()))
                    .is_err()
            {
                push_issue(
                    report,
                    location,
                    EdiConstraintViolation::NotAllowed {
                        allowed_count: constraint.allowed_values().len(),
                    },
                );
            }
            Ok(())
        }
        Instance::Scalar(_) => Err(EdiFormatError::InvalidValueConstraintLayout {
            path: full_path.join("/"),
            reason: "path crosses a scalar",
        }),
        Instance::DocumentSet(_) => Err(EdiFormatError::InvalidValueConstraintLayout {
            path: full_path.join("/"),
            reason: "path crosses a document set",
        }),
    }
}

fn lexical_value(value: &Value) -> Option<std::borrow::Cow<'_, str>> {
    match value {
        Value::Null | Value::XmlNil(_) => None,
        Value::String(value) => Some(std::borrow::Cow::Borrowed(value)),
        Value::Bool(value) => Some(std::borrow::Cow::Owned(value.to_string())),
        Value::Int(value) => Some(std::borrow::Cow::Owned(value.to_string())),
        Value::Float(value) => Some(std::borrow::Cow::Owned(value.to_string())),
    }
}

fn push_issue(report: &mut EdiValidationReport, location: &str, violation: EdiConstraintViolation) {
    if report.issues.len() == MAX_VALIDATION_ISSUES {
        report.truncated = true;
        return;
    }
    report.issues.push(EdiValidationIssue {
        location: location.to_string(),
        violation,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_all_length_and_code_violations_across_repetition() {
        let instance = Instance::Group(vec![(
            "Rows".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![(
                    "Code".into(),
                    Instance::Scalar(Value::String("A".into())),
                )]),
                Instance::Group(vec![(
                    "Code".into(),
                    Instance::Scalar(Value::String("LONG".into())),
                )]),
                Instance::Group(vec![("Code".into(), Instance::Scalar(Value::Null))]),
            ]),
        )]);
        let Some(constraint) = EdiValueConstraint::new(
            vec!["Rows".into(), "Code".into()],
            2,
            3,
            vec!["AA".into(), "BB".into()],
        ) else {
            panic!("valid constraint");
        };

        let Ok(report) = validate_values(&instance, &[constraint]) else {
            panic!("valid constraint layout");
        };
        assert_eq!(report.issues().len(), 4);
        assert_eq!(report.issues()[0].location(), "Rows[1]/Code");
        assert_eq!(report.issues()[2].location(), "Rows[2]/Code");
        assert!(!report.truncated());
    }

    #[test]
    fn duplicate_or_structural_paths_are_typed_errors() {
        let Some(constraint) = EdiValueConstraint::new(vec!["Group".into()], 0, 3, Vec::new())
        else {
            panic!("valid constraint");
        };
        let instance = Instance::Group(vec![("Group".into(), Instance::Group(Vec::new()))]);
        assert!(matches!(
            validate_values(&instance, std::slice::from_ref(&constraint)),
            Err(EdiFormatError::InvalidValueConstraintLayout { reason, .. })
                if reason == "path ends at a group"
        ));
        assert!(matches!(
            validate_values(&Instance::Group(Vec::new()), &[constraint.clone(), constraint]),
            Err(EdiFormatError::InvalidValueConstraintLayout { reason, .. })
                if reason == "duplicate path"
        ));
    }
}
