//! Source component ownership and port resolution for MFD export.

use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};
use mapping::{FormatOptions, NodeId, Project};

use crate::MfdError;

use super::external_source;
use super::schema::{KeyAlloc, PortMatch, PortPairMatch, PortTree, SideFormat, side_format};
use super::xbrl;

pub(super) struct SourceExport<'a> {
    pub(super) name: &'a str,
    pub(super) schema: &'a SchemaNode,
    pub(super) path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) format: SideFormat,
    pub(super) ports: PortTree,
    pub(super) request_ports: Option<PortTree>,
    pub(super) dynamic_path_node: Option<NodeId>,
    pub(super) document_path_port: Option<u32>,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: String,
}

pub(super) struct SourceExports<'a> {
    primary: SourceExport<'a>,
    extras: Vec<SourceExport<'a>>,
}

/// Native schema identity for one XML collection materialization.
pub(super) struct XmlSequenceIdentity {
    pub(super) source_uid: u32,
    pub(super) collection_port: u32,
    pub(super) instance_root: String,
    pub(super) namespaces: BTreeMap<u32, Option<String>>,
    pub(super) parent_ports: Vec<u32>,
}

pub(super) struct JoinCollection<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) port: u32,
}

impl<'a> SourceExports<'a> {
    pub(super) fn build(project: &'a Project, keys: &mut KeyAlloc) -> Result<Self, MfdError> {
        let primary = build_source(
            &project.source.name,
            &project.source,
            project.source_path.as_deref(),
            &project.source_options,
            None,
            0,
            keys,
        )?;
        let mut extras = Vec::with_capacity(project.extra_sources.len());
        for (index, source) in project.extra_sources.iter().enumerate() {
            extras.push(build_source(
                &source.name,
                &source.schema,
                (!source.path.is_empty()).then_some(source.path.as_str()),
                &source.options,
                source.dynamic_path.as_ref().map(|dynamic| dynamic.node),
                index + 1,
                keys,
            )?);
        }
        Ok(Self { primary, extras })
    }

    pub(super) fn len(&self) -> usize {
        self.extras.len() + 1
    }

    pub(super) const fn primary_component_uid(&self) -> u32 {
        self.primary.component_uid
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &SourceExport<'a>> {
        std::iter::once(&self.primary).chain(&self.extras)
    }

