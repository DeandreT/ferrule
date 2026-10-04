//! Admission counters for complete XML output sets; these are not heap/RSS limits.
use std::{error::Error, fmt};

use super::{XmlBoundaryError, XmlBoundaryErrorKind};

pub const MAX_XML_OUTPUT_ARTIFACTS: u64 = 4096;
pub const MAX_XML_OUTPUT_SET_BYTES: u64 = 256 * 1024 * 1024;

/// Serialization identifies its exact target; parse/mapping errors have no owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlOutputTarget {
    Primary,
    Named { index: usize, name: &'static str },
}

#[derive(Debug)]
pub struct XmlOutputSetError {
    pub target: Option<XmlOutputTarget>,
    pub boundary: XmlBoundaryError,
}

impl XmlOutputSetError {
    pub fn new(target: Option<XmlOutputTarget>, boundary: XmlBoundaryError) -> Self {
        Self { target, boundary }
    }

    pub fn into_boundary(self) -> XmlBoundaryError {
        self.boundary
    }

    /// Defensive alignment check before any output is serialized.
    pub fn alignment(detail: impl Into<String>) -> Self {
        Self::from(XmlBoundaryError::with_source(
            XmlBoundaryErrorKind::Output,
            XmlOutputSetAlignmentError(detail.into()),
        ))
    }
}

impl From<XmlBoundaryError> for XmlOutputSetError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self::new(None, boundary)
    }
}

impl fmt::Display for XmlOutputSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.target {
            Some(XmlOutputTarget::Primary) => write!(f, "primary XML output: {}", self.boundary),
            Some(XmlOutputTarget::Named { index, name }) => {
                write!(f, "named XML output {index} `{name}`: {}", self.boundary)
            }
            None => write!(f, "{}", self.boundary),
        }
    }
}
impl Error for XmlOutputSetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.boundary)
    }
}

#[derive(Debug)]
struct XmlOutputSetAlignmentError(String);
impl fmt::Display for XmlOutputSetAlignmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Error for XmlOutputSetAlignmentError {}

/// A set limit is an Output cause, distinct from a per-document DocumentLimit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlOutputSetResourceError {
    pub resource: &'static str,
    pub observed_count: u64,
    pub limit: u64,
}
impl fmt::Display for XmlOutputSetResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is {}; maximum is {}",
            self.resource, self.observed_count, self.limit
        )
    }
}
impl Error for XmlOutputSetResourceError {}

pub struct XmlOutputSetBudget {
    bytes: u64,
}
impl XmlOutputSetBudget {
    pub fn new(artifact_count: usize) -> Result<Self, XmlOutputSetError> {
        let observed = u64::try_from(artifact_count).unwrap_or(u64::MAX);
        if observed > MAX_XML_OUTPUT_ARTIFACTS {
            return Err(Self::refusal(
                None,
                "xml_output_artifact_count",
                observed,
                MAX_XML_OUTPUT_ARTIFACTS,
            ));
        }
        Ok(Self { bytes: 0 })
    }

    /// Charge the just-serialized UTF-8 document before retaining/converting it.
    /// All typed targets and one temporary XML document may already be live.
    pub fn charge(
        &mut self,
        target: XmlOutputTarget,
        bytes: usize,
    ) -> Result<(), XmlOutputSetError> {
        let observed = self
            .bytes
            .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        if observed > MAX_XML_OUTPUT_SET_BYTES {
            return Err(Self::refusal(
                Some(target),
                "xml_output_set_utf8_bytes",
                observed,
                MAX_XML_OUTPUT_SET_BYTES,
            ));
        }
        self.bytes = observed;
        Ok(())
    }

