//! Additive static XML input admission. Per-document projection limits remain separate.

use std::fmt;

use super::{XmlBoundaryError, XmlBoundaryErrorKind, XmlOutputSetError, XmlOutputTarget};
use crate::RuntimeError;

pub const MAX_XML_INPUT_ARTIFACTS: u64 = 4096;
pub const MAX_XML_INPUT_SET_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlInputSource {
    Primary,
    Named { index: usize, name: &'static str },
}

/// Exactly one phase owner, or none for whole-execution name/mapping failures.
/// The original boundary and its complete typed source chain are retained.
#[derive(Debug)]
#[non_exhaustive]
pub struct XmlExecutionError {
    pub input: Option<XmlInputSource>,
    pub output: Option<XmlOutputTarget>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlExecutionError {
    pub fn input(source: XmlInputSource, boundary: XmlBoundaryError) -> Self {
        Self {
            input: Some(source),
            output: None,
            boundary: Box::new(boundary),
        }
    }

    pub fn into_boundary(self) -> XmlBoundaryError {
        *self.boundary
    }

    /// Old no-sources set APIs preserve their existing error wrapper.
    pub fn into_output_set(self) -> XmlOutputSetError {
        XmlOutputSetError::new(self.output, *self.boundary)
    }
}

impl From<XmlBoundaryError> for XmlExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            input: None,
            output: None,
            boundary: Box::new(boundary),
        }
    }
}

impl From<XmlOutputSetError> for XmlExecutionError {
    fn from(error: XmlOutputSetError) -> Self {
        Self {
            input: None,
            output: error.target,
            boundary: Box::new(error.boundary),
        }
    }
}

impl fmt::Display for XmlExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(input) = self.input {
            write!(f, "XML input {input:?}: ")?;
        }
        if let Some(output) = self.output {
            write!(f, "XML output {output:?}: ")?;
        }
        self.boundary.fmt(f)
    }
}

impl std::error::Error for XmlExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlInputSetResourceError {
    pub resource: &'static str,
    pub observed_count: u64,
    pub limit: u64,
}

impl fmt::Display for XmlInputSetResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is {}; maximum is {}",
            self.resource, self.observed_count, self.limit
        )
    }
}
impl std::error::Error for XmlInputSetResourceError {}

/// Counts original UTF-8 input, not retained instances or process memory.
#[derive(Debug, Default)]
pub struct XmlInputSetBudget {
    bytes: u64,
}

impl XmlInputSetBudget {
    pub fn new(artifact_count: usize) -> Result<Self, XmlExecutionError> {
        let count = u64::try_from(artifact_count).unwrap_or(u64::MAX);
        if count > MAX_XML_INPUT_ARTIFACTS {
            return Err(refusal(
                None,
                "xml_input_artifact_count",
                count,
                MAX_XML_INPUT_ARTIFACTS,
            ));
        }
        Ok(Self::default())
    }

    pub fn charge(
        &mut self,
        source: XmlInputSource,
        bytes: usize,
    ) -> Result<(), XmlExecutionError> {
        let count = u64::try_from(bytes).unwrap_or(u64::MAX);
        let observed = self.bytes.saturating_add(count);
        if observed > MAX_XML_INPUT_SET_BYTES {
            return Err(refusal(
                Some(source),
                "xml_input_set_utf8_bytes",
                observed,
                MAX_XML_INPUT_SET_BYTES,
            ));
        }
        self.bytes = observed;
        Ok(())
    }
}

fn refusal(
    source: Option<XmlInputSource>,
    resource: &'static str,
    count: u64,
    limit: u64,
) -> XmlExecutionError {
    let cause = XmlInputSetResourceError {
        resource,
        observed_count: count,
        limit,
    };
    let mut error = XmlExecutionError::from(XmlBoundaryError::with_source(
        XmlBoundaryErrorKind::Input,
        cause,
    ));
    error.input = source;
    error
}

