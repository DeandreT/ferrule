//! XML element serialization over an explicitly owned structured read.
//!
//! The owner selects one exact Primary or named endpoint; the path and optional
//! frame are endpoint-local. The schema and document options keep the legacy
//! [`crate::Node::XmlSerialize`] meaning, but every field is required on the
//! wire so a stored owned serializer never acquires an implicit default.

use serde::{Deserialize, Deserializer, Serialize};

use crate::OwnedSourceRef;

/// Serializes the one schema-known element selected from its exact owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedXmlSerialize {
    /// Endpoint-local element path. A frame, when present, prefixes it.
    pub source: OwnedSourceRef,
    /// Exact endpoint-local repetition frame that owns `source.path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Vec<String>>,
    pub schema: Box<ir::SchemaNode>,
    pub declaration: bool,
    pub indent: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_namespace"
    )]
    pub namespace: Option<String>,
}

fn deserialize_namespace<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<String>::deserialize(deserializer)? {
        Some(namespace) if namespace.is_empty() => Err(serde::de::Error::custom(
            "owned XML serializer namespace cannot be empty",
        )),
        namespace => Ok(namespace),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, SourceOwner};
    use ir::{ScalarType, SchemaNode};

    fn serializer(owner: SourceOwner, frame: Option<Vec<String>>) -> OwnedXmlSerialize {
        OwnedXmlSerialize {
            source: OwnedSourceRef {
                owner,
                path: vec!["Orders".into(), "Order".into(), "Detail".into()],
            },
            frame,
            schema: Box::new(SchemaNode::group(
                "Detail",
                vec![SchemaNode::scalar("Code", ScalarType::String)],
            )),
            declaration: false,
            indent: true,
            namespace: Some("urn:detail".into()),
        }
    }

    fn wire(node: &Node) -> serde_json::Value {
        serde_json::to_value(node).expect("serializable owned XML serializer")
    }

    #[test]
    fn owned_serializer_round_trips_exact_owner_frame_and_options() {
        for (owner, frame) in [
            (SourceOwner::Primary, None),
            (
                SourceOwner::Named {
                    name: " audit ".into(),
                },
                Some(vec!["Orders".into(), "Order".into()]),
            ),
            (SourceOwner::Primary, Some(vec![])),
        ] {
            let node = Node::OwnedXmlSerialize(serializer(owner, frame));
            let value = wire(&node);
            assert_eq!(value["kind"], "owned_xml_serialize");
            let decoded: Node = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(wire(&decoded), value);
            assert_eq!(node.owned_source_kind(), Some("owned_xml_serialize"));
            assert!(node.dependencies().is_empty());
        }
    }

    #[test]
    fn owned_serializer_wire_is_strict() {
        let valid = serde_json::json!({
            "kind": "owned_xml_serialize",
            "source": {"owner": {"kind": "primary"}, "path": ["Detail"]},
            "schema": serde_json::to_value(SchemaNode::group("Detail", vec![])).unwrap(),
            "declaration": true,
            "indent": false,
        });
        assert!(serde_json::from_value::<Node>(valid.clone()).is_ok());
        let edit = |change: &dyn Fn(&mut serde_json::Map<String, serde_json::Value>)| {
            let mut value = valid.clone();
            change(value.as_object_mut().expect("object"));
            value
        };
        for invalid in [
            // Every document option is explicit; nothing defaults.
            edit(&|value| {
                value.remove("indent");
            }),
            edit(&|value| {
                value.remove("declaration");
            }),
            edit(&|value| {
                value.remove("schema");
            }),
            edit(&|value| {
                value.remove("source");
            }),
            // A legacy path is not an owner and legacy fields are not accepted.
            edit(&|value| {
                value.insert("path".into(), serde_json::json!(["Detail"]));
            }),
            edit(&|value| {
                value.insert("source".into(), serde_json::json!(["Detail"]));
            }),
            edit(&|value| {
                value.insert(
                    "source".into(),
                    serde_json::json!({"owner": {"kind": "named", "name": ""}, "path": []}),
                );
            }),
            edit(&|value| {
                value.insert("namespace".into(), serde_json::json!(""));
            }),
            edit(&|value| {
                value.insert("owner".into(), serde_json::json!({"kind": "primary"}));
            }),
        ] {
            assert!(
                serde_json::from_value::<Node>(invalid.clone()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn legacy_serializer_wire_is_unchanged_and_old_readers_refuse_the_owned_form() {
        let legacy = Node::XmlSerialize {
            path: vec!["Detail".into()],
            frame: None,
            schema: Box::new(SchemaNode::group("Detail", vec![])),
            declaration: false,
            indent: true,
            namespace: None,
        };
        let value = wire(&legacy);
        assert_eq!(value["kind"], "xml_serialize");
        assert!(value.get("source").is_none());
        // Legacy defaults remain: declaration false and indent true.
        let defaulted: Node = serde_json::from_value(serde_json::json!({
            "kind": "xml_serialize",
            "path": ["Detail"],
            "schema": value["schema"].clone(),
        }))
        .unwrap();
        assert_eq!(wire(&defaulted), value);
        assert_eq!(legacy.owned_source_kind(), None);

        // A reader knowing only the legacy serializer refuses the owned kind
        // instead of dropping or reinterpreting it.
        #[derive(Debug, serde::Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        #[allow(dead_code)]
        enum OldNode {
            XmlSerialize {
                path: Vec<String>,
                #[serde(default)]
                frame: Option<Vec<String>>,
                schema: Box<SchemaNode>,
            },
        }
        let owned = wire(&Node::OwnedXmlSerialize(serializer(
            SourceOwner::Primary,
            None,
        )));
        assert!(serde_json::from_value::<OldNode>(owned.clone()).is_err());
        let mut relabelled = owned;
        relabelled["kind"] = serde_json::json!("xml_serialize");
        assert!(serde_json::from_value::<Node>(relabelled).is_err());
    }
}
