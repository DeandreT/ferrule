use serde::{Deserialize, Deserializer, Serialize};

use crate::NodeId;

/// Validated same-shape recursive group filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecursiveFilterPlan {
    children: String,
    items: String,
    predicate: NodeId,
}

impl RecursiveFilterPlan {
    pub fn new(children: String, items: String, predicate: NodeId) -> Option<Self> {
        (!children.is_empty() && !items.is_empty() && children != items).then_some(Self {
            children,
            items,
            predicate,
        })
    }

    pub fn children(&self) -> &str {
        &self.children
    }

    pub fn items(&self) -> &str {
        &self.items
    }

    pub const fn predicate(&self) -> NodeId {
        self.predicate
    }
}

impl<'de> Deserialize<'de> for RecursiveFilterPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            children: String,
            items: String,
            predicate: NodeId,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.children, wire.items, wire.predicate).ok_or_else(|| {
            serde::de::Error::custom(
                "recursive filter paths must be non-empty and identify distinct collections",
            )
        })
    }
}
