//! Streaming validation of decoded fields in the existing segment descent.
//! No wire values or parallel instance tree are retained: only borrowed
//! constraints, the active schema path and the ordinary bounded issue report.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mapping::EdiValueConstraint;

use crate::validation::validate_lexical;
use crate::{EdiFormatError, EdiValidationReport};

pub(crate) struct SourceValidation<'a> {
    constraints: BTreeMap<&'a [String], &'a EdiValueConstraint>,
    path: Vec<String>,
    location: String,
    report: EdiValidationReport,
}

impl<'a> SourceValidation<'a> {
    pub(crate) fn new(constraints: &'a [EdiValueConstraint]) -> Self {
        Self {
            constraints: constraints
                .iter()
                .map(|value| (value.path(), value))
                .collect(),
            path: Vec::new(),
            location: String::new(),
            report: EdiValidationReport::default(),
        }
    }

    pub(crate) fn enter(&mut self, name: &str, occurrence: Option<usize>) -> usize {
        let mark = self.location.len();
        if !self.constraints.is_empty() {
            self.path.push(name.to_string());
            if !self.location.is_empty() {
                self.location.push('/');
            }
            self.location.push_str(name);
            if let Some(occurrence) = occurrence {
                let _ = write!(self.location, "[{occurrence}]");
            }
        }
        mark
    }

    pub(crate) fn leave(&mut self, mark: usize) {
        if !self.constraints.is_empty() {
            self.path.pop();
            self.location.truncate(mark);
        }
    }

    pub(crate) fn check(&mut self, raw: &str) {
        // Empty decoded fields become Null under the existing grammar. A
        // lexical constraint does not introduce a separate requiredness rule.
        if !raw.is_empty()
            && !self.report.truncated()
            && let Some(constraint) = self.constraints.get(self.path.as_slice())
        {
            validate_lexical(raw, constraint, &self.location, &mut self.report);
        }
    }

    pub(crate) fn finish(self) -> Result<(), EdiFormatError> {
        if self.report.is_empty() {
            Ok(())
        } else {
            Err(EdiFormatError::Validation(self.report))
        }
    }
}
