//! Document concatenation never changes the count of selected scope contexts.

use ir::{DocumentMember, Instance};

use crate::EngineError;

pub(super) fn extend(
    documents: &mut Vec<DocumentMember>,
    value: Instance,
) -> Result<(), EngineError> {
    match value {
        Instance::DocumentSet(items) => {
            documents.extend(items);
            Ok(())
        }
        Instance::Scalar(_) => {
            Err(EngineError::InvalidDocumentCollectionItem { found: "a scalar" })
        }
        Instance::Group(_) => Err(EngineError::InvalidDocumentCollectionItem { found: "a group" }),
        Instance::Repeated(_) => Err(EngineError::InvalidDocumentCollectionItem {
            found: "a repeated value",
        }),
        Instance::MappedSequence(_) => Err(EngineError::InvalidDocumentCollectionItem {
            found: "a mapped sequence",
        }),
    }
}
