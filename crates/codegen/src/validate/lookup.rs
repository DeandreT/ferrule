use std::collections::BTreeMap;

use ir::SchemaKind;
use mapping::NodeId;

use super::{Expression, ProgramValidationError, sources::SourceCatalog};

pub(super) fn validate(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::Lookup {
            collection,
            key,
            value,
            ..
        } = expression
        else {
            continue;
        };
        let candidates = sources.direct_path_targets(collection);
        if !candidates
            .iter()
            .any(|candidate| candidate.node().repeating)
        {
            return Err(ProgramValidationError::InvalidLookupCollection {
                node,
                collection: collection.clone(),
            });
        }
        let scalar_below_collection = |path: &[String]| {
            candidates.iter().any(|candidate| {
                candidate.node().repeating
                    && candidate.follow_direct(path).is_some_and(|leaf| {
                        matches!(leaf.node().kind, SchemaKind::Scalar { .. })
                            && (path.is_empty() || !leaf.node().repeating)
                    })
            })
        };
        if !scalar_below_collection(key) {
            return Err(ProgramValidationError::InvalidLookupKeyPath {
                node,
                collection: collection.clone(),
                key: key.clone(),
            });
        }
        if !scalar_below_collection(value) {
            return Err(ProgramValidationError::InvalidLookupValuePath {
                node,
                collection: collection.clone(),
                value: value.clone(),
            });
        }
    }
    Ok(())
}