/// Supplied-order unexpected/duplicate checks precede declaration-order missing checks.
/// Callers guard count before collecting names; this helper also guards its own allocation.
pub fn xml_input_indices(
    expected: &[&'static str],
    supplied: &[&str],
) -> Result<Vec<usize>, XmlExecutionError> {
    XmlInputSetBudget::new(supplied.len().saturating_add(1))?;
    XmlInputSetBudget::new(expected.len().saturating_add(1))?;
    let mut matched = vec![None; expected.len()];
    for (supplied_index, name) in supplied.iter().copied().enumerate() {
        let Some(index) = expected.iter().position(|expected| *expected == name) else {
            return Err(XmlExecutionError::from(XmlBoundaryError::from(
                RuntimeError::UnexpectedNamedSource {
                    name: name.to_owned(),
                },
            )));
        };
        if matched[index].is_some() {
            return Err(XmlExecutionError::from(XmlBoundaryError::from(
                RuntimeError::DuplicateNamedSource {
                    name: expected[index],
                },
            )));
        }
        matched[index] = Some(supplied_index);
    }
    matched
        .into_iter()
        .enumerate()
        .map(|(index, supplied)| {
            supplied.ok_or_else(|| {
                XmlExecutionError::from(XmlBoundaryError::from(RuntimeError::MissingNamedSource {
                    name: expected[index],
                }))
            })
        })
        .collect()
}

/// All original document sizes are checked before the aggregate is charged.
/// No document has been decoded or projected when this function returns an error.
pub fn preflight_xml_input_sizes(
    sizes: &[(XmlInputSource, usize)],
) -> Result<(), XmlExecutionError> {
    let mut budget = XmlInputSetBudget::new(sizes.len())?;
    for &(source, bytes) in sizes {
        super::check_document_size(bytes)
            .map_err(|error| XmlExecutionError::input(source, error))?;
    }
    for &(source, bytes) in sizes {
        budget.charge(source, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    const A: XmlInputSource = XmlInputSource::Named {
        index: 0,
        name: "A",
    };

    #[test]
    fn exact_names_keep_input_order_failures_and_declaration_order_indices() {
        assert_eq!(xml_input_indices(&["A", "B"], &["B", "A"]).unwrap(), [1, 0]);
        for (names, expected) in [
            (vec!["a", "A"], "unexpected"),
            (vec!["A", "A"], "duplicate"),
            (vec!["B"], "missing"),
        ] {
            let error = xml_input_indices(&["A", "B"], &names).unwrap_err();
            assert!(error.input.is_none() && error.output.is_none());
            let cause = error
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>()
                .unwrap();
            assert!(match (expected, cause) {
                ("unexpected", RuntimeError::UnexpectedNamedSource { name }) => name == "a",
                ("duplicate", RuntimeError::DuplicateNamedSource { name }) => *name == "A",
                ("missing", RuntimeError::MissingNamedSource { name }) => *name == "A",
                _ => false,
            });
        }
    }

    #[test]
    fn full_size_preflight_refuses_a_later_document_before_aggregate_overflow() {
        let cap = super::super::MAX_XML_DOCUMENT_BYTES;
        let fifth = XmlInputSource::Named {
            index: 3,
            name: "D",
        };
        let sixth = XmlInputSource::Named {
            index: 4,
            name: "E",
        };
        let sizes = vec![
            (XmlInputSource::Primary, cap),
            (A, cap),
            (
                XmlInputSource::Named {
                    index: 1,
                    name: "B",
                },
                cap,
            ),
            (
                XmlInputSource::Named {
                    index: 2,
                    name: "C",
                },
                cap,
            ),
            (fifth, cap),
            (sixth, cap + 1),
        ];
        let error = preflight_xml_input_sizes(&sizes).unwrap_err();
        assert_eq!(error.input, Some(sixth));
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::DocumentLimit);
        assert_eq!(
            (error.boundary.bytes, error.boundary.limit),
            (Some(cap + 1), Some(cap))
        );
        assert!(preflight_xml_input_sizes(&sizes[..4]).is_ok());
        let error = preflight_xml_input_sizes(&sizes[..5]).unwrap_err();
        assert_eq!(error.input, Some(fifth));
        let cause = error
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlInputSetResourceError>()
            .unwrap();
        assert_eq!(cause.observed_count, 5 * cap as u64);
        assert_eq!(cause.limit, MAX_XML_INPUT_SET_BYTES);
        assert_eq!((error.boundary.bytes, error.boundary.limit), (None, None));
    }

    #[test]
    fn exact_count_and_failed_charge_preserve_state_and_original_error_chain() {
        assert!(XmlInputSetBudget::new(4096).is_ok());
        let error = XmlInputSetBudget::new(4097).unwrap_err();
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Input);
        assert!(error.input.is_none() && error.output.is_none());
        let mut budget = XmlInputSetBudget::new(1).unwrap();
        budget.charge(A, MAX_XML_INPUT_SET_BYTES as usize).unwrap();
        let error = budget.charge(A, 1).unwrap_err();
        assert_eq!(error.input, Some(A));
        assert!(error.output.is_none());
        assert!(
            error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .is::<XmlInputSetResourceError>()
        );
        assert_eq!(budget.bytes, MAX_XML_INPUT_SET_BYTES);
        assert!(budget.charge(A, usize::MAX).is_err());
        assert_eq!(budget.bytes, MAX_XML_INPUT_SET_BYTES);
    }
}
