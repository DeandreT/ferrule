use serde::{Deserialize, Deserializer, Serialize};

use crate::NodeId;

/// Validated same-shape recursive group filter.
///
/// Children and items are field names relative to each visited group. The
/// plan never names a source owner: an explicitly owned filter takes its single
/// owner from the scope's owned source selector, so the reader refuses an
/// `owner` key here instead of ignoring a second, competing owner.
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
            #[serde(default, deserialize_with = "present")]
            owner: bool,
        }

        fn present<'de, D>(deserializer: D) -> Result<bool, D::Error>
        where
            D: Deserializer<'de>,
        {
            serde::de::IgnoredAny::deserialize(deserializer).map(|_| true)
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.owner {
            return Err(serde::de::Error::custom(
                "recursive filter ownership belongs to the scope's owned source selector",
            ));
        }
        Self::new(wire.children, wire.items, wire.predicate).ok_or_else(|| {
            serde::de::Error::custom(
                "recursive filter paths must be non-empty and identify distinct collections",
            )
        })
    }
}
