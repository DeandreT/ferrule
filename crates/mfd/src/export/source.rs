//! Source component ownership and port resolution for MFD export.

use ir::SchemaNode;
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
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: String,
}

pub(super) struct SourceExports<'a> {
    primary: SourceExport<'a>,
    extras: Vec<SourceExport<'a>>,
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

    pub(super) fn iter(&self) -> impl Iterator<Item = &SourceExport<'a>> {
        std::iter::once(&self.primary).chain(&self.extras)
    }

    pub(super) fn primary_ports(&self) -> &PortTree {
        &self.primary.ports
    }

    pub(super) fn key_for_abs(&self, path: &[String]) -> Option<u32> {
        let (source, _, local) = self.owner(path);
        source.ports.key_for_abs(local)
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

    pub(super) fn is_named_extra_path(&self, path: &[String]) -> bool {
        path.first().is_some_and(|name| {
            self.extras
                .iter()
                .any(|source| source.name == name.as_str())
        })
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
    Ok(SourceExport {
        name,
        schema,
        path,
        options,
        format,
        ports,
        request_ports,
        dynamic_path_node,
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
}
