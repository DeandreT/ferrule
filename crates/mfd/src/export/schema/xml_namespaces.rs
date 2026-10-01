//! Namespace identities for file-backed XML component entry trees.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ir::{SchemaKind, SchemaNode, XmlNamespace};

use super::xml_escape;

const WRAPPER_NAMESPACE: &str = "http://www.altova.com/mapforce";

/// Slot zero is unqualified; slot one always owns the reserved file wrappers.
/// User namespace identities are deterministic and may reuse the reserved URI.
pub(in crate::export) struct XmlNamespaces {
    slots: BTreeMap<String, usize>,
    root_namespace: Option<String>,
}

impl XmlNamespaces {
    pub(super) fn new(schema: &SchemaNode, exported_namespace: Option<&str>) -> Self {
        fn collect(node: &SchemaNode, namespaces: &mut BTreeSet<String>) {
            if !node.text {
                for namespace in node.xml_namespace.iter().chain(&node.xml_name_alternatives) {
                    if let XmlNamespace::Qualified(namespace) = namespace {
                        namespaces.insert(namespace.as_str().to_string());
                    }
                }
            }
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    collect(child, namespaces);
                }
            }
        }

        let root_namespace = match &schema.xml_namespace {
            Some(XmlNamespace::Qualified(namespace)) => Some(namespace.as_str().to_string()),
            Some(XmlNamespace::Unqualified) => None,
            None => exported_namespace
                .filter(|namespace| !namespace.is_empty())
                .map(str::to_string),
        };
        let mut namespaces = BTreeSet::new();
        collect(schema, &mut namespaces);
        namespaces.extend(root_namespace.iter().cloned());
        namespaces.remove(WRAPPER_NAMESPACE);
        let mut slots = BTreeMap::from([(WRAPPER_NAMESPACE.to_string(), 1)]);
        slots.extend(
            namespaces
                .into_iter()
                .enumerate()
                .map(|(index, namespace)| (namespace, index + 2)),
        );
        Self {
            slots,
            root_namespace,
        }
    }

    pub(super) fn header(&self) -> String {
        let mut namespaces = self.slots.iter().collect::<Vec<_>>();
        namespaces.sort_by_key(|(_, slot)| **slot);
        let mut output = String::from("<header><namespaces><namespace/>");
        for (namespace, _) in namespaces {
            let _ = write!(output, "<namespace uid=\"{}\"/>", xml_escape(namespace));
        }
        output.push_str("</namespaces></header>");
        output
    }

    pub(super) fn entry_attribute(&self, node: &SchemaNode, root: bool) -> String {
        if node.text {
            return String::new();
        }
        let namespace = match &node.xml_namespace {
            Some(XmlNamespace::Qualified(namespace)) => Some(namespace.as_str()),
            Some(XmlNamespace::Unqualified) => None,
            None => root.then_some(self.root_namespace.as_deref()).flatten(),
        };
        namespace.map_or_else(String::new, |namespace| {
            let slot = self.slots[namespace];
            format!(" ns=\"{slot}\"")
        })
    }
}
