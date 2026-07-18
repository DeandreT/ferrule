use std::collections::BTreeSet;

use ir::{SchemaKind, XML_TEXT_FIELD};
use mapping::{IterationOutput, ScopeConstruction, XmlMixedContentElement};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, schema_node_at, schema_node_at_resolved};
use super::scope::{IterationNodes, ScopeBuilder};
use super::source::SourcePath;

pub(super) fn install(
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let candidates = target
        .ports
        .iter()
        .filter_map(|(target_key, target_path)| {
            let target_node = schema_node_at(&target.schema, target_path)?;
            if !matches!(target_node.kind, SchemaKind::Group { .. })
                || target_node.text_child().is_none()
            {
                return None;
            }
            let source_key = *builder.edge_from.get(target_key)?;
            let source_path = builder.source_abs_path(source_key)?;
            let source_schema = &builder.sources.get(source_path.source)?.schema;
            let source_node = schema_node_at_resolved(source_schema, &source_path.path)?;
            if !matches!(source_node.kind, SchemaKind::Group { .. })
                || source_node.text_child().is_none()
                || !has_direct_text_wire(target, target_path, &source_path, builder)
            {
                return None;
            }
            let elements = child_mappings(target, target_path, &source_path, builder);
            (!elements.is_empty()).then_some((
                target_path.clone(),
                source_path,
                target_node.repeating,
                elements,
            ))
        })
        .collect::<Vec<_>>();

    for (target_path, source_path, target_repeating, elements) in candidates {
        builder.note_framed_prefixes(&source_path);
        let source = builder.context_path(&source_path);
        scopes.add_iteration(
            &target_path,
            &source,
            IterationNodes::default(),
            if target_repeating {
                IterationOutput::Repeated
            } else {
                IterationOutput::First
            },
        );
        scopes.ensure_scope(&target_path).construction =
            ScopeConstruction::XmlMixedContent { elements };
    }
}

fn has_direct_text_wire(
    target: &SchemaComponent,
    target_path: &[String],
    source_group: &SourcePath,
    builder: &GraphBuilder<'_>,
) -> bool {
    target.ports.iter().any(|(target_key, path)| {
        let Some([target_name]) = path.strip_prefix(target_path) else {
            return false;
        };
        if target_name != XML_TEXT_FIELD {
            return false;
        }
        builder
            .edge_from
            .get(target_key)
            .and_then(|source_key| builder.source_abs_path(*source_key))
            .is_some_and(|source| {
                let suffix = source.path.strip_prefix(source_group.path.as_slice());
                source.source == source_group.source
                    && (matches!(suffix, Some([]))
                        || matches!(suffix, Some([name]) if name == XML_TEXT_FIELD))
            })
    })
}

fn child_mappings(
    target: &SchemaComponent,
    target_path: &[String],
    source_group: &SourcePath,
    builder: &GraphBuilder<'_>,
) -> Vec<XmlMixedContentElement> {
    let mut seen = BTreeSet::new();
    target
        .ports
        .iter()
        .filter_map(|(target_key, path)| {
            let [target_name] = path.strip_prefix(target_path)? else {
                return None;
            };
            let target_node = schema_node_at(&target.schema, path)?;
            if target_node.text
                || !target_node.repeating
                || !matches!(target_node.kind, SchemaKind::Scalar { .. })
            {
                return None;
            }
            let source = builder
                .edge_from
                .get(target_key)
                .and_then(|source_key| builder.source_abs_path(*source_key))?;
            if source.source != source_group.source {
                return None;
            }
            let [source_name] = source.path.strip_prefix(source_group.path.as_slice())? else {
                return None;
            };
            let source_node = builder.schema_node(&source)?;
            if !source_node.repeating || !matches!(source_node.kind, SchemaKind::Scalar { .. }) {
                return None;
            }
            let pair = (source_name.clone(), target_name.clone());
            seen.insert(pair.clone()).then_some(XmlMixedContentElement {
                source: pair.0,
                target: pair.1,
            })
        })
        .collect()
}