    fn refusal(
        target: Option<XmlOutputTarget>,
        resource: &'static str,
        observed_count: u64,
        limit: u64,
    ) -> XmlOutputSetError {
        XmlOutputSetError::new(
            target,
            XmlBoundaryError::with_source(
                XmlBoundaryErrorKind::Output,
                XmlOutputSetResourceError {
                    resource,
                    observed_count,
                    limit,
                },
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_set_counts_primary_and_charges_utf8_before_accepting_next_document() {
        assert!(XmlOutputSetBudget::new(4096).is_ok());
        let count = XmlOutputSetBudget::new(4097).err().unwrap();
        assert_eq!(count.target, None);
        assert_eq!(count.boundary.kind, XmlBoundaryErrorKind::Output);
        assert_eq!((count.boundary.bytes, count.boundary.limit), (None, None));
        let cause = count
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(
            (cause.resource, cause.observed_count, cause.limit),
            ("xml_output_artifact_count", 4097, 4096)
        );
        let mut budget = XmlOutputSetBudget::new(5).unwrap();
        // Real cap, counted without allocating four 64 MiB buffers.
        for target in [
            XmlOutputTarget::Primary,
            XmlOutputTarget::Named {
                index: 0,
                name: "a",
            },
            XmlOutputTarget::Named {
                index: 1,
                name: "b",
            },
        ] {
            budget
                .charge(target, super::super::MAX_XML_DOCUMENT_BYTES)
                .unwrap();
        }
        let last = XmlOutputTarget::Named {
            index: 2,
            name: "c",
        };
        budget
            .charge(last, super::super::MAX_XML_DOCUMENT_BYTES - 1)
            .unwrap();
        budget.charge(last, 1).unwrap();
        let next = XmlOutputTarget::Named {
            index: 3,
            name: "d",
        };
        let refused = budget.charge(next, 1).unwrap_err();
        assert_eq!(refused.target, Some(next));
        assert_eq!(
            (refused.boundary.bytes, refused.boundary.limit),
            (None, None)
        );
        let cause = refused
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(cause.observed_count, MAX_XML_OUTPUT_SET_BYTES + 1);
        assert_eq!(cause.limit, MAX_XML_OUTPUT_SET_BYTES);
    }

    #[test]
    fn external_counter_overflow_refuses_without_mutating_the_accepted_budget() {
        let mut budget = XmlOutputSetBudget::new(2).unwrap();
        budget.charge(XmlOutputTarget::Primary, 1).unwrap();
        let target = XmlOutputTarget::Named {
            index: 0,
            name: "overflow",
        };
        // No large allocation or altered cap: an arbitrary external count must
        // refuse, even when checked u64 addition cannot represent its sum.
        let refused = budget.charge(target, usize::MAX).unwrap_err();
        assert_eq!(refused.target, Some(target));
        assert_eq!(refused.boundary.kind, XmlBoundaryErrorKind::Output);
        let cause = refused
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(cause.resource, "xml_output_set_utf8_bytes");
        assert_eq!(cause.limit, MAX_XML_OUTPUT_SET_BYTES);
        assert!(cause.observed_count > cause.limit);
        if usize::BITS == 64 {
            assert_eq!(cause.observed_count, u64::MAX);
        }
        assert_eq!(
            (refused.boundary.bytes, refused.boundary.limit),
            (None, None)
        );
        budget
            .charge(target, MAX_XML_OUTPUT_SET_BYTES as usize - 1)
            .unwrap();
        let next = budget.charge(target, 1).unwrap_err();
        let cause = next
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(cause.observed_count, MAX_XML_OUTPUT_SET_BYTES + 1);
    }

    #[test]
    fn set_wrapper_and_single_unwrap_preserve_original_typed_source_chain() {
        let boundary = XmlBoundaryError::with_source(
            XmlBoundaryErrorKind::Output,
            XmlOutputSetResourceError {
                resource: "test",
                observed_count: 9,
                limit: 8,
            },
        );
        let set = XmlOutputSetError::new(
            Some(XmlOutputTarget::Named {
                index: 0,
                name: "audit",
            }),
            boundary,
        );
        assert!(
            set.source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .is_some()
        );
        let original = set.into_boundary();
        assert_eq!(original.kind, XmlBoundaryErrorKind::Output);
        assert!(
            original
                .source()
                .unwrap()
                .downcast_ref::<XmlOutputSetResourceError>()
                .is_some()
        );
    }
}
