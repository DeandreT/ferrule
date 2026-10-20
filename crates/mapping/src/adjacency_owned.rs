//! Adjacency-tree construction over an explicitly owned input collection.
//!
//! Only the input collection carries an owner. The key and parent paths are
//! relative to each selected row, and the target key/children names describe
//! the generated tree; none of them selects a source. The optional root
//! expression is evaluated in the receiving (parent) context.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{NodeId, OwnedSourceRef};

/// Validated construction of a recursive target group from the rows of one
/// exact endpoint-relative collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedAdjacencyTreePlan {
    collection: OwnedSourceRef,
    key: Vec<String>,
    parent: Vec<String>,
    target_key: String,
    target_children: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<NodeId>,
}

impl OwnedAdjacencyTreePlan {
    /// The collection path is endpoint data and gains no path restriction here;
    /// schema validation decides whether it selects a repeating group. Row and
    /// output fields keep the legacy plan's rules.
    pub fn new(
        collection: OwnedSourceRef,
        key: Vec<String>,
        parent: Vec<String>,
        target_key: String,
        target_children: String,
        root: Option<NodeId>,
    ) -> Option<Self> {
        (valid_row_path(&key)
            && valid_row_path(&parent)
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

    pub fn collection(&self) -> &OwnedSourceRef {
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

fn valid_row_path(path: &[String]) -> bool {
    !path.is_empty() && path.iter().all(|segment| !segment.is_empty())
}

const FIELDS: &[&str] = &[
    "collection",
    "key",
    "parent",
    "target_key",
    "target_children",
    "root",
];

struct PlanVisitor;

impl<'de> Visitor<'de> for PlanVisitor {
    type Value = OwnedAdjacencyTreePlan;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an owned adjacency-tree plan object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut collection = None;
        let mut paths: [Option<Vec<String>>; 2] = Default::default();
        let mut names: [Option<String>; 2] = Default::default();
        let mut root = None;
        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "collection" => {
                    if collection.is_some() {
                        return Err(de::Error::duplicate_field("collection"));
                    }
                    collection = Some(map.next_value::<OwnedSourceRef>()?);
                }
                "key" | "parent" => {
                    let slot = usize::from(field == "parent");
                    if paths[slot].is_some() {
                        return Err(de::Error::duplicate_field(FIELDS[slot + 1]));
                    }
                    paths[slot] = Some(map.next_value::<Vec<String>>()?);
                }
                "target_key" | "target_children" => {
                    let slot = usize::from(field == "target_children");
                    if names[slot].is_some() {
                        return Err(de::Error::duplicate_field(FIELDS[slot + 3]));
                    }
                    names[slot] = Some(map.next_value::<String>()?);
                }
                "root" => {
                    if root.is_some() {
                        return Err(de::Error::duplicate_field("root"));
                    }
                    // An explicit null is the same absent root the writer omits.
                    root = Some(map.next_value::<Option<NodeId>>()?);
                }
                _ => return Err(de::Error::unknown_field(&field, FIELDS)),
            }
        }
        let collection = collection.ok_or_else(|| de::Error::missing_field("collection"))?;
        let [key, parent] = paths;
        let [target_key, target_children] = names;
        let key = key.ok_or_else(|| de::Error::missing_field("key"))?;
        let parent = parent.ok_or_else(|| de::Error::missing_field("parent"))?;
        let target_key = target_key.ok_or_else(|| de::Error::missing_field("target_key"))?;
        let target_children =
            target_children.ok_or_else(|| de::Error::missing_field("target_children"))?;
        OwnedAdjacencyTreePlan::new(
            collection,
            key,
            parent,
            target_key,
            target_children,
            root.flatten(),
        )
        .ok_or_else(|| {
            de::Error::custom(
                "owned adjacency tree requires non-empty distinct key and parent paths and distinct non-empty target fields",
            )
        })
    }
}

impl<'de> Deserialize<'de> for OwnedAdjacencyTreePlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PlanVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AdjacencyTreePlan, Project, ScopeConstruction, SourceOwner, project_file};

    fn plan(owner: SourceOwner, root: Option<NodeId>) -> OwnedAdjacencyTreePlan {
        OwnedAdjacencyTreePlan::new(
            OwnedSourceRef {
                owner,
                path: vec!["Rows".into(), "Row".into()],
            },
            vec!["Key".into()],
            vec!["Link".into(), "Parent".into()],
            "name".into(),
            "children".into(),
            root,
        )
        .expect("valid owned plan")
    }

    #[test]
    fn owned_plan_round_trips_exact_owner_paths_and_root() {
        for (owner, root) in [
            (SourceOwner::Primary, None),
            (
                SourceOwner::Named {
                    name: " audit ".into(),
                },
                Some(7),
            ),
        ] {
            let plan = plan(owner, root);
            let wire = serde_json::to_value(&plan).unwrap();
            assert_eq!(
                serde_json::from_value::<OwnedAdjacencyTreePlan>(wire.clone()).unwrap(),
                plan
            );
            assert_eq!(
                wire["collection"]["path"],
                serde_json::json!(["Rows", "Row"])
            );
            assert_eq!(wire["parent"], serde_json::json!(["Link", "Parent"]));
            assert_eq!(wire.get("root").is_some(), root.is_some());
            let construction = ScopeConstruction::OwnedAdjacencyTree { plan: plan.clone() };
            let wire = serde_json::to_value(&construction).unwrap();
            assert!(wire.get("owned_adjacency_tree").is_some(), "{wire}");
            assert_eq!(
                serde_json::from_value::<ScopeConstruction>(wire).unwrap(),
                construction
            );
        }
        // An empty collection path selects a repeating endpoint root; schema
        // validation decides whether it is a collection.
        let root = OwnedAdjacencyTreePlan::new(
            OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec![],
            },
            vec!["k".into()],
            vec!["p".into()],
            "k".into(),
            "c".into(),
            None,
        );
        assert!(root.is_some());
        let explicit_null = r#"{"collection":{"owner":{"kind":"primary"},"path":[]},"key":["k"],"parent":["p"],"target_key":"k","target_children":"c","root":null}"#;
        assert_eq!(
            serde_json::from_str::<OwnedAdjacencyTreePlan>(explicit_null).unwrap(),
            root.unwrap()
        );
    }

    #[test]
    fn owned_plan_rejects_malformed_owners_fields_and_row_paths() {
        let valid = r#"{"collection":{"owner":{"kind":"primary"},"path":["Row"]},"key":["Key"],"parent":["Parent"],"target_key":"name","target_children":"children","root":3}"#;
        assert!(serde_json::from_str::<OwnedAdjacencyTreePlan>(valid).is_ok());
        let fields =
            r#""key":["Key"],"parent":["Parent"],"target_key":"name","target_children":"children""#;
        let primary = r#"{"owner":{"kind":"primary"},"path":["Row"]}"#;
        let invalid = [
            // Malformed or unknown owners.
            format!(
                r#"{{"collection":{{"owner":{{"kind":"named","name":""}},"path":["Row"]}},{fields}}}"#
            ),
            format!(r#"{{"collection":{{"owner":{{"kind":"named"}},"path":["Row"]}},{fields}}}"#),
            format!(
                r#"{{"collection":{{"owner":{{"kind":"remote","name":"x"}},"path":["Row"]}},{fields}}}"#
            ),
            format!(
                r#"{{"collection":{{"owner":{{"kind":"primary","name":"x"}},"path":["Row"]}},{fields}}}"#
            ),
            // A legacy path is not an owner, and an owned reference is exact.
            format!(r#"{{"collection":["Row"],{fields}}}"#),
            format!(r#"{{"collection":{{"owner":{{"kind":"primary"}}}},{fields}}}"#),
            format!(
                r#"{{"collection":{{"owner":{{"kind":"primary"}},"path":["Row"],"frame":[]}},{fields}}}"#
            ),
            // Row paths and output names keep the legacy rules and are not owners.
            format!(
                r#"{{"collection":{primary},"key":[],"parent":["Parent"],"target_key":"name","target_children":"children"}}"#
            ),
            format!(
                r#"{{"collection":{primary},"key":["Key"],"parent":["Key"],"target_key":"name","target_children":"children"}}"#
            ),
            format!(
                r#"{{"collection":{primary},"key":["Key"],"parent":[""],"target_key":"name","target_children":"children"}}"#
            ),
            format!(
                r#"{{"collection":{primary},"key":["Key"],"parent":["Parent"],"target_key":"same","target_children":"same"}}"#
            ),
            format!(
                r#"{{"collection":{primary},"key":{{"owner":{{"kind":"primary"}},"path":["Key"]}},"parent":["Parent"],"target_key":"name","target_children":"children"}}"#
            ),
            format!(
                r#"{{"collection":{primary},"key":["Key"],"parent":["Parent"],"target_key":"name"}}"#
            ),
            format!(r#"{{"collection":{primary},{fields},"owner":{{"kind":"primary"}}}}"#),
            format!(r#"{{"collection":{primary},{fields},"root":1,"root":2}}"#),
            format!(r#"{{"collection":{primary},"collection":{primary},{fields}}}"#),
        ];
        for invalid in &invalid {
            assert!(
                serde_json::from_str::<OwnedAdjacencyTreePlan>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn legacy_plan_wire_is_unchanged_and_old_readers_refuse_owned_plans() {
        let legacy = AdjacencyTreePlan::new(
            vec!["Row".into()],
            vec!["Key".into()],
            vec!["Parent".into()],
            "name".into(),
            "children".into(),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&ScopeConstruction::AdjacencyTree { plan: legacy }).unwrap(),
            r#"{"adjacency_tree":{"plan":{"collection":["Row"],"key":["Key"],"parent":["Parent"],"target_key":"name","target_children":"children"}}}"#
        );
        // An owned plan cannot be read as the legacy plan or legacy variant.
        let owned = serde_json::to_value(plan(SourceOwner::Primary, Some(1))).unwrap();
        assert!(serde_json::from_value::<AdjacencyTreePlan>(owned.clone()).is_err());
        let wire = serde_json::json!({ "adjacency_tree": { "plan": owned } });
        assert!(serde_json::from_value::<ScopeConstruction>(wire).is_err());

        // A reader that only knows the earlier construction variants rejects
        // the new variant instead of dropping or reinterpreting it.
        #[derive(Debug, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum OldConstruction {
            Constructed,
            CopyCurrentSource,
            AdjacencyTree { plan: AdjacencyTreePlan },
        }
        let construction = ScopeConstruction::OwnedAdjacencyTree {
            plan: plan(SourceOwner::Primary, None),
        };
        let wire = serde_json::to_value(&construction).unwrap();
        assert!(serde_json::from_value::<OldConstruction>(wire).is_err());

        let project = Project {
            source: ir::SchemaNode::group("Input", vec![]),
            target: ir::SchemaNode::group("Output", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Default::default(),
            root: crate::Scope {
                children: vec![crate::Scope {
                    target_field: "Tree".into(),
                    construction: construction.clone(),
                    ..crate::Scope::default()
                }],
                ..crate::Scope::default()
            },
        };
        let text = project_file::encode_pretty(&project).unwrap();
        assert!(text.contains("owned_adjacency_tree"));
        assert_eq!(
            project_file::decode_str(&text).unwrap().root.children[0].construction,
            construction
        );
        assert_eq!(
            crate::first_project_owned_source_scope(&project),
            Some(
                crate::SourceScopeLocation::root(crate::SourceScopeTarget::Primary)
                    .descendant(crate::SourceScopeStep::Child(0))
            )
        );
    }
}
