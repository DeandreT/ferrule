//! XML mixed-content atomization over an explicitly owned group read.
//!
//! The owner selects one exact Primary or named endpoint; the group path and
//! optional frame are endpoint-local. Each replacement names its evaluation
//! context explicitly: the receiving parent context, or one owned occurrence of
//! the replaced direct child element. An occurrence's collection is always the
//! group path plus the element name in the same owner, so no second owner or
//! collection path is stored beside the source.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{NodeId, OwnedSourceRef};

/// Atomizes one owned mixed-content group, replacing selected direct child
/// element occurrences with graph-computed values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedXmlMixedContent {
    /// Endpoint-local mixed group path. A frame, when present, prefixes it.
    pub source: OwnedSourceRef,
    /// Exact endpoint-local repetition frame that owns `source.path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Vec<String>>,
    /// Replacement rules, matched by direct child element name.
    pub replacements: Vec<OwnedXmlMixedContentReplacement>,
}

impl OwnedXmlMixedContent {
    /// The owned collection whose occurrences an occurrence-context
    /// replacement of `element` evaluates in.
    pub fn occurrence_collection(&self, element: &str) -> Vec<String> {
        let mut collection = self.source.path.clone();
        collection.push(element.to_owned());
        collection
    }
}

/// One direct-element replacement of [`OwnedXmlMixedContent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedXmlMixedContentReplacement {
    #[serde(deserialize_with = "deserialize_element")]
    pub element: String,
    pub context: OwnedXmlMixedContentContext,
    pub expression: NodeId,
}

/// Where a replacement expression evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedXmlMixedContentContext {
    /// Once per matching occurrence, in the receiving context.
    Parent,
    /// Once per matching occurrence, with that occurrence as the latest frame
    /// of the source owner at the group path plus the element name.
    Occurrence,
}

fn deserialize_element<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let element = String::deserialize(deserializer)?;
    if element.is_empty() {
        return Err(serde::de::Error::custom(
            "owned XML mixed-content replacement element cannot be empty",
        ));
    }
    Ok(element)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, SourceOwner, XmlMixedContentReplacement};

    fn mixed(owner: SourceOwner, frame: Option<Vec<String>>) -> OwnedXmlMixedContent {
        OwnedXmlMixedContent {
            source: OwnedSourceRef {
                owner,
                path: vec!["Doc".into(), "Para".into()],
            },
            frame,
            replacements: vec![
                OwnedXmlMixedContentReplacement {
                    element: "b".into(),
                    context: OwnedXmlMixedContentContext::Occurrence,
                    expression: 3,
                },
                OwnedXmlMixedContentReplacement {
                    element: "i".into(),
                    context: OwnedXmlMixedContentContext::Parent,
                    expression: 4,
                },
            ],
        }
    }

    fn wire(node: &Node) -> serde_json::Value {
        serde_json::to_value(node).expect("serializable owned mixed content")
    }

    #[test]
    fn owned_mixed_content_round_trips_exact_owner_frame_and_contexts() {
        for (owner, frame) in [
            (SourceOwner::Primary, None),
            (
                SourceOwner::Named {
                    name: " notes ".into(),
                },
                Some(vec!["Doc".into(), "Para".into()]),
            ),
            (SourceOwner::Primary, Some(vec![])),
        ] {
            let node = Node::OwnedXmlMixedContent(mixed(owner, frame));
            let value = wire(&node);
            assert_eq!(value["kind"], "owned_xml_mixed_content");
            assert_eq!(value["replacements"][0]["context"], "occurrence");
            assert_eq!(value["replacements"][1]["context"], "parent");
            let decoded: Node = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(wire(&decoded), value);
            assert_eq!(node.owned_source_kind(), Some("owned_xml_mixed_content"));
            // Replacements are the only graph inputs, in rule order.
            assert_eq!(node.dependencies(), vec![3, 4]);
        }
        assert_eq!(
            mixed(SourceOwner::Primary, None).occurrence_collection("b"),
            ["Doc", "Para", "b"]
        );
    }

    #[test]
    fn owned_mixed_content_wire_is_strict() {
        let valid = serde_json::json!({
            "kind": "owned_xml_mixed_content",
            "source": {"owner": {"kind": "primary"}, "path": ["Para"]},
            "replacements": [{"element": "b", "context": "parent", "expression": 2}],
        });
        assert!(serde_json::from_value::<Node>(valid.clone()).is_ok());
        let edit = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut value = valid.clone();
            change(&mut value);
            value
        };
        for invalid in [
            // Replacements and each replacement context are explicit.
            edit(&|value| {
                value.as_object_mut().unwrap().remove("replacements");
            }),
            edit(&|value| {
                value["replacements"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("context");
            }),
            edit(&|value| value["replacements"][0]["context"] = "item".into()),
            edit(&|value| value["replacements"][0]["element"] = "".into()),
            // A legacy collection path is not an occurrence context.
            edit(&|value| {
                value["replacements"][0]["collection"] = serde_json::json!(["Para", "b"]);
            }),
            // Legacy paths and owners beside the source are refused.
            edit(&|value| value["path"] = serde_json::json!(["Para"])),
            edit(&|value| value["source"] = serde_json::json!(["Para"])),
            edit(&|value| value["owner"] = serde_json::json!({"kind": "primary"})),
            edit(&|value| {
                value["source"] =
                    serde_json::json!({"owner": {"kind": "named", "name": ""}, "path": []});
            }),
        ] {
            assert!(
                serde_json::from_value::<Node>(invalid.clone()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn legacy_mixed_content_wire_is_unchanged_and_old_readers_refuse_the_owned_form() {
        let legacy = Node::XmlMixedContent {
            path: vec!["Para".into()],
            frame: None,
            replacements: vec![XmlMixedContentReplacement {
                element: "b".into(),
                collection: vec!["Para".into(), "b".into()],
                expression: 2,
            }],
        };
        let value = wire(&legacy);
        assert_eq!(value["kind"], "xml_mixed_content");
        assert!(value.get("source").is_none());
        assert!(value["replacements"][0].get("context").is_none());
        // Legacy parent-context replacements still omit their collection.
        let parent: Node = serde_json::from_value(serde_json::json!({
            "kind": "xml_mixed_content",
            "path": ["Para"],
            "replacements": [{"element": "b", "expression": 2}],
        }))
        .unwrap();
        assert!(wire(&parent)["replacements"][0].get("collection").is_none());
        assert_eq!(legacy.owned_source_kind(), None);

        // A reader that knows only the legacy node refuses the owned kind
        // instead of dropping or reinterpreting it.
        #[derive(Debug, serde::Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        #[allow(dead_code)]
        enum OldNode {
            XmlMixedContent {
                path: Vec<String>,
                #[serde(default)]
                frame: Option<Vec<String>>,
                replacements: Vec<XmlMixedContentReplacement>,
            },
        }
        let owned = wire(&Node::OwnedXmlMixedContent(mixed(
            SourceOwner::Primary,
            None,
        )));
        assert!(serde_json::from_value::<OldNode>(owned.clone()).is_err());
        let mut relabelled = owned;
        relabelled["kind"] = serde_json::json!("xml_mixed_content");
        assert!(serde_json::from_value::<Node>(relabelled).is_err());
    }
}
