use std::collections::BTreeMap;

use mapping::NodeId;

use crate::Expression;

use super::{ProgramValidationError, sources::SourceCatalog};

pub(super) fn validate(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let Expression::CollectionFind { collection, .. } = expression else {
            continue;
        };
        if sources.path_targets(collection).is_empty() {
            return Err(ProgramValidationError::InvalidCollectionFindCollection {
                node,
                collection: collection.clone(),
            });
        }
    }
    Ok(())
}
