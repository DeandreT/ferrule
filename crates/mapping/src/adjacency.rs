use serde::{Deserialize, Deserializer, Serialize};

use crate::NodeId;

/// Validated construction of a recursive target group from flat string-keyed
/// adjacency rows. A missing `root` selects rows whose parent field is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdjacencyTreePlan {
    collection: Vec<String>,
    key: Vec<String>,
    parent: Vec<String>,
    target_key: String,
    target_children: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<NodeId>,
}

impl AdjacencyTreePlan {
    pub fn new(
        collection: Vec<String>,
        key: Vec<String>,
        parent: Vec<String>,
        target_key: String,
        target_children: String,
        root: Option<NodeId>,
    ) -> Option<Self> {
        (valid_path(&collection)
            && valid_path(&key)
            && valid_path(&parent)
            && key != parent
            && !target_key.is_empty()
            && !target_children.is_empty()
            && target_key != target_children)
            .then_some(Self {
                collection,
                key,
                parent,
                target_key,
                target_children,
                root,
            })
    }

    pub fn collection(&self) -> &[String] {
        &self.collection
    }

    pub fn key(&self) -> &[String] {
        &self.key
    }

    pub fn parent(&self) -> &[String] {
        &self.parent
    }

    pub fn target_key(&self) -> &str {
        &self.target_key
    }

    pub fn target_children(&self) -> &str {
        &self.target_children
    }

    pub const fn root(&self) -> Option<NodeId> {
        self.root
    }
}

fn valid_path(path: &[String]) -> bool {
    !path.is_empty() && path.iter().all(|segment| !segment.is_empty())
}

impl<'de> Deserialize<'de> for AdjacencyTreePlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            collection: Vec<String>,
            key: Vec<String>,
            parent: Vec<String>,
            target_key: String,
            target_children: String,
            #[serde(default)]
            root: Option<NodeId>,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.collection,
            wire.key,
            wire.parent,
            wire.target_key,
            wire.target_children,
            wire.root,
        )
        .ok_or_else(|| {
            serde::de::Error::custom(
                "adjacency tree paths and target fields must be non-empty, and key, parent, and target fields must be distinct",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_rejects_empty_or_colliding_fields_during_construction_and_deserialization() {
        assert!(
            AdjacencyTreePlan::new(
                vec!["row".into()],
                vec!["name".into()],
                vec!["base".into()],
                "name".into(),
                "children".into(),
                None,
            )
            .is_some()
        );
        assert!(
            AdjacencyTreePlan::new(
                Vec::new(),
                vec!["name".into()],
                vec!["base".into()],
                "name".into(),
                "children".into(),
                None,
            )
            .is_none()
        );
        assert!(
            AdjacencyTreePlan::new(
                vec!["row".into()],
                vec!["".into()],
                vec!["base".into()],
                "name".into(),
                "children".into(),
                None,
            )
            .is_none()
        );
        assert!(
            AdjacencyTreePlan::new(
                vec!["row".into()],
                vec!["name".into()],
                vec!["name".into()],
                "name".into(),
                "children".into(),
                None,
            )
            .is_none()
        );
        assert!(
            serde_json::from_str::<AdjacencyTreePlan>(
                r#"{"collection":["row"],"key":["name"],"parent":["base"],"target_key":"same","target_children":"same"}"#,
            )
            .is_err()
        );
    }

    #[test]
    fn construction_round_trips_its_optional_root_node() {
        let construction = crate::ScopeConstruction::AdjacencyTree {
            plan: AdjacencyTreePlan::new(
                vec!["row".into()],
                vec!["name".into()],
                vec!["base".into()],
                "name".into(),
                "children".into(),
                Some(42),
            )
            .unwrap(),
        };
        let encoded = serde_json::to_string(&construction).unwrap();
        let decoded: crate::ScopeConstruction = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, construction);
    }
}
