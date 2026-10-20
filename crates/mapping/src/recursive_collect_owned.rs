//! Recursive collection sequences whose root collection has an explicit owner.
//!
//! Only the root collection selects source data from an endpoint. Child,
//! descent and value paths stay relative to each selected group, and the
//! generated `item` remains a private sequence identity, never an owner.

use serde::{Deserialize, Serialize};

use crate::{NodeId, OwnedSourceRef};

/// The owned counterpart of [`crate::SequenceExpr::RecursiveCollect`].
///
/// The body is strict: unknown keys, a legacy path array in place of the
/// owned collection, and malformed or empty named owners are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedRecursiveCollect {
    /// Endpoint-relative root collection of exactly one Primary or named owner.
    pub collection: OwnedSourceRef,
    /// Repeated child groups, relative to each visited group.
    pub children: Vec<String>,
    /// Scalar appended to the inherited prefix, relative to each visited group.
    pub descent_value: Vec<String>,
    /// Leaf collection, relative to each visited group.
    pub values: Vec<String>,
    /// Scalar leaf value, relative to each leaf item.
    pub value: Vec<String>,
    /// Parent-context prefix evaluated before the walk.
    pub prefix: NodeId,
    /// Parent-context separator evaluated after the prefix.
    pub separator: NodeId,
    /// The private generated-item declaration.
    pub item: NodeId,
}