    pub(super) fn xml_sequence_identity_for_port(
        &self,
        port: u32,
    ) -> Result<Option<XmlSequenceIdentity>, MfdError> {
        fn find(
            node: &SchemaNode,
            path: &mut Vec<String>,
            ports: &PortTree,
            port: u32,
        ) -> Option<Vec<String>> {
            if matches!(node.kind, SchemaKind::Group { .. })
                && ports.key_for_abs(path) == Some(port)
            {
                return Some(path.clone());
            }
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    let found = find(child, path, ports, port);
                    path.pop();
                    if found.is_some() {
                        return found;
                    }
                }
            }
            None
        }
        for (index, source) in self.iter().enumerate() {
            if let Some(mut path) = find(source.schema, &mut Vec::new(), &source.ports, port) {
                if index > 0 {
                    path.insert(0, source.name.to_string());
                }
                return self.xml_sequence_identity(&path);
            }
        }
        Ok(None)
    }

    pub(super) fn xml_sequence_identity(
        &self,
        path: &[String],
    ) -> Result<Option<XmlSequenceIdentity>, MfdError> {
        let (source, _, local) = self.owner(path);
        if source.format != SideFormat::Xml
            || source.options.wsdl.is_some()
            || source.options.http_get.is_some()
            || source.options.external_source.is_some()
            || source.options.local_xml_file_set
            || source.dynamic_path_node.is_some()
        {
            return Ok(None);
        }
        let root_namespace = format_xml::xsd::export_namespace(source.schema)?;
        let mut instance_root = format!(
            "{{{}}}{}",
            root_namespace.as_deref().unwrap_or_default(),
            source.schema.name
        );
        let mut node = source.schema;
        let mut parent_ports = Vec::new();
        let mut prefix = Vec::new();
        for segment in local {
            if let Some(port) = source.ports.key_for_abs(&prefix) {
                parent_ports.push(port);
            }
            prefix.push(segment.clone());
            let Some(child) = node.child(segment) else {
                return Ok(None);
            };
            let namespace = child
                .xml_namespace
                .as_ref()
                .and_then(ir::XmlNamespace::uri)
                .unwrap_or_default();
            instance_root.push_str(&format!("/{{{namespace}}}{}", child.name));
            node = child;
        }
        if node.attribute || node.text || !matches!(node.kind, SchemaKind::Group { .. }) {
            return Ok(None);
        }
        let Some(collection_port) = source.ports.key_for_abs(local) else {
            return Ok(None);
        };
        let mut namespaces = BTreeMap::new();
        fn collect(
            node: &SchemaNode,
            path: &mut Vec<String>,
            ports: &PortTree,
            namespaces: &mut BTreeMap<u32, Option<String>>,
        ) {
            if !node.text
                && let Some(port) = ports.key_for_abs(path)
            {
                namespaces.insert(
                    port,
                    node.xml_namespace
                        .as_ref()
                        .and_then(ir::XmlNamespace::uri)
                        .map(str::to_string),
                );
            }
            if let SchemaKind::Group { children, .. } = &node.kind {
                for child in children {
                    path.push(child.name.clone());
                    collect(child, path, ports, namespaces);
                    path.pop();
                }
            }
        }
        collect(node, &mut local.to_vec(), &source.ports, &mut namespaces);
        if local.is_empty() {
            namespaces.insert(collection_port, root_namespace);
        }
        Ok(Some(XmlSequenceIdentity {
            source_uid: source.component_uid,
            collection_port,
            instance_root,
            namespaces,
            parent_ports,
        }))
    }

    pub(super) fn key_for_abs(&self, path: &[String]) -> Option<u32> {
        let (source, _, local) = self.owner(path);
        source.ports.key_for_abs(local)
    }

    pub(super) fn key_for_alternative(&self, path: &[String], name: &str) -> Option<u32> {
        let (source, _, local) = self.owner(path);
        source.ports.key_for_alternative(local, name)
    }

    pub(super) fn match_field(&self, path: &[String], pinned: bool) -> PortMatch {
        let (source, primary, local) = self.owner(path);
        if !primary || pinned {
            source
                .ports
                .key_for_abs(local)
                .map_or(PortMatch::Missing, PortMatch::Unique)
        } else if let Some(key) = source.ports.key_for_abs(local) {
            PortMatch::Unique(key)
        } else {
            source.ports.match_suffix(local)
        }
    }

    pub(super) fn match_document_path(&self) -> PortMatch {
        let mut matches = self.iter().filter_map(|source| source.document_path_port);
        let Some(first) = matches.next() else {
            return PortMatch::Missing;
        };
        if matches.next().is_some() {
            PortMatch::Ambiguous
        } else {
            PortMatch::Unique(first)
        }
    }

    pub(super) fn match_sequence(&self, path: &[String]) -> PortMatch {
        let (source, primary, local) = self.owner(path);
        if !primary {
            return source
                .ports
                .key_for_abs(local)
                .map_or(PortMatch::Missing, PortMatch::Unique);
        }
        let mut unique = None;
        for source in self.iter() {
            match source.ports.match_suffix(local) {
                PortMatch::Missing => {}
                PortMatch::Unique(key) if unique.is_none() => unique = Some(key),
                PortMatch::Unique(_) | PortMatch::Ambiguous => return PortMatch::Ambiguous,
            }
        }
        unique.map_or(PortMatch::Missing, PortMatch::Unique)
    }

    pub(super) fn lookup_ports(
        &self,
        collection: &[String],
        key: &[String],
        value: &[String],
    ) -> PortPairMatch {
        let (source, primary, local) = self.owner(collection);
        if primary {
            source.ports.match_collection_pair(local, key, value)
        } else {
            let mut key_path = local.to_vec();
            key_path.extend(key.iter().cloned());
            let mut value_path = local.to_vec();
            value_path.extend(value.iter().cloned());
            match (
                source.ports.key_for_abs(&key_path),
                source.ports.key_for_abs(&value_path),
            ) {
                (Some(key), Some(value)) => PortPairMatch::Unique(key, value),
                _ => PortPairMatch::Missing,
            }
        }
    }

    pub(super) fn schema_node_at(&self, path: &[String]) -> Option<&SchemaNode> {
        let (source, _, local) = self.owner(path);
        let mut node = source.schema;
        for segment in local {
            node = node.child(segment)?;
        }
        Some(node)
    }

    /// Resolves one join input against exactly one exported source component.
    /// Named sources must use their explicit source-name prefix, so schema and
    /// port ownership cannot diverge through suffix matching.
    pub(super) fn join_collection(&self, path: &[String]) -> Option<JoinCollection<'_>> {
        let (source, local) = match path.first() {
            Some(name) => {
                let mut matches = self.extras.iter().filter(|source| source.name == name);
                match (matches.next(), matches.next()) {
                    (Some(source), None) => (source, &path[1..]),
                    (None, None) => (&self.primary, path),
                    _ => return None,
                }
            }
            None => (&self.primary, path),
        };
        let schema = local
            .iter()
            .try_fold(source.schema, |node, segment| node.child(segment))?;
        let port = source.ports.key_for_abs(local)?;
        Some(JoinCollection { schema, port })
    }

    /// Resolves a scope's source as relative to its active anchor, falling
    /// back to an absolute primary-source path only when the relative path is
    /// invalid. This mirrors engine source traversal without guessing when
    /// both interpretations are valid.
    pub(super) fn resolve_scope_path(
        &self,
        anchor: &[String],
        source: &[String],
    ) -> (Vec<String>, bool) {
        let mut relative = anchor.to_vec();
        relative.extend(source.iter().cloned());
        if !anchor.is_empty()
            && self.schema_node_at(&relative).is_none()
            && self.schema_node_at(source).is_some()
        {
            (source.to_vec(), true)
        } else {
            (relative, false)
        }
    }

    pub(super) fn is_named_extra_path(&self, path: &[String]) -> bool {
        path.first().is_some_and(|name| {
            self.extras
                .iter()
                .any(|source| source.name == name.as_str())
        })
    }

    pub(super) fn owner_with_index<'s, 'p>(
        &'s self,
        path: &'p [String],
    ) -> (usize, &'s SourceExport<'a>, &'p [String]) {
        if let Some(name) = path.first()
            && let Some((index, source)) = self
                .extras
                .iter()
                .enumerate()
                .find(|(_, source)| source.name == name)
        {
            return (index + 1, source, &path[1..]);
        }
        (0, &self.primary, path)
    }

    fn owner<'s, 'p>(&'s self, path: &'p [String]) -> (&'s SourceExport<'a>, bool, &'p [String]) {
        if let Some(name) = path.first()
            && let Some(source) = self.extras.iter().find(|source| source.name == name)
        {
            return (source, false, &path[1..]);
        }
        (&self.primary, true, path)
    }
}

