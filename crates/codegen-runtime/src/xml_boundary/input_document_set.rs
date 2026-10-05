//! Static named inputs and dynamic primary XML members retain exclusive phase ownership.
use super::{XmlBoundaryError, XmlDocumentExecutionError, XmlDocumentOutputOwner, XmlInputSource};
use std::error::Error;
use std::fmt;

/// Exactly one execution phase, or no owner for a global refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmlInputDocumentOwner {
    Input(XmlInputSource),
    Member(XmlDocumentOutputOwner),
}

/// Original XML boundary allocation and its complete standard typed source chain.
#[derive(Debug)]
#[non_exhaustive]
pub struct XmlInputDocumentExecutionError {
    pub owner: Option<XmlInputDocumentOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlInputDocumentExecutionError {
    /// The static input collector forwards its exact owner and original boundary.
    /// This accepts no old output/request fields and is not a conversion from
    /// arbitrary XmlExecutionError values.
    pub fn from_input_boundary(
        input: Option<XmlInputSource>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        Self {
            owner: input.map(XmlInputDocumentOwner::Input),
            boundary,
        }
    }
}

impl From<XmlBoundaryError> for XmlInputDocumentExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
        }
    }
}

impl From<XmlDocumentExecutionError> for XmlInputDocumentExecutionError {
    fn from(error: XmlDocumentExecutionError) -> Self {
        Self {
            owner: error.member.map(XmlInputDocumentOwner::Member),
            boundary: error.boundary,
        }
    }
}

impl fmt::Display for XmlInputDocumentExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlInputDocumentOwner::Input(input)) => write!(f, "XML input {input:?}: ")?,
            Some(XmlInputDocumentOwner::Member(member)) => write!(
                f,
                "XML output {:?} member {} `{}`: ",
                member.target, member.index, member.path
            )?,
            None => {}
        }
        self.boundary.fmt(f)
    }
}

impl Error for XmlInputDocumentExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        XmlBoundaryErrorKind, XmlDocumentSetBudget, XmlInputSetBudget, XmlInputSetResourceError,
        XmlOutputSetResourceError, XmlOutputTarget, parse_structured_xml,
        parse_structured_xml_bytes,
    };
    use super::*;

    #[test]
    fn original_named_utf8_boundary_allocation_and_decoder_cause_remain_exact() {
        let original = Box::new(parse_structured_xml_bytes("{}", &[0xff]).unwrap_err());
        let allocation = original.as_ref() as *const XmlBoundaryError;
        let cause = original.source().unwrap() as *const dyn Error;
        assert!(original.source().unwrap().is::<std::str::Utf8Error>());
        let error = XmlInputDocumentExecutionError::from_input_boundary(
            Some(XmlInputSource::Named {
                index: 1,
                name: "beta",
            }),
            original,
        );
        assert_eq!(
            error.owner,
            Some(XmlInputDocumentOwner::Input(XmlInputSource::Named {
                index: 1,
                name: "beta"
            }))
        );
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        assert!(std::ptr::eq(
            error
                .source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .unwrap(),
            allocation
        ));
        assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
    }

    #[test]
    fn schema_ownership_is_preserved_by_phase_instead_of_blanket_category() {
        let input = XmlInputDocumentExecutionError::from_input_boundary(
            Some(XmlInputSource::Primary),
            Box::new(parse_structured_xml("{", "<Input/>").unwrap_err()),
        );
        assert_eq!(input.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert_eq!(
            input.owner,
            Some(XmlInputDocumentOwner::Input(XmlInputSource::Primary))
        );
        let serializer = XmlDocumentExecutionError::serialization(
            3,
            "same.xml",
            parse_structured_xml("{", "<Input/>").unwrap_err(),
        );
        let original = serializer.boundary.as_ref() as *const XmlBoundaryError;
        let output = XmlInputDocumentExecutionError::from(serializer);
        assert_eq!(output.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert!(output.owner.is_none());
        assert!(std::ptr::eq(output.boundary.as_ref(), original));
    }

    #[test]
    fn independent_input_and_output_count_errors_keep_global_ownership_and_typed_causes() {
        let original = XmlInputSetBudget::new(4097).unwrap_err();
        let input =
            XmlInputDocumentExecutionError::from_input_boundary(original.input, original.boundary);
        assert!(input.owner.is_none());
        assert_eq!(input.boundary.kind, XmlBoundaryErrorKind::Input);
        assert!(
            input
                .boundary
                .source()
                .unwrap()
                .is::<XmlInputSetResourceError>()
        );
        let original = XmlDocumentSetBudget::new(4097).err().unwrap();
        let output = XmlInputDocumentExecutionError::from(original);
        assert!(output.owner.is_none());
        assert_eq!(output.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(
            output
                .boundary
                .source()
                .unwrap()
                .is::<XmlOutputSetResourceError>()
        );
        assert!(XmlDocumentSetBudget::new(0).is_ok());
    }

    #[test]
    fn member_conversion_keeps_final_index_path_and_original_boundary_cause() {
        // Counter-only control; no actual 256 MiB document is constructed.
        let mut budget = XmlDocumentSetBudget::new(5).unwrap();
        for index in 0..4 {
            budget.charge(index, "same.xml", 64 * 1024 * 1024).unwrap();
        }
        let original = budget.charge(4, "../雪😀.xml", 1).unwrap_err();
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let output = XmlInputDocumentExecutionError::from(original);
        let Some(XmlInputDocumentOwner::Member(member)) = output.owner else {
            panic!("expected member");
        };
        assert_eq!((member.index, member.path.as_str()), (4, "../雪😀.xml"));
        assert_eq!(member.target, XmlOutputTarget::Primary);
        assert!(std::ptr::eq(output.boundary.as_ref(), allocation));
        assert!(
            output
                .boundary
                .source()
                .unwrap()
                .is::<XmlOutputSetResourceError>()
        );
    }
}