fn build_source<'a>(
    name: &'a str,
    schema: &'a SchemaNode,
    path: Option<&'a str>,
    options: &'a FormatOptions,
    dynamic_path_node: Option<NodeId>,
    index: usize,
    keys: &mut KeyAlloc,
) -> Result<SourceExport<'a>, MfdError> {
    let component_uid = u32::try_from(index + 2)
        .map_err(|_| MfdError::Unsupported("too many source components for .mfd export".into()))?;
    let format = if options.http_get.is_some() {
        SideFormat::Xml
    } else {
        side_format(&path.map(str::to_string), options)
    };
    let explicit_text = xbrl::explicit_text_ports(schema, options);
    let ports = PortTree::build_with_explicit_text(schema, keys, &explicit_text);
    let request_ports = external_source::request_ports(options, keys);
    let document_path_port = options.local_xml_file_set.then(|| keys.next());
    Ok(SourceExport {
        name,
        schema,
        path,
        options,
        format,
        ports,
        request_ports,
        dynamic_path_node,
        document_path_port,
        component_uid,
        sibling_suffix: if index == 0 {
            "source".to_string()
        } else {
            format!("source-{}", index + 1)
        },
    })
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};
    use mapping::FormatOptions;

    use super::{SourceExports, build_source};
    use crate::export::schema::{KeyAlloc, PortMatch};

    #[test]
    fn exact_primary_path_wins_before_ambiguous_suffix_fallback() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Nested",
                    vec![SchemaNode::scalar("Name", ScalarType::String)],
                ),
            ],
        );
        let options = FormatOptions::default();
        let mut keys = KeyAlloc { next: 1 };
        let Ok(primary) = build_source(
            "Root",
            &schema,
            Some("source.xml"),
            &options,
            None,
            0,
            &mut keys,
        ) else {
            panic!("ordinary XML source should build");
        };
        let sources = SourceExports {
            primary,
            extras: Vec::new(),
        };

        assert!(matches!(
            sources.match_field(&["Name".into()], false),
            PortMatch::Unique(_)
        ));
    }

    #[test]
    fn scope_paths_fall_back_to_absolute_only_when_relative_is_invalid() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![SchemaNode::group("Detail", Vec::new()).repeating()],
                )
                .repeating(),
            ],
        );
        let options = FormatOptions::default();
        let mut keys = KeyAlloc { next: 1 };
        let Ok(primary) = build_source(
            "Root",
            &schema,
            Some("source.xml"),
            &options,
            None,
            0,
            &mut keys,
        ) else {
            panic!("ordinary XML source should build");
        };
        let sources = SourceExports {
            primary,
            extras: Vec::new(),
        };

        assert_eq!(
            sources.resolve_scope_path(&["Item".into()], &["Detail".into()]),
            (vec!["Item".into(), "Detail".into()], false)
        );
        assert_eq!(
            sources.resolve_scope_path(&["Item".into()], &["Item".into()]),
            (vec!["Item".into()], true)
        );
    }
    #[test]
    fn xml_sequence_identity_retains_expanded_nested_and_attribute_names() {
        let mut value = SchemaNode::scalar("Value", ScalarType::String);
        value.xml_namespace = ir::XmlNamespace::qualified("urn:value");
        let mut attribute = SchemaNode::scalar("code", ScalarType::String);
        attribute.attribute = true;
        let mut items = SchemaNode::group("Item", vec![value, attribute]).repeating();
        items.xml_namespace = ir::XmlNamespace::qualified("urn:item");
        let mut schema = SchemaNode::group("Root", vec![items]);
        schema.xml_namespace = ir::XmlNamespace::qualified("urn:root");
        let options = FormatOptions::default();
        let mut keys = KeyAlloc { next: 1 };
        let primary = build_source(
            "Root",
            &schema,
            Some("source.xml"),
            &options,
            None,
            0,
            &mut keys,
        )
        .expect("XML source");
        let sources = SourceExports {
            primary,
            extras: Vec::new(),
        };
        let path = ["Item".to_string()];
        let identity = sources
            .xml_sequence_identity(&path)
            .expect("valid namespaces")
            .expect("XML group");
        assert_eq!(identity.source_uid, 2);
        assert_eq!(identity.instance_root, "{urn:root}Root/{urn:item}Item");
        assert_eq!(
            identity.namespaces[&sources.key_for_abs(&path).unwrap()],
            Some("urn:item".to_string())
        );
        assert_eq!(
            identity.namespaces[&sources
                .key_for_abs(&["Item".into(), "Value".into()])
                .unwrap()],
            Some("urn:value".to_string())
        );
        assert_eq!(
            identity.namespaces[&sources
                .key_for_abs(&["Item".into(), "code".into()])
                .unwrap()],
            None
        );
        assert!(
            sources
                .xml_sequence_identity(&["Item".into(), "Value".into()])
                .unwrap()
                .is_none()
        );
        assert!(
            sources
                .xml_sequence_identity(&["Missing".into()])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn xml_sequence_identity_keeps_named_source_ownership_and_rejects_other_formats() {
        let primary_schema = SchemaNode::group(
            "Primary",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        );
        let named_schema = SchemaNode::group(
            "External",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        );
        let options = FormatOptions::default();
        let mut keys = KeyAlloc { next: 1 };
        let primary = build_source(
            "Primary",
            &primary_schema,
            Some("source.xml"),
            &options,
            None,
            0,
            &mut keys,
        )
        .expect("primary");
        let named = build_source(
            "Aux",
            &named_schema,
            Some("named.xml"),
            &options,
            None,
            1,
            &mut keys,
        )
        .expect("named");
        let sources = SourceExports {
            primary,
            extras: vec![named],
        };
        let identity = sources
            .xml_sequence_identity(&["Aux".into(), "Rows".into()])
            .unwrap()
            .unwrap();
        assert_eq!(identity.source_uid, 3);
        assert_eq!(identity.instance_root, "{}External/{}Rows");
        let by_port = sources
            .xml_sequence_identity_for_port(identity.collection_port)
            .unwrap()
            .unwrap();
        assert_eq!(by_port.source_uid, 3);
        assert_eq!(by_port.instance_root, identity.instance_root);
        let primary_identity = sources
            .xml_sequence_identity(&["Rows".into()])
            .unwrap()
            .unwrap();
        assert_eq!(primary_identity.source_uid, 2);
        assert_ne!(primary_identity.collection_port, identity.collection_port);
        assert_eq!(
            sources
                .xml_sequence_identity_for_port(primary_identity.collection_port)
                .unwrap()
                .unwrap()
                .source_uid,
            2
        );
        let primary = build_source(
            "Primary",
            &named_schema,
            Some("source.json"),
            &options,
            None,
            0,
            &mut keys,
        )
        .expect("JSON source");
        let sources = SourceExports {
            primary,
            extras: Vec::new(),
        };
        assert!(
            sources
                .xml_sequence_identity(&["Rows".into()])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn xml_sequence_identity_declines_document_set_and_dynamic_source_boundaries() {
        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        );
        for (options, dynamic) in [
            (
                FormatOptions {
                    local_xml_file_set: true,
                    ..FormatOptions::default()
                },
                None,
            ),
            (FormatOptions::default(), Some(99)),
        ] {
            let mut keys = KeyAlloc { next: 1 };
            let primary = build_source(
                "Root",
                &schema,
                Some("input.xml"),
                &options,
                dynamic,
                0,
                &mut keys,
            )
            .expect("XML boundary");
            let sources = SourceExports {
                primary,
                extras: Vec::new(),
            };
            assert!(
                sources
                    .xml_sequence_identity(&["Rows".into()])
                    .unwrap()
                    .is_none()
            );
        }
    }
}
